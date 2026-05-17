import SwiftUI

struct RemoteAvatar: View {
    let url: String?
    let initials: String
    let color: String
    let source: String
    let size: CGFloat

    var body: some View {
        ZStack {
            Circle()
                .fill(Color(hex: color))
            if let url, let parsed = URL(string: url) {
                AsyncImage(url: parsed) { phase in
                    switch phase {
                    case let .success(image):
                        image
                            .resizable()
                            .scaledToFill()
                    default:
                        Text(initials)
                            .font(.caption.weight(.bold))
                            .foregroundStyle(.white)
                            .minimumScaleFactor(0.7)
                    }
                }
            } else {
                Text(initials)
                    .font(.caption.weight(.bold))
                    .foregroundStyle(.white)
                    .minimumScaleFactor(0.7)
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .overlay {
            if source == "placeholder" {
                Circle()
                    .strokeBorder(.secondary.opacity(0.45), style: StrokeStyle(lineWidth: 1, dash: [3, 2]))
            }
        }
        .accessibilityLabel(source)
    }
}

struct DiagnosticRow: View {
    let title: String
    let value: String

    init(_ title: String, _ value: String) {
        self.title = title
        self.value = value
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline) {
            Text(title)
                .foregroundStyle(.secondary)
            Spacer(minLength: 16)
            Text(value)
                .multilineTextAlignment(.trailing)
                .textSelection(.enabled)
        }
        .font(.caption)
    }
}

struct MetricCell: View {
    let title: String
    let value: String
    let valueID: String

    init(_ title: String, _ value: String, valueID: String) {
        self.title = title
        self.value = value
        self.valueID = valueID
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title.uppercased())
                .font(.caption2)
                .foregroundStyle(.secondary)
            Text(value)
                .font(.system(.caption, design: .monospaced).weight(.semibold))
                .lineLimit(1)
                .minimumScaleFactor(0.75)
                .accessibilityIdentifier(valueID)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

extension ProfileCard {
    static func placeholder(pubkey: String) -> ProfileCard {
        ProfileCard(
            pubkey: pubkey,
            npub: pubkey,
            display: shortPubkeyDisplay(pubkey),
            pictureUrl: nil,
            nip05: "",
            about: "Waiting for selected author kind:0",
            avatarInitials: "..",
            avatarColor: generatedAvatarColor(pubkey),
            source: "placeholder"
        )
    }
}

func shortPubkeyDisplay(_ value: String) -> String {
    guard value.count > 16 else {
        return value
    }
    return "\(value.prefix(8))...\(value.suffix(8))"
}

func generatedAvatarColor(_ value: String) -> String {
    let palette = [
        "5E5CE6", "0A84FF", "30D158", "FF9F0A",
        "FF453A", "BF5AF2", "64D2FF", "FFD60A",
    ]
    let total = value.unicodeScalars.reduce(0) { partial, scalar in
        partial + Int(scalar.value)
    }
    return palette[total % palette.count]
}

extension Color {
    init(hex: String) {
        let trimmed = hex.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
        let value = UInt64(trimmed, radix: 16) ?? 0x8E8E93
        let red = Double((value >> 16) & 0xFF) / 255.0
        let green = Double((value >> 8) & 0xFF) / 255.0
        let blue = Double(value & 0xFF) / 255.0
        self.init(red: red, green: green, blue: blue)
    }
}

extension View {
    @ViewBuilder
    func nmpGlassPanel(cornerRadius: CGFloat) -> some View {
        if #available(iOS 26.0, *) {
            self.glassEffect(.regular.interactive(), in: .rect(cornerRadius: cornerRadius))
        } else {
            background(
                .regularMaterial,
                in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
            )
        }
    }
}

#Preview {
    ContentView()
        .environmentObject(KernelModel())
}
