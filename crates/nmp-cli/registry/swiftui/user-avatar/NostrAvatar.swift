import SwiftUI
import Foundation

/// Circular avatar for a Nostr pubkey. Shows the profile picture when the
/// host projection has it; falls back to a deterministic identicon derived
/// from `pubkey`.
///
/// Replace `AsyncImage` with your own image cache (Kingfisher, Nuke, etc.)
/// if you already have one — the identicon fallback is self-contained.
///
/// Depends on `swiftui/user-avatar` for `ProfileWire` / `NostrProfileHost`
/// and `swiftui/content-core` for the shared `NostrIdenticon`.
public struct NostrAvatar: View, Equatable {
    @Environment(\.nostrProfileHost) private var profileHost

    public let pubkey: String
    public let pictureUrl: URL?
    public let size: CGFloat
    public let consumerID: String?
    @State private var generatedConsumerID: String
    @State private var claimedPubkey: String?

    /// Compare only the render-relevant inputs. SwiftUI-owned `@State`
    /// storage changes independently and must not participate in equality.
    public nonisolated static func == (lhs: NostrAvatar, rhs: NostrAvatar) -> Bool {
        lhs.pubkey == rhs.pubkey
            && lhs.pictureUrl == rhs.pictureUrl
            && lhs.size == rhs.size
            && lhs.consumerID == rhs.consumerID
    }

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
                    profileHost?.releaseProfileRef(
                        pubkey: claimedPubkey,
                        consumerID: generatedConsumerID
                    )
                }
                claimedPubkey = pubkey
                profileHost?.resolveProfileRef(pubkey: pubkey, consumerID: generatedConsumerID)
            }
        }
        .onDisappear {
            if let claimedPubkey {
                profileHost?.releaseProfileRef(pubkey: claimedPubkey, consumerID: generatedConsumerID)
                self.claimedPubkey = nil
            }
        }
    }

    private var identicon: some View {
        NostrIdenticon.identiconView(forPubkey: pubkey, size: size)
            .clipShape(Circle())
    }
}
