import SwiftUI
import Foundation

/// Circular avatar for a Nostr pubkey. Shows the profile picture when the
/// caller (or the host projection) has it; falls back to the deterministic
/// 5×5 symmetric identicon from `NostrIdenticon` (defined in
/// `Components/NostrContent/ContentTreeWire.swift`).
///
/// Registry component `swiftui/user-avatar`, installed into Chirp. The
/// load-bearing shared behaviour — claiming/releasing the profile interest
/// through `NostrProfileHost`, reading the current Rust-owned projection, and
/// the `AsyncImage` load — is unchanged from the registry source. The fallback
/// uses the shared `NostrIdenticon.identiconView` so iOS main-avatar, iOS
/// mention chip, and Android all render the same pixel pattern for a given
/// pubkey (closes #2224).
struct NostrAvatar: View, Equatable {
    @Environment(\.nostrProfileHost) private var profileHost

    let pubkey: String
    /// Explicit picture URL when the caller already has one (e.g. baked into a
    /// timeline snapshot). When `nil`, the current profile projection is read
    /// from the host.
    let url: String?
    var size: CGFloat = 44

    @State private var generatedConsumerID: String
    @State private var claimedPubkey: String?
    /// ADR-0063 Lane E (#1671): per-key observer so EXACTLY this avatar
    /// re-renders when its one pubkey's `refs.profile` row commits — no
    /// whole-map invalidation, no app-side cache. Bound to the host's
    /// `profileRowChanged` in `.task(id: pubkey)`.
    @StateObject private var rowObserver = KeyedRefRowObserver()

    /// Equatable conformance comparing only the rendered-value inputs.
    ///
    /// `@State` vars (`generatedConsumerID`, `claimedPubkey`) are internal
    /// identity managed by SwiftUI across body re-evaluations and must NOT
    /// participate in equality — including them would cause `.equatable()` to
    /// wrongly suppress re-renders when those internal vars change.
    nonisolated static func == (lhs: NostrAvatar, rhs: NostrAvatar) -> Bool {
        lhs.pubkey == rhs.pubkey
            && lhs.url == rhs.url
            && lhs.size == rhs.size
    }

    init(
        pubkey: String,
        url: String? = nil,
        size: CGFloat = 44,
        consumerID: String? = nil
    ) {
        self.pubkey = pubkey
        self.url = url
        self.size = size
        self._generatedConsumerID = State(
            initialValue: consumerID ?? "nostr-avatar.\(UUID().uuidString)")
        self._claimedPubkey = State(initialValue: nil)
    }

    /// Convenience initializer from a decoded `ProfileWire` projection.
    init(profile: ProfileWire, size: CGFloat = 44) {
        self.init(
            pubkey: profile.pubkey,
            url: profile.pictureUrl,
            size: size)
    }

    var body: some View {
        // ADR-0063 Lane E (#1671): when the caller did not bake a URL, read the
        // live picture from the unified `refs.profile` keyed-ref cache. The
        // `rowObserver` (bound below) forces a re-read of exactly this leaf when
        // this pubkey's row commits, so the avatar fills in reactively with no
        // app-side cache and no whole-map invalidation.
        let resolvedUrl = url ?? profileHost?.profile(forPubkey: pubkey)?.pictureUrl

        ZStack {
            if let resolvedUrl, let u = URL(string: resolvedUrl) {
                // Gray placeholder visible while the image loads.
                Circle().fill(ChirpColor.avatarFallbackBackground)
                AsyncImage(url: u) { phase in
                    if let img = phase.image {
                        FadingImage(image: img)
                    }
                }
            } else {
                // No URL yet: render the 5x5 identicon (same algorithm as
                // iOS mention chips and Android avatars — closes #2224).
                NostrIdenticon.identiconView(forPubkey: pubkey, size: size)
                    .clipShape(Circle())
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .overlay(Circle().stroke(ChirpColor.hairlineSoft, lineWidth: 0.5))
        .accessibilityHidden(true)
        .task(id: pubkey) {
            await MainActor.run {
                if let claimedPubkey, claimedPubkey != pubkey {
                    profileHost?.releaseProfile(
                        pubkey: claimedPubkey,
                        consumerID: generatedConsumerID)
                }
                claimedPubkey = pubkey
                // ADR-0063 Lane E (#1671): bind the per-key observer so only
                // this avatar re-renders when this pubkey's row arrives, then
                // resolve via `resolve_ref(Profile, …, .profileRef, .cacheOk)` —
                // the lightweight feed/list shape with cache + OneShot fill and
                // no live sub. The profile screen is the only `.live` resolver.
                if let host = profileHost {
                    rowObserver.observe(host.profileRowChanged, pubkey: pubkey)
                }
                profileHost?.resolveProfile(
                    pubkey: pubkey, consumerID: generatedConsumerID,
                    shape: .profileRef, liveness: .cacheOk)
            }
        }
        .onDisappear {
            if let claimedPubkey {
                profileHost?.releaseProfile(pubkey: claimedPubkey, consumerID: generatedConsumerID)
                self.claimedPubkey = nil
            }
        }
    }
}
