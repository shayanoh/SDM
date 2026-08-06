import AppKit
import SwiftUI

/// Bridges to this view's own `NSWindow`. SwiftUI has no view-level API for
/// window chrome (titlebar background, `titlebarAppearsTransparent`) — this
/// is the standard trick: an invisible `NSView` whose `.window` property,
/// read once it's actually inserted into the hierarchy, is exactly the
/// window hosting this view. `DispatchQueue.main.async` because `.window`
/// is `nil` at the moment `makeNSView`/`updateNSView` run — the view isn't
/// attached to a window yet at that point in the layout pass.
struct WindowAccessor: NSViewRepresentable {
    let onResolve: (NSWindow?) -> Void

    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async { onResolve(view.window) }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        DispatchQueue.main.async { onResolve(nsView.window) }
    }
}
