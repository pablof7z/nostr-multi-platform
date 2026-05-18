import SwiftUI

/// Manual ISBN entry sheet. Used when the scanner can't read a damaged
/// barcode (old paperbacks), when the camera is unavailable, and as the
/// primary entry point for VoiceOver users.
///
/// Validates on every keystroke so the "Find" button enables the instant a
/// valid Bookland ISBN is typed. `onResult(nil)` means the user cancelled
/// without committing.
struct ManualISBNEntryView: View {
    var onResult: (String?) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var raw: String = ""
    @FocusState private var focused: Bool

    private var normalizedISBN: String? {
        ISBNValidator.validate(raw)
    }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 20) {
                Text("Type the 10- or 13-digit ISBN on the back cover.")
                    .font(.footnote)
                    .foregroundStyle(Color.highlighterInkMuted)

                TextField("978-…", text: $raw)
                    .textContentType(.oneTimeCode)
                    .keyboardType(.asciiCapableNumberPad)
                    .font(.title3.monospacedDigit())
                    .padding(14)
                    .background(Color.highlighterPaper, in: RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).stroke(Color.highlighterRule, lineWidth: 1))
                    .focused($focused)
                    .onAppear { focused = true }

                Spacer(minLength: 0)
            }
            .padding(20)
            .background(Color.highlighterPaper.ignoresSafeArea())
            .navigationTitle("Enter ISBN")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        onResult(nil)
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Find") {
                        guard let isbn = normalizedISBN else { return }
                        onResult(isbn)
                        dismiss()
                    }
                    .fontWeight(.semibold)
                    .disabled(normalizedISBN == nil)
                }
            }
        }
    }
}
