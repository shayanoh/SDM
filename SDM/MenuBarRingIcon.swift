import SwiftUI

/// Spec §9.7: "The menu bar icon shows a determinate ring for overall
/// progress." Built as plain SwiftUI rather than a rasterized `NSImage`,
/// which `.menuBarExtraStyle(.window)`'s custom label view supports directly.
struct MenuBarRingIcon: View {
    let fraction: Double

    var body: some View {
        ZStack {
            Circle().stroke(Color.secondary.opacity(0.4), lineWidth: 2)
            Circle()
                .trim(from: 0, to: max(0.02, min(fraction, 1)))
                .stroke(Color.primary, style: StrokeStyle(lineWidth: 2, lineCap: .round))
                .rotationEffect(.degrees(-90))
            Image("MenuBarLogo")
                .resizable()
                .scaledToFit()
                .frame(width: 9, height: 9)
        }
        .frame(width: 16, height: 16)
    }
}
