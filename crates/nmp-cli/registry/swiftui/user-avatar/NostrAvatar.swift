import SwiftUI
import Foundation

/// Circular avatar for a Nostr pubkey. Shows the profile picture when the
/// host projection has it; falls back to a deterministic identicon derived
/// from `pubkey`.
///
/// Replace `AsyncImage` with your own image cache (Kingfisher, Nuke, etc.)
/// if you already have one — the identicon fallback is self-contained.
///
/// Depends on `swiftui/user-avatar` for `ProfileWire` and `NostrProfileHost`.
public struct NostrAvatar: View {
    @Environment(\.nostrProfileHost) private var profileHost

    public let pubkey: String
    public let pictureUrl: URL?
    public let size: CGFloat
    public let consumerID: String?
    @State private var generatedConsumerID: String
    @State private var claimedPubkey: String?

    public init(
        pubkey: String,
        pictureUrl: URL? = nil,
        size: CGFloat = 40,
        consumerID: String? = nil
    ) {
        self.pubkey = pubkey
        self.pictureUrl = pictureUrl
        self.size = size
        self.consumerID = consumerID
        self._generatedConsumerID = State(
            initialValue: consumerID ?? "nostr-avatar.\(UUID().uuidString)"
        )
        self._claimedPubkey = State(initialValue: nil)
    }

    public init(profile: ProfileWire, size: CGFloat = 40) {
        self.pubkey = profile.pubkey
        self.pictureUrl = profile.avatarURL
        self.size = size
        self.consumerID = nil
        self._generatedConsumerID = State(
            initialValue: "nostr-avatar.static.\(UUID().uuidString)"
        )
        self._claimedPubkey = State(initialValue: nil)
    }

    public var body: some View {
        let url = pictureUrl ?? profileHost?.profile(forPubkey: pubkey)?.avatarURL

        Group {
            if let url {
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
        .task(id: pubkey) {
            await MainActor.run {
                if let claimedPubkey, claimedPubkey != pubkey {
                    profileHost?.releaseProfile(
                        pubkey: claimedPubkey,
                        consumerID: generatedConsumerID
                    )
                }
                claimedPubkey = pubkey
                profileHost?.claimProfile(pubkey: pubkey, consumerID: generatedConsumerID)
            }
        }
        .onDisappear {
            if let claimedPubkey {
                profileHost?.releaseProfile(pubkey: claimedPubkey, consumerID: generatedConsumerID)
                self.claimedPubkey = nil
            }
        }
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
