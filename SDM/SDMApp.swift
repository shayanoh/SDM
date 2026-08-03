//
//  SDMApp.swift
//  SDM
//
//  Created by Shayan Ostadhassan on 8/3/26.
//

import AppKit
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

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(controller)
                .task {
                    appDelegate.controller = controller
                    await controller.startHeartbeat()
                }
        }
    }
}
