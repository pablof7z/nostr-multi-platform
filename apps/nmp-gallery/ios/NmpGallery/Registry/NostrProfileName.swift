import SwiftUI

/// Inline display-name text for a Nostr profile.
///
/// Two construction modes:
///   • `NostrProfileName(profile:)` — caller already holds a `ProfileWire`
///     (static, no claiming). Renders `profile.display`.
///   • `NostrProfileName(pubkey:)` — *self-claiming*. The component owns the
///     responsibility of claiming the kind:0 it needs: on appear it claims the
///     profile from the `NostrProfileHost`, reads the resolved projection
///     reactively, and releases on disappear. This mirrors `NostrAvatar`'s
///     claim/release lifecycle exactly.
///
/// Display always comes from a Rust-formatted source — `displayName` when the
/// kind:0 has resolved, else the Rust-truncated `npubShort` (aim.md §6.9).
/// Never a Swift-side abbreviation, never raw hex. Until the host has *any*
/// profile (resolved, author-view, or mention) for the pubkey, the component
/// renders nothing rather than synthesize a Swift-formatted fallback.
///
/// Depends on `swiftui/user-avatar` for `ProfileWire` and `NostrProfileHost`.
public struct NostrProfileName: View {
    @Environment(\.nostrProfileHost) private var profileHost

    /// Static profile supplied directly by the caller (no claiming). `nil`
    /// when constructed in the self-claiming `pubkey:` mode.
    private let staticProfile: ProfileWire?
    /// Pubkey to self-claim (also the static profile's pubkey in static mode).
    private let pubkey: String
    private let consumerID: String?
    public var font: Font
    public var color: Color

    @State private var generatedConsumerID: String
    @State private var claimedPubkey: String?

    /// Static variant: render an already-resolved `ProfileWire`. No claiming.
    public init(
        profile: ProfileWire,
        font: Font = .headline,
        color: Color = .primary
    ) {
        self.staticProfile = profile
        self.pubkey = profile.pubkey
        self.consumerID = nil
        self.font = font
        self.color = color
        self._generatedConsumerID = State(
            initialValue: "nostr-profile-name.static.\(UUID().uuidString)"
        )
        self._claimedPubkey = State(initialValue: nil)
    }

    /// Self-claiming variant: claim the kind:0 for `pubkey` from the host,
    /// read the resolved profile reactively, release on disappear.
    public init(
        pubkey: String,
        font: Font = .body,
        color: Color = .primary,
        consumerID: String? = nil
    ) {
        self.staticProfile = nil
        self.pubkey = pubkey
        self.consumerID = consumerID
        self.font = font
        self.color = color
        self._generatedConsumerID = State(
            initialValue: consumerID ?? "nostr-profile-name.\(UUID().uuidString)"
        )
        self._claimedPubkey = State(initialValue: nil)
    }

    public var body: some View {
        // Static mode: render the supplied profile directly, no claiming.
        if let staticProfile {
            return AnyView(label(for: staticProfile))
        }

        // Self-claiming mode: resolve reactively from the host, claim/release
        // exactly like `NostrAvatar`.
        let resolved = profileHost?.profile(forPubkey: pubkey)
        return AnyView(
            Group {
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
        )
    }

    private func label(for profile: ProfileWire) -> some View {
        Text(profile.display)
            .font(font)
            .foregroundStyle(color)
            .lineLimit(1)
            .accessibilityLabel("Display name: \(profile.display)")
    }
}
