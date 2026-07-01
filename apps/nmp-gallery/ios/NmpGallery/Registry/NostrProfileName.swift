import SwiftUI

/// Inline display-name text for a Nostr profile.
///
/// Two construction modes:
///   • `NostrProfileName(profile:)` — caller already holds a `ProfileWire`.
///     Resolves the profile's pubkey on mount so the name stays fresh (mirrors
///     `NostrAvatar`), reads `profile.display`.
///   • `NostrProfileName(pubkey:)` — self-resolving. The component owns the
///     responsibility of resolving the kind:0 it needs: on mount it resolves
///     the profile through the `NostrProfileHost`, reads the `refs.profile` projection
///     reactively, and releases on disappear. This mirrors `NostrAvatar`'s
///     resolve/release lifecycle exactly, so any standalone name render anywhere
///     triggers resolution (resolve-only invariant — every author-displaying
///     surface must self-resolve).
///
/// Display always comes from a Rust-formatted source — `displayName` when set,
/// else `npubShort` (always Rust-formatted — aim.md §6.9). Until the host has
/// any profile for the pubkey, the self-resolving variant renders nothing rather
/// than synthesize a Swift-side abbreviation.
///
/// Depends on `swiftui/user-avatar` for `ProfileWire` and `NostrProfileHost`.
public struct NostrProfileName: View {
    @Environment(\.nostrProfileHost) private var profileHost

    /// Static profile supplied directly by the caller. `nil` when constructed
    /// in the self-resolving `pubkey:` mode.
    private let staticProfile: ProfileWire?
    /// Pubkey to resolve (also the static profile's pubkey in static mode).
    private let pubkey: String
    public var font: Font
    public var color: Color

    @State private var generatedConsumerID: String
    @State private var activePubkey: String?

    /// Static variant: render the supplied `ProfileWire`. Resolves its pubkey on
    /// mount so the name stays fresh.
    public init(
        profile: ProfileWire,
        font: Font = .headline,
        color: Color = .primary
    ) {
        self.staticProfile = profile
        self.pubkey = profile.pubkey
        self.font = font
        self.color = color
        self._generatedConsumerID = State(
            initialValue: "nostr-profile-name.\(UUID().uuidString)")
        self._activePubkey = State(initialValue: nil)
    }

    /// Self-resolving variant: resolve the kind:0 for `pubkey` through the host,
    /// read the resolved profile reactively, release on disappear.
    public init(
        pubkey: String,
        font: Font = .body,
        color: Color = .primary,
        consumerID: String? = nil
    ) {
        self.staticProfile = nil
        self.pubkey = pubkey
        self.font = font
        self.color = color
        self._generatedConsumerID = State(
            initialValue: consumerID ?? "nostr-profile-name.\(UUID().uuidString)")
        self._activePubkey = State(initialValue: nil)
    }

    public var body: some View {
        let resolved = staticProfile ?? profileHost?.profile(forPubkey: pubkey)
        return Group {
            if let resolved {
                label(for: resolved)
            } else {
                // No kind:0 yet, and no Rust-formatted npubShort available.
                // Render nothing rather than a Swift-side abbreviation.
                EmptyView()
            }
        }
        .task(id: pubkey) {
            await MainActor.run {
                if let activePubkey, activePubkey != pubkey {
                    profileHost?.releaseProfileRef(
                        pubkey: activePubkey, consumerID: generatedConsumerID)
                }
                activePubkey = pubkey
                profileHost?.resolveProfileRef(pubkey: pubkey, consumerID: generatedConsumerID)
            }
        }
        .onDisappear {
            if let activePubkey {
                profileHost?.releaseProfileRef(
                    pubkey: activePubkey, consumerID: generatedConsumerID)
                self.activePubkey = nil
            }
        }
    }

    private func label(for profile: ProfileWire) -> some View {
        Text(profile.display)
            .font(font)
            .foregroundStyle(color)
            .lineLimit(1)
            .accessibilityLabel("Display name: \(profile.display)")
    }
}
