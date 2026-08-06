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
/// SwiftUI gives a `WindowGroup` no termination hook. `EngineController`'s
/// heartbeat shuts the engine down when its `.task` is cancelled, but on ⌘Q
/// the cancelled continuation is not guaranteed to be scheduled before the
/// process dies, and on last-window-close it races process exit. Since
/// `flush()` is the only thing that writes `state.json`, the ordinary quit
/// path wrote nothing at all — `.incomplete` files and their `.sdmpart`
/// sidecars survived with no record of the item owning them, and the next
/// launch restored an empty list.
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
    @State private var controller = EngineController()
    @State private var grabberController = GrabberController()
    @State private var themeStore = ThemeStore()
    @State private var activationPolicyController = ActivationPolicyController()
    @State private var clipboardWatcher = ClipboardWatcher()
    /// Link ids already handed to the download engine by auto-add-and-start,
    /// so a later snapshot change (e.g. an unrelated link finishing its
    /// probe) does not re-add the same package a second time — `addPackage`
    /// has no idempotency of its own.
    @State private var autoAddedLinkIDs: Set<UUID> = []
    @State private var sidebarSelection: MainWindowView.SidebarItem? = .downloads
    @State private var linkNotifications = NotificationManager()
    @State private var notifiedLinkIDs: Set<UUID> = []

    var body: some Scene {
        WindowGroup(id: "main") {
            MainWindowView(selection: $sidebarSelection)
                .environment(controller)
                .environment(grabberController)
                .environment(themeStore)
                .task {
                    appDelegate.controller = controller
                    appDelegate.activationPolicyController = activationPolicyController
                    await controller.startHeartbeat()
                }
                .onAppear {
                    clipboardWatcher.onLinksDetected = { urls in
                        guard GrabberSettings.clipboardWatchingEnabled else { return }
                        Task { await grabberController.ingest(urls: urls) }
                    }
                    if GrabberSettings.clipboardWatchingEnabled { clipboardWatcher.start() }
                }
                .onDisappear { clipboardWatcher.stop() }
                .onChange(of: grabberController.snapshot) { _, newSnapshot in
                    let freshIDs = Set(newSnapshot.links.map(\.id)).subtracting(notifiedLinkIDs)
                    if !freshIDs.isEmpty {
                        notifiedLinkIDs.formUnion(freshIDs)
                        linkNotifications.notifyLinksGrabbed(count: freshIDs.count)
                    }

                    guard GrabberSettings.autoAddAndStartOnGrab else { return }
                    for package in newSnapshot.packages {
                        let ids = Set(package.linkIDs)
                        guard ids.isDisjoint(with: autoAddedLinkIDs) else { continue }
                        let links = newSnapshot.links.filter { ids.contains($0.id) }
                        guard !links.isEmpty, links.allSatisfy({ $0.verdict == .online }) else {
                            continue
                        }
                        autoAddedLinkIDs.formUnion(ids)
                        let name = package.name
                        let urls = links.map(\.originalURL)
                        Task {
                            await controller.addPackage(
                                name: name, urls: urls, startImmediately: true)
                        }
                    }
                }
        }

        Settings {
            SettingsView()
                .environment(controller)
                .environment(themeStore)
                .environment(activationPolicyController)
        }

        MenuBarExtra(isInserted: menuBarInsertedBinding) {
            MenuBarPopoverView(selection: $sidebarSelection)
                .environment(controller)
                .environment(grabberController)
                .environment(themeStore)
        } label: {
            Image(nsImage: statusItemImage)
        }
        .menuBarExtraStyle(.window)
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
        let running = controller.snapshot.packages.flatMap(\.items).filter { $0.state == .running || $0.state == .queued || $0.state == .completed}
        guard !running.isEmpty else { return 0 }
        return running.reduce(0.0) { $0 + $1.fractionCompleted } / Double(running.count)
    }
    
    private var downloadsRunning: Bool {
        return controller.snapshot.packages.flatMap(\.items).filter {$0.state == .running}.count > 0
    }

    /// `MenuBarExtra`'s custom label view ignores `.frame`/sizing modifiers
    /// on live SwiftUI content — the status item falls back to the image's
    /// native pixel size, rendering oversized and cropped. Rasterizing to a
    /// fixed-size `NSImage` via `ImageRenderer` sidesteps that: the label
    /// only ever sees a plain bitmap at the exact size we ask for.
    private var statusItemImage: NSImage {
        let isDark = NSApp.effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        let theme = themeStore.resolved(for: isDark ? .dark : .light)
        let renderer = ImageRenderer(content: MenuBarRingIcon(fraction: overallFraction, drawCircle: downloadsRunning, theme: theme))
        renderer.scale = 2
        return renderer.nsImage ?? NSImage()
    }
}
