import AppKit
import SwiftUI

/// Disables macOS's native, system-accent-colored row-selection highlight
/// on the `List` this is attached to (via `.background()`), so a
/// theme-driven `.listRowBackground` tint — already applied per row — is
/// the only selection indicator drawn.
///
/// Verified against current SwiftUI docs before writing this: neither
/// `.tint(_:)` nor `.listItemTint(_:)` reaches a `List`'s native selection
/// background — `.listItemTint` is documented as affecting only sidebar
/// `Label` icons and watchOS background platters, nothing else. There is no
/// public SwiftUI modifier for this, so this walks up from an injected
/// `NSView` to the `NSTableView` `List` bridges to on macOS and sets its
/// `selectionHighlightStyle` directly — `NSTableView.SelectionHighlightStyle`
/// is long-standing, public, documented AppKit API; only *finding* the
/// table view (there being no direct handle to it) needs a manual walk.
/// This only changes how selection is *drawn* — it does not touch selection
/// state, so clicks, `⌘A`, and keyboard navigation are unaffected.
private struct NativeSelectionHighlightDisabler: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        DispatchQueue.main.async { [weak view] in disable(startingFrom: view) }
        return view
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        DispatchQueue.main.async { [weak nsView] in disable(startingFrom: nsView) }
    }

    private func disable(startingFrom view: NSView?) {
        var current = view?.superview
        while let node = current {
            if let scrollView = node as? NSScrollView,
                let tableView = scrollView.documentView as? NSTableView
            {
                tableView.selectionHighlightStyle = .none
                return
            }
            if let tableView = node as? NSTableView {
                tableView.selectionHighlightStyle = .none
                return
            }
            current = node.superview
        }
    }
}

extension View {
    /// Apply to a `List(selection:)` whose selection should be indicated
    /// purely via `.listRowBackground` rather than the system's own
    /// accent-colored highlight.
    func hidesNativeSelectionHighlight() -> some View {
        background(NativeSelectionHighlightDisabler())
    }
}
