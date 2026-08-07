import CoreTransferable
import Foundation
import UniformTypeIdentifiers

/// Carries a dragged row's identity — an item or a package — through
/// `.draggable`/`.dropDestination`. Spec §9.3: "a custom `Transferable`
/// carrying item IDs," extended to packages the same way once packages
/// needed to drag too (see the doc comment below for why).
///
/// One enum with one content type rather than two separate `Transferable`
/// structs deliberately: a view that needs to accept both an item drop
/// (move into this package) and a package drop (reorder) used to carry two
/// stacked `.dropDestination(for:)` modifiers for two different types —
/// confirmed live that only one of the two ever actually receives a drop
/// (dragging worked, but nothing accepted the drop). A single `.draggable`/
/// `.dropDestination` pair per view, switching on the case inside the one
/// `action` closure, has no such ambiguity.
enum DraggedRowID: Codable, Transferable {
    case item(UUID)
    case package(UUID)

    static var transferRepresentation: some TransferRepresentation {
        CodableRepresentation(contentType: .sdmDraggedRow)
    }
}

/// Deliberately *not* `List`'s native `.onMove`, for both items and
/// packages. The two cannot coexist anywhere in the same `List`: on macOS,
/// once any row carries `.draggable`, an unclaimed item-row drag (no
/// `.onMove` at the item level) gets picked up by the *nearest enclosing*
/// `.onMove` handler instead of failing cleanly — which turned out to be the
/// package-level one, one level up, treating an item's row-index as a
/// package-reorder offset and crashing (`ids.move(fromOffsets:toOffset:)` on
/// a 2-package array with an item-sized offset). Confirmed live via a crash
/// log pointing straight at `PackagesListView`'s old package-level
/// `.onMove` closure. See `PackagesListView.DraggableItemRow` and
/// `PackageHeaderRow` for where `.draggable`/`.dropDestination` replace it.
extension UTType {
    static var sdmDraggedRow: UTType { UTType(exportedAs: "com.shayanoh.sdm.dragged-row") }
}
