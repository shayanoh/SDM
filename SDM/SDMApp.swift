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

    func applicationWillTerminate(_ notification: Notification) {
        controller?.shutdownBlocking()
    }
}

@main
struct SDMApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var controller = EngineController()
    @State private var grabberController = GrabberController()
    @State private var clipboardWatcher = ClipboardWatcher()
    /// Link ids already handed to the download engine by auto-add-and-start,
    /// so a later snapshot change (e.g. an unrelated link finishing its
    /// probe) does not re-add the same package a second time — `addPackage`
    /// has no idempotency of its own.
    @State private var autoAddedLinkIDs: Set<UUID> = []
    @State private var sidebarSelection: ContentView.SidebarItem? = .downloads
    @State private var linkNotifications = NotificationManager()
    @State private var notifiedLinkIDs: Set<UUID> = []

    var body: some Scene {
        WindowGroup(id: "main") {
            ContentView(selection: $sidebarSelection)
                .environment(controller)
                .environment(grabberController)
                .task {
                    appDelegate.controller = controller
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
        }

        MenuBarExtra {
            MenuBarPopoverView(selection: $sidebarSelection)
                .environment(controller)
                .environment(grabberController)
        } label: {
            Image(nsImage: statusItemImage)
        }
        .menuBarExtraStyle(.window)
    }

    private var overallFraction: Double {
        let running = controller.snapshot.packages.flatMap(\.items).filter { $0.state == .running }
        guard !running.isEmpty else { return 0 }
        return running.reduce(0.0) { $0 + $1.fractionCompleted } / Double(running.count)
    }

    /// `MenuBarExtra`'s custom label view ignores `.frame`/sizing modifiers
    /// on live SwiftUI content — the status item falls back to the image's
    /// native pixel size, rendering oversized and cropped. Rasterizing to a
    /// fixed-size `NSImage` via `ImageRenderer` sidesteps that: the label
    /// only ever sees a plain bitmap at the exact size we ask for.
    private var statusItemImage: NSImage {
        let renderer = ImageRenderer(content: MenuBarRingIcon(fraction: overallFraction))
        renderer.scale = 2
        return renderer.nsImage ?? NSImage()
    }
}
