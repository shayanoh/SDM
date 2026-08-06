import SwiftUI

enum SDMSurfaceKind {
    case sidebar, toolbar
}

private struct SDMSurfaceModifier: ViewModifier {
    let kind: SDMSurfaceKind

    func body(content: Content) -> some View {
        if #available(macOS 26, *) {
            content.glassEffect(.regular, in: .rect)
        } else {
            content.background(.regularMaterial)
        }
    }
}

extension View {
    /// Spec §9.9: Liquid Glass isolated to this one file. Every other view
    /// calls `.sdmSurface(_:)` rather than writing its own `if #available`;
    /// there is exactly one place to change when the deployment baseline
    /// moves past macOS 26.
    func sdmSurface(_ kind: SDMSurfaceKind) -> some View {
        modifier(SDMSurfaceModifier(kind: kind))
    }
}
