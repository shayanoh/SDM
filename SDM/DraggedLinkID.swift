import CoreTransferable
import Foundation
import UniformTypeIdentifiers

struct DraggedLinkID: Codable, Transferable {
    let linkID: UUID

    static var transferRepresentation: some TransferRepresentation {
        CodableRepresentation(contentType: .sdmDraggedLink)
    }
}

extension UTType {
    static var sdmDraggedLink: UTType { UTType(exportedAs: "com.shayanoh.sdm.dragged-link") }
}
