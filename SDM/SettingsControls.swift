import SDMCore
import SwiftUI

/// Card-style group for the settings window: a heading plus themed bordered
/// content, replacing `Form`/`Section`'s system chrome so groups read as
/// visually separate cards that match the rest of the app's theme-role
/// colors instead of system defaults.
struct SettingsSection<Content: View>: View {
    let title: String
    let theme: Theme
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.headline)
                .foregroundStyle(theme.textSecondaryColor)
            VStack(alignment: .leading, spacing: 12) {
                content()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
            .background(
                RoundedRectangle(cornerRadius: 8)
                    .fill(theme.surfaceSecondaryColor.opacity(0.5))
            )
            .overlay(
                RoundedRectangle(cornerRadius: 8)
                    .stroke(theme.borderColor, lineWidth: 1)
            )
        }
    }
}

/// Editable, range-clamped numeric field paired with a stepper. Replaces
/// bare `Stepper` controls, which only show their value inside the label
/// text and give no way to type a value directly. A `GridRow` so it lines
/// up with sibling fields inside a parent `Grid`.
struct SteppedNumberField: View {
    let label: String
    @Binding var value: Int
    let range: ClosedRange<Int>

    @State private var text: String = ""

    var body: some View {
        GridRow {
            Text(label)
                .gridColumnAlignment(.leading)
            HStack(spacing: 4) {
                TextField("", text: $text)
                    .textFieldStyle(.roundedBorder)
                    .multilineTextAlignment(.trailing)
                    .frame(width: 56)
                    .onSubmit(commit)
                    .onChange(of: text) { _, newValue in
                        let digitsOnly = newValue.filter(\.isNumber)
                        if digitsOnly != newValue { text = digitsOnly }
                        commit()
                    }
                Stepper("", value: $value, in: range)
                    .labelsHidden()
                Text("\(range.lowerBound)..\(range.upperBound)")
                    .font(.callout)
                    .foregroundStyle(.secondary)

            }
            .gridColumnAlignment(.leading)
        }
        .onAppear { text = String(value) }
        .onChange(of: value) { _, newValue in
            text = String(newValue)
        }
    }

    /// Invalid or out-of-range text reverts to the last valid value rather
    /// than being accepted.
    private func commit() {
        guard !text.isEmpty else {
            value = range.lowerBound
            return
        }
        guard let parsed = Int(text), range.contains(parsed) else {
            text = String(value)
            return
        }
        value = parsed
        text = String(parsed)
    }
}
