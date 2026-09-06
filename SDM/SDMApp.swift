//
//  SDMApp.swift
//  SDM
//
//  Created by Shayan Ostadhassan on 8/3/26.
//

import AppKit
import SDMCore
import SDMEngine
import SDMGrabber
import SwiftUI

/// Exists for one reason: to flush durable state on quit.
///
/// SwiftUI gives a `WindowGroup` no termination hook, and `EngineController`'s
/// heartbeat deliberately does not shut the engine down on its own — see
/// `EngineController.startHeartbeatIfNeeded()`, since closing the main window
/// does not mean the app is quitting. Since `flush()` is the only thing that
/// writes `state.json`, something has to call it reliably at real
/// termination, or the ordinary quit path writes nothing at all —
/// `.incomplete` files and their `.sdmpart` sidecars would survive with no
/// record of the item owning them, and the next launch would restore an
/// empty list.
///
/// `applicationWillTerminate` is the one callback AppKit guarantees before a
/// normal quit, and it is synchronous, so the flush blocks here.
final class AppDelegate: NSObject, NSApplicationDelegate {
    /// Assigned by the scene once the controller exists. Weak so the delegate,
    /// which AppKit keeps for the process lifetime, does not decide the
    /// controller's.
    weak var controller: EngineController?
    weak var activationPolicyController: ActivationPolicyController?

    func applicationDidFinishLaunching(_ notification: Notification) {
        NotificationCenter.default.addObserver(
            self, selector: #selector(windowVisibilityChanged(_:)),
            name: NSWindow.willCloseNotification, object: nil)
        NotificationCenter.default.addObserver(
            self, selector: #selector(windowVisibilityChanged(_:)),
            name: NSWindow.didBecomeMainNotification, object: nil)
        activationPolicyController?.apply()
    }

    /// Spec §10.2's "Menu bar only" mode toggles the dock icon on every
    /// window open/close. Deferred one runloop turn: `willCloseNotification`
    /// fires before the window is actually removed from `NSApp.windows`, so
    /// checking visibility synchronously here would see the closing window
    /// as still open.
    @objc private func windowVisibilityChanged(_ notification: Notification) {
        DispatchQueue.main.async { [weak self] in
            self?.activationPolicyController?.apply()
        }
    }

    /// Dock-icon click when the app has no visible windows (`.accessory`
    /// mode with a closed window). Restores dock visibility per spec §10.2's
    /// "Reopen via: menu bar icon (policy → `.regular`)" — the equivalent
    /// gesture for `.accessory` apps is a dock-icon click if one is still
    /// showing, or the menu bar icon itself, which independently opens the
    /// window and lets `windowVisibilityChanged` restore the policy.
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool)
        -> Bool
    {
        activationPolicyController?.apply()
        return true
    }

    /// Spec §10.2: "Quitting with active non-resumable downloads shows a
    /// confirmation, since that progress cannot be recovered." `runModal()`
    /// blocks synchronously — the same "blocking is the point" reasoning
    /// `EngineController.shutdownBlocking` already documents for the
    /// termination path this gates — so no `.terminateLater` bookkeeping is
    /// needed.
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let controller, hasActiveNonResumableDownloads(controller) else {
            return .terminateNow
        }
        let alert = NSAlert()
        alert.messageText = "Quit with active downloads in progress?"
        alert.informativeText =
            "One or more downloads cannot be resumed. Quitting now will lose their progress permanently."
        alert.addButton(withTitle: "Quit")
        alert.addButton(withTitle: "Cancel")
        alert.alertStyle = .warning
        let response = alert.runModal()
        return response == .alertFirstButtonReturn ? .terminateNow : .terminateCancel
    }

    private func hasActiveNonResumableDownloads(_ controller: EngineController) -> Bool {
        controller.snapshot.packages.flatMap(\.items).contains {
            $0.state == .running && $0.isResumable == false
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        controller?.shutdownBlocking()
    }
}

@main
struct SDMApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var controller: EngineController
    @State private var grabberController: GrabberController
    @State private var themeStore = ThemeStore()
    @State private var activationPolicyController: ActivationPolicyController
    @State private var launchAtLoginController = LaunchAtLoginController()
    @State private var clipboardWatcher: ClipboardWatcher
    /// Link ids already handed to the download engine by auto-add-and-start,
    /// so a later snapshot change (e.g. an unrelated link finishing its
    /// probe) does not re-add the same package a second time — `addPackage`
    /// has no idempotency of its own.
    @State private var autoAddedLinkIDs: Set<UUID> = []
    @State private var sidebarSelection: MainWindowView.SidebarItem? = .downloads
    @State private var notificationManager: NotificationManager
    @State private var notifiedLinkIDs: Set<UUID> = []
    @State private var menuBarIconController = MenuBarIconController()
    @State private var managedBinaries: ManagedBinariesController
    @Environment(\.openWindow) private var openWindow

    init() {
        let notification = NotificationManager()
        let managed = ManagedBinariesController(
            notify: { body in Task { @MainActor in notification.postInfo(body) } })
        let engine = EngineController(
            notificationManager: notification, binaryLocator: managed.binaryLocator)
        let clipboard = ClipboardWatcher()
        let grabber = GrabberController(
            binaryLocator: managed.binaryLocator, managedBinaries: managed)
        let activationPolicy = ActivationPolicyController()
        _notificationManager = State(initialValue: notification)
        _managedBinaries = State(initialValue: managed)
        _controller = State(initialValue: engine)
        _clipboardWatcher = State(initialValue: clipboard)
        _grabberController = State(initialValue: grabber)
        _activationPolicyController = State(initialValue: activationPolicy)

        appDelegate.controller = engine
        appDelegate.activationPolicyController = activationPolicy

        engine.startHeartbeatIfNeeded()
        managed.start()
        clipboard.onLinksDetected = { urls in
            guard GrabberSettings.clipboardWatchingEnabled else { return }
            Task { await grabber.ingest(urls: urls) }
        }
        if GrabberSettings.clipboardWatchingEnabled { clipboard.start() }

        do {
            try ChromeNativeHostInstaller.install()
        } catch {
            print("Failed to install Chrome native host: \(error)")
        }
    }

    var body: some Scene {
        let _ = {
            notificationManager.onSideBarChangeRequest = {
                switch $0 {
                case "linkgrabber":
                    sidebarSelection = .linkgrabber
                case "downloads":
                    sidebarSelection = .downloads
                case "completed":
                    sidebarSelection = .completed
                default:
                    break
                }
                openWindow(id: "main")
                NSApp.activate(ignoringOtherApps: true)
            }
        }()
        // `Window`, not `WindowGroup`: a group allows unbounded duplicate
        // windows (each `openWindow(id:)` call — or the group's own default
        // "New Window" command — opens another), which is how "Open SDM"
        // from the menu bar used to spawn a fresh window every time instead
        // of surfacing the existing one. `Window` is SwiftUI's single-
        // instance scene — `openWindow(id:)` against an already-open one
        // activates it instead.
        Window("Shayan's Download Manager", id: "main") {
            MainWindowView(selection: $sidebarSelection)
                .environment(controller)
                .environment(grabberController)
                .environment(themeStore)
                .environment(clipboardWatcher)
                .onOpenURL(perform: { url in
                    sidebarSelection = .linkgrabber
                    Task { await handleOpenUrl(url) }
                })
        }
        .onChange(of: grabberController.snapshot) { _, newSnapshot in
            let freshIDs = Set(newSnapshot.links.map(\.id) + newSnapshot.mediaRows.map(\.id))
                .subtracting(notifiedLinkIDs)
            if !freshIDs.isEmpty {
                notifiedLinkIDs.formUnion(freshIDs)
                notificationManager.notifyLinksGrabbed(count: freshIDs.count)
            }

            guard GrabberSettings.autoAddAndStartOnGrab else { return }
            for package in newSnapshot.packages {
                let ids = Set(package.linkIDs)
                guard ids.isDisjoint(with: autoAddedLinkIDs) else { continue }
                let links = newSnapshot.links.filter { ids.contains($0.id) }
                let media = newSnapshot.mediaRows.filter { ids.contains($0.id) }
                // Only auto-add a package whose every row is ready: HTTP links
                // online, media rows resolved. A half-ready package waits.
                guard !links.isEmpty || !media.isEmpty,
                    links.allSatisfy({ $0.verdict == .online }),
                    media.allSatisfy({ $0.state == .resolved })
                else { continue }
                autoAddedLinkIDs.formUnion(ids)
                let name = package.name
                let note = package.note
                let (items, _) = grabberController.downloadItems(inPackage: package.id)
                guard !items.isEmpty else { continue }
                Task {
                    await controller.addItems(
                        name: name, note: note, items: items, startImmediately: true)
                }
            }
        }
        .commands {
            CommandGroup(replacing: .appSettings) {
                Button("Settings…") {
                    openWindow(id: "settings")
                }
                .keyboardShortcut(",", modifiers: .command)
            }
            CommandGroup(before: .appSettings) {
                Button("Install Google Chrome Extensions...") {
                    openWindow(id: "chrome-extension-setup")
                }
            }
        }

        Window("Settings", id: "settings") {
            SettingsView()
                .environment(controller)
                .environment(managedBinaries)
                .environment(themeStore)
                .environment(activationPolicyController)
                .environment(launchAtLoginController)
                .environment(clipboardWatcher)
                .environment(notificationManager)
        }
        .windowResizability(.contentSize)

        Window(
            "Chrome Extension Setup",
            id: "chrome-extension-setup"
        ) {
            ChromeExtensionSetupView()
        }
        .windowResizability(.contentSize)
        .environment(themeStore)

        MenuBarExtra(isInserted: menuBarInsertedBinding) {
            MenuBarPopoverView(selection: $sidebarSelection)
                .environment(controller)
                .environment(grabberController)
                .environment(themeStore)
        } label: {
            Image(nsImage: menuBarIconController.image)
        }
        .menuBarExtraStyle(.window)
        // `controller.snapshot` changes every heartbeat tick regardless —
        // this closure runs at that same rate, but `update(fraction:
        // drawCircle:)` only re-rasterizes the icon when the ring would
        // actually look different, instead of every tick.
        .onChange(of: controller.snapshot) { _, _ in
            menuBarIconController.update(fraction: overallFraction, drawCircle: downloadsRunning)
        }
    }

    private func handleOpenUrl(_ url: URL) async {
        guard url.scheme == "com-shayanoh-sdm" else {
            return
        }

        guard url.host == "download" else {
            return
        }

        guard
            let components = URLComponents(
                url: url,
                resolvingAgainstBaseURL: false
            ),
            let downloadURLString = components.queryItems?
                .first(where: { $0.name == "url" })?
                .value,
            let downloadURL = URL(string: downloadURLString)
        else {
            return
        }

        print("Received download:", downloadURL)
        await grabberController.ingest(urls: [downloadURL])
    }
    /// `MenuBarExtra(isInserted:)` needs a `Binding`, but there is nothing to
    /// persist here beyond what `activationPolicyController` already tracks —
    /// this just projects `showsMenuBarIcon` through a no-op setter, since
    /// the only way this value changes is `activationPolicyController.mode`
    /// itself changing, which SwiftUI already observes.
    private var menuBarInsertedBinding: Binding<Bool> {
        Binding(get: { activationPolicyController.showsMenuBarIcon }, set: { _ in })
    }

    private var overallFraction: Double {
        let running = controller.snapshot.packages.flatMap(\.items).filter {
            ($0.state == .running || $0.state == .queued || $0.state == .completed)
                && ($0.totalBytes ?? 0) > 0
        }
        guard !running.isEmpty else { return 0 }
        return running.reduce(0.0) { $0 + $1.fractionCompleted } / Double(running.count)
    }

    private var downloadsRunning: Bool {
        return controller.snapshot.packages.flatMap(\.items).filter { $0.state == .running }.count
            > 0
    }
}
