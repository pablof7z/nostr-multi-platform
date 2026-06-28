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
public struct NostrAvatar: View, Equatable {
    @Environment(\.nostrProfileHost) private var profileHost

    public let pubkey: String
    public let pictureUrl: URL?
    public let size: CGFloat
    public let consumerID: String?
    @State private var generatedConsumerID: String
    @State private var claimedPubkey: String?

    /// Equatable conformance comparing only the rendered-value inputs.
    ///
    /// `@State` vars (`generatedConsumerID`, `claimedPubkey`) are internal
    /// identity managed by SwiftUI across body re-evaluations and must NOT
    /// participate in equality — including them would cause `.equatable()` to
    /// wrongly suppress re-renders when those internal vars change.
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
        // #2224: the deterministic 5×5 symmetric grid is the single shared
        // fallback across every platform. `NostrIdenticon` (defined once in
        // `ContentTreeWire.swift`) renders the same pattern as Chirp iOS and
        // the Android Compose avatar for a given pubkey.
        NostrIdenticon.identiconView(forPubkey: pubkey, size: size)
            .clipShape(Circle())
    }
}

// MARK: - Identicon
//
// NOTE (gallery local edit): the `NostrIdenticon` enum that originally lived
// here in the upstream `swiftui/user-avatar` registry component has been
// removed. The single definition lives in `ContentTreeWire.swift`
// (`swiftui/content-core`) and is kept byte-identical to the registry source
// by the cross-platform identicon drift gate (#2224). Keeping both
// definitions in the same Swift module is a redeclaration error.
