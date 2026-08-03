//
//  SDMApp.swift
//  SDM
//
//  Created by Shayan Ostadhassan on 8/3/26.
//

import SwiftUI

@main
struct SDMApp: App {
    @State private var controller = EngineController()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(controller)
                .task { await controller.startHeartbeat() }
        }
    }
}
