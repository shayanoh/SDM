import SwiftUI

#Preview {
    VStack(spacing:25) {
        MenuBarRingIcon(fraction: 0.25, drawCircle: true)
        MenuBarRingIcon(fraction: 0, drawCircle: false)
    }
    .padding(16)
}
/// Spec §9.7: "The menu bar icon shows a determinate ring for overall
/// progress." Built as plain SwiftUI rather than a rasterized `NSImage`,
/// which `.menuBarExtraStyle(.window)`'s custom label view supports directly.
struct MenuBarRingIcon: View {
    let fraction: Double
    let drawCircle: Bool

    var body: some View {
        ZStack {
            if (drawCircle) {
                Circle().stroke(Color.secondary.opacity(0.4), lineWidth: 1)
                    .padding(2)
                Circle()
                    .trim(from: 0, to: max(0.02, min(fraction, 1)))
                    .stroke(Color.primary, style: StrokeStyle(lineWidth: 1, lineCap: .round))
                    .rotationEffect(.degrees(-90))
                    .padding(2)
            }
            Image("MenuBarLogo")
                .resizable()
                .scaledToFit()
                .frame(width: 14, height: 14)
        }
        .frame(width: 16, height: 16)
    }
}
