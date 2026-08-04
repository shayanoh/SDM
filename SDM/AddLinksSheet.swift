import SwiftUI

/// Multiline paste field feeding the identical extraction pipeline as
/// clipboard watching. Spec §7.5.
struct AddLinksSheet: View {
    @Environment(GrabberController.self) private var controller
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Add links")
                .font(.headline)
            TextEditor(text: $text)
                .font(.body.monospaced())
                .frame(minWidth: 420, minHeight: 200)
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(.separator))

            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Add") {
                    let pasted = text
                    dismiss()
                    Task { await controller.ingest(text: pasted) }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .padding()
    }
}
