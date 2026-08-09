import SDMCore
import SwiftUI

#Preview {
    VStack(spacing: 25) {
        MenuBarRingIcon(fraction: 0.5, drawCircle: true)
        MenuBarRingIcon(fraction: 0, drawCircle: false)
    }
    .padding(16)
}

/// Spec §9.7: "The menu bar icon shows a determinate ring for overall
/// progress." Built as plain SwiftUI rather than a rasterized `NSImage`,
/// which `.menuBarExtraStyle(.window)`'s custom label view supports
/// directly. Takes `theme` explicitly — it is rendered via `ImageRenderer`
/// outside the live view tree (see `SDMApp.statusItemImage`), where
/// `@Environment` is unavailable.
struct MenuBarRingIcon: View {
    let fraction: Double
    let drawCircle: Bool

    var body: some View {
        ZStack(alignment: .bottom) {
            Image("MenuBarLogo")
                .resizable()
                .scaledToFit()
                .opacity(drawCircle ? 0.25 : 1)
            if drawCircle {
                GeometryReader { geometry in
                    Image("MenuBarLogo")
                        .resizable()
                        .scaledToFit()
                        .frame(
                            width: geometry.size.width,
                            height: geometry.size.height
                        )
                        .clipShape(
                            Rectangle()
                                .path(
                                    in: CGRect(
                                        x: 0,
                                        y: 0,
                                        width: geometry.size.width * fraction,
                                        height: geometry.size.height
                                    ))
                        )
                }
            }
        }
        .frame(width: 14, height: 14)
    }
}
