import SwiftUI

/// Inline display-name text for a Nostr profile.
///
/// Registry component `swiftui/user-name`, installed into Chirp.
///
/// Three construction modes:
///
///   * `init(profile:)` — renders `profile.display` (`displayName` when set,
///     else the Rust-truncated `npubShort`). The simple single-projection
///     case used by profile headers. The caller already holds the resolved
///     `ProfileWire`, so this leaf claims the profile interest anyway (mounting
///     the name keeps the kind:0 fresh, mirroring `NostrAvatar`).
///   * `init(pubkey:)` — *self-claiming*. The component owns the responsibility
///     of claiming the kind:0 it needs: on mount it claims the profile from the
///     `NostrProfileHost`, reads the resolved projection reactively, and
///     releases on disappear. This mirrors `NostrAvatar`'s claim/release
///     lifecycle exactly, so any standalone name render anywhere triggers
///     resolution (F-CR-00 claim-only invariant — every author-displaying
///     surface must self-claim). The Rust-formatted `npubShort` (aim.md §6.9)
///     is shown until the kind:0 resolves; never a Swift-side abbreviation.
///   * `init(displayName:)` — renders a label the *caller* already resolved and
///     for which the caller has ALREADY claimed the profile interest (e.g.
///     Chirp's timeline rows, where the adjacent `NostrAvatar` self-claims the
///     same author). No claim here would double-count the interest, so this
///     mode is render-only. Chirp's `NoteRowView.resolveAuthorLabel` resolves
///     the author label across several kernel projections (claimed / resolved /
///     timeline-baked / event-card — the PR #823 flicker fix); that
///     multi-source resolution is a data concern that belongs in the app, not
///     in a rendering leaf, so the resolved string is passed straight in.
///
/// Depends on `swiftui/user-avatar` for `ProfileWire` and `NostrProfileHost`.
struct NostrProfileName: View {
    @Environment(\.nostrProfileHost) private var profileHost

    private enum Source {
        /// Caller resolved the label out-of-band AND already claims the
        /// interest (no claim here). Carries the literal string.
        case preResolved(String)
        /// Caller holds a static `ProfileWire`; claim its pubkey on mount.
        case staticProfile(ProfileWire)
        /// Self-claiming by pubkey; resolve reactively from the host.
        case selfClaim(pubkey: String)
    }

    private let source: Source
    var font: Font
    var color: Color

    @State private var generatedConsumerID: String
    @State private var claimedPubkey: String?
    /// ADR-0063 Lane E (#1671): per-key observer so EXACTLY this name re-renders
    /// when its one pubkey's `refs.profile` row commits. Inert in the
    /// render-only `displayName:` mode (no claim pubkey).
    @StateObject private var rowObserver = KeyedRefRowObserver()

    /// Static variant: render an already-resolved `ProfileWire`. Claims the
    /// profile's pubkey on mount so the name stays fresh (mirrors `NostrAvatar`).
    init(
        profile: ProfileWire,
        font: Font = .headline,
        color: Color = .primary
    ) {
        self.source = .staticProfile(profile)
        self.font = font
        self.color = color
        self._generatedConsumerID = State(
            initialValue: "nostr-profile-name.\(UUID().uuidString)")
        self._claimedPubkey = State(initialValue: nil)
    }

    /// Self-claiming variant: claim the kind:0 for `pubkey` from the host, read
    /// the resolved projection reactively, release on disappear.
    init(
        pubkey: String,
        font: Font = .body,
        color: Color = .primary,
        consumerID: String? = nil
    ) {
        self.source = .selfClaim(pubkey: pubkey)
        self.font = font
        self.color = color
        self._generatedConsumerID = State(
            initialValue: consumerID ?? "nostr-profile-name.\(UUID().uuidString)")
        self._claimedPubkey = State(initialValue: nil)
    }

    /// Render a pre-resolved display label for an interest the caller already
    /// claims (see type doc). Render-only: no claim/release here.
    init(
        displayName: String,
        font: Font = .headline,
        color: Color = .primary
    ) {
        self.source = .preResolved(displayName)
        self.font = font
        self.color = color
        self._generatedConsumerID = State(initialValue: "")
        self._claimedPubkey = State(initialValue: nil)
    }

    /// Pubkey this leaf is responsible for claiming, if any. `nil` in the
    /// render-only `displayName:` mode.
    private var claimPubkey: String? {
        switch source {
        case .preResolved:
            return nil
        case .staticProfile(let profile):
            return profile.pubkey
        case .selfClaim(let pubkey):
            return pubkey
        }
    }

    /// The label to render given the current host projection.
    private func resolvedLabel() -> String {
        switch source {
        case .preResolved(let label):
            return label
        case .staticProfile(let profile):
            return profile.display
        case .selfClaim(let pubkey):
            // Prefer the live projection's `display` (`displayName` when the
            // kind:0 has resolved, else the kernel's `npubShort`). Until the
            // host has any card for the key, fall back to `shortHex` — the same
            // abbreviation the kernel bakes into `npubShort` (KernelModel) — so
            // the surface is never blank while the claim is in flight.
            return profileHost?.profile(forPubkey: pubkey)?.display
                ?? pubkey.shortHex
        }
    }

    var body: some View {
        let view = Text(resolvedLabel())
            .font(font)
            .foregroundStyle(color)
            .lineLimit(1)
            .accessibilityLabel("Display name: \(resolvedLabel())")

        if let claimPubkey {
            // Self-claim/release on mount, mirroring `NostrAvatar` exactly so
            // every refcounted claim has a matched release (no leaked inflight
            // profile requests).
            return AnyView(
                view
                    .task(id: claimPubkey) {
                        await MainActor.run {
                            if let claimedPubkey, claimedPubkey != claimPubkey {
                                profileHost?.releaseProfile(
                                    pubkey: claimedPubkey,
                                    consumerID: generatedConsumerID)
                            }
                            claimedPubkey = claimPubkey
                            // ADR-0063 Lane E (#1671): bind the per-key observer
                            // (only this name re-renders on this pubkey's row),
                            // then resolve via `resolve_ref(Profile, …,
                            // .profileRef, .cacheOk)` — inline/standalone name is
                            // a list/reading context (no live sub).
                            if let host = profileHost {
                                rowObserver.observe(host.profileRowChanged, pubkey: claimPubkey)
                            }
                            profileHost?.resolveProfile(
                                pubkey: claimPubkey,
                                consumerID: generatedConsumerID,
                                shape: .profileRef,
                                liveness: .cacheOk)
                        }
                    }
                    .onDisappear {
                        if let claimedPubkey {
                            profileHost?.releaseProfile(
                                pubkey: claimedPubkey, consumerID: generatedConsumerID)
                            self.claimedPubkey = nil
                        }
                    }
            )
        }
        return AnyView(view)
    }
}
