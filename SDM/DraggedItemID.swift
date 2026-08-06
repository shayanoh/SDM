import CoreTransferable
import Foundation
import UniformTypeIdentifiers

/// Carries a dragged download item's id between rows and package headers.
/// Spec §9.3: "a custom `Transferable` carrying item IDs."
struct DraggedItemID: Codable, Transferable {
    let itemID: UUID

    static var transferRepresentation: some TransferRepresentation {
        CodableRepresentation(contentType: .sdmDraggedItem)
    }
}

extension UTType {
    static var sdmDraggedItem: UTType { UTType(exportedAs: "com.shayanoh.sdm.dragged-item") }
}
