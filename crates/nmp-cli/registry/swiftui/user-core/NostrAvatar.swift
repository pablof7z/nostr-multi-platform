import SwiftUI

/// Circular avatar for a Nostr profile. Shows the remote picture when
/// available; falls back to a deterministic identicon derived from `pubkey`.
///
/// Replace `AsyncImage` with your own image cache (Kingfisher, Nuke, etc.)
/// if you already have one — the identicon fallback is self-contained.
///
/// Depends on `swiftui/user-core` for `ProfileWire`.
public struct NostrAvatar: View {
    public let pubkey: String
    public let pictureUrl: URL?
    public let size: CGFloat

    public init(pubkey: String, pictureUrl: URL? = nil, size: CGFloat = 40) {
        self.pubkey = pubkey
        self.pictureUrl = pictureUrl
        self.size = size
    }

    public init(profile: ProfileWire, size: CGFloat = 40) {
        self.pubkey = profile.pubkey
        self.pictureUrl = profile.avatarURL
        self.size = size
    }

    public var body: some View {
        Group {
            if let url = pictureUrl {
                AsyncImage(url: url) { phase in
                    switch phase {
                    case .success(let image):
                        image.resizable().scaledToFill()
                    default:
                        identicon
                    }
                }
            } else {
                identicon
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .accessibilityHidden(true)
    }

    private var identicon: some View {
        ZStack {
            Circle().fill(NostrIdenticon.color(forPubkey: pubkey))
            Text(NostrIdenticon.initials(forPubkey: pubkey))
                .font(.system(size: size * 0.35, weight: .semibold))
                .foregroundStyle(.white)
        }
    }
}

// MARK: - Identicon

/// Deterministic color + initials from a raw pubkey hex string.
/// Edit `palette` to match your app's brand palette.
public enum NostrIdenticon {
    private static let palette: [Color] = [
        Color(red: 0.36, green: 0.20, blue: 0.81),
        Color(red: 0.10, green: 0.53, blue: 0.82),
        Color(red: 0.13, green: 0.55, blue: 0.42),
        Color(red: 0.82, green: 0.33, blue: 0.18),
        Color(red: 0.76, green: 0.15, blue: 0.45),
        Color(red: 0.20, green: 0.20, blue: 0.20),
    ]

    public static func color(forPubkey pubkey: String) -> Color {
        let sum = pubkey.utf8.prefix(4).reduce(0) { $0 + Int($1) }
        return palette[sum % palette.count]
    }

    public static func initials(forPubkey pubkey: String) -> String {
        guard pubkey.count >= 2 else { return "?" }
        return String(pubkey.prefix(2)).uppercased()
    }
}
