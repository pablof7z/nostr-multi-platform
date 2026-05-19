import SwiftUI

/// A text field that toggles between secure and plain entry with an eye button.
///
/// Use this wherever a user pastes an API key and needs the option to reveal it.
struct RevealableAPIKeyField: View {
    enum IconStyle {
        case outline
        case filled
    }

    let placeholder: String
    @Binding var text: String
    var iconStyle: IconStyle = .outline
    var tint: Color = .secondary
    var isDisabled: Bool = false

    @State private var isRevealed = false

    init(_ placeholder: String, text: Binding<String>, iconStyle: IconStyle = .outline, tint: Color = .secondary, isDisabled: Bool = false) {
        self.placeholder = placeholder
        self._text = text
        self.iconStyle = iconStyle
        self.tint = tint
        self.isDisabled = isDisabled
    }

    var body: some View {
        HStack {
            if isRevealed {
                TextField(placeholder, text: $text)
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
            } else {
                SecureField(placeholder, text: $text)
            }
            Button {
                isRevealed.toggle()
            } label: {
                Image(systemName: eyeIconName)
                    .foregroundStyle(tint)
            }
            .buttonStyle(.plain)
            .disabled(isDisabled)
            .accessibilityLabel(isRevealed ? "Hide API key" : "Show API key")
        }
    }

    private var eyeIconName: String {
        switch iconStyle {
        case .outline: isRevealed ? "eye.slash"      : "eye"
        case .filled:  isRevealed ? "eye.slash.fill" : "eye.fill"
        }
    }
}
