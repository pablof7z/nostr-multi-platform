import SwiftUI

public struct NostrGroupComposer: View {
    @Binding private var text: String
    public var placeholder: String
    public var isEnabled: Bool
    public var onSend: (String) -> Void

    public init(
        text: Binding<String>,
        placeholder: String = "Message",
        isEnabled: Bool = true,
        onSend: @escaping (String) -> Void
    ) {
        self._text = text
        self.placeholder = placeholder
        self.isEnabled = isEnabled
        self.onSend = onSend
    }

    public var body: some View {
        HStack(spacing: 8) {
            TextField(placeholder, text: $text)
                .textFieldStyle(.plain)
                .padding(.horizontal, 10)
                .padding(.vertical, 8)
                .background(.secondary.opacity(0.10), in: RoundedRectangle(cornerRadius: 8))
                .disabled(!isEnabled)

            Button {
                let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !trimmed.isEmpty else { return }
                onSend(trimmed)
                text = ""
            } label: {
                Text("Send").font(.callout.weight(.semibold))
            }
            .buttonStyle(.borderedProminent)
            .disabled(!isEnabled || text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
        }
    }
}
