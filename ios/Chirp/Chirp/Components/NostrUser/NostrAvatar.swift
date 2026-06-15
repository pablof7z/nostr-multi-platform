import SwiftUI
import Foundation

/// Circular avatar for a Nostr pubkey. Shows the profile picture when the
/// caller (or the host projection) has it; falls back to Chirp's brand
/// gradient fill plus initials until the picture URL arrives.
///
/// Registry component `swiftui/user-avatar`, installed into Chirp and
/// customized per the registry's documented extension point ("edit the
/// fallback to match your app's brand palette"). The load-bearing shared
/// behaviour — claiming/releasing the profile interest through
/// `NostrProfileHost`, reading the current Rust-owned projection, and the
/// `AsyncImage` load — is unchanged from the registry source. Only the
/// fallback rendering uses Chirp's gradient + caller-supplied initials so
/// there is no visual regression versus the previous `ChirpAvatar`.
///
/// The bundled registry `NostrIdenticon` enum is intentionally *not* shipped
/// here: Chirp already vendors a richer `NostrIdenticon` (5×5 grid identicon)
/// in `Components/NostrContent/ContentTreeWire.swift`. Re-declaring it would be
/// a duplicate public symbol. The color fallback therefore reuses Chirp's
/// `ChirpColor.avatar(from:)` gradient keyed off the kernel-supplied
/// `colorHex`, matching every other avatar surface in the app.
struct NostrAvatar: View, Equatable {
    @Environment(\.nostrProfileHost) private var profileHost

    let pubkey: String
    /// Explicit picture URL when the caller already has one (e.g. baked into a
    /// timeline snapshot). When `nil`, the current profile projection is read
    /// from the host.
    let url: String?
    /// Brand initials shown on the gradient fallback. When `nil`, derived from
    /// the pubkey so the component still works without a caller-supplied label.
    let initials: String?
    /// Hex color the kernel supplies (`avatarColor`) that keys the gradient.
    /// When `nil`, derived from the pubkey.
    let colorHex: String?
    var size: CGFloat = 44

    @State private var generatedConsumerID: String
    @State private var claimedPubkey: String?

    /// Equatable conformance comparing only the rendered-value inputs.
    ///
    /// `@State` vars (`generatedConsumerID`, `claimedPubkey`) are internal
    /// identity managed by SwiftUI across body re-evaluations and must NOT
    /// participate in equality — including them would cause `.equatable()` to
    /// wrongly suppress re-renders when those internal vars change.
    nonisolated static func == (lhs: NostrAvatar, rhs: NostrAvatar) -> Bool {
        lhs.pubkey == rhs.pubkey
            && lhs.url == rhs.url
            && lhs.initials == rhs.initials
            && lhs.colorHex == rhs.colorHex
            && lhs.size == rhs.size
    }

    init(
        pubkey: String,
        url: String? = nil,
        initials: String? = nil,
        colorHex: String? = nil,
        size: CGFloat = 44,
        consumerID: String? = nil
    ) {
        self.pubkey = pubkey
        self.url = url
        self.initials = initials
        self.colorHex = colorHex
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
            initials: nil,
            colorHex: nil,
            size: size)
    }

    private var resolvedInitials: String {
        if let initials, !initials.isEmpty { return initials }
        return pubkey.displayInitials
    }

    private var resolvedColorHex: String {
        if let colorHex, !colorHex.isEmpty { return colorHex }
        return pubkey.pubkeyColorHex
    }

    var body: some View {
        let resolvedUrl = url ?? profileHost?.profile(forPubkey: pubkey)?.pictureUrl

        ZStack {
            Circle().fill(ChirpColor.avatar(from: resolvedColorHex))
            if let resolvedUrl, let u = URL(string: resolvedUrl) {
                AsyncImage(url: u) { phase in
                    if let img = phase.image {
                        FadingImage(image: img)
                    }
                }
            }
            if resolvedUrl == nil || resolvedUrl?.isEmpty == true {
                Text(resolvedInitials)
                    .font(.system(size: size * 0.4, weight: .semibold))
                    .foregroundStyle(.primary)
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
                // Feed/list avatar → `.cacheOk` (cache + OneShot fill, no live
                // sub). The profile screen is the only `.live` claimer.
                profileHost?.claimProfile(
                    pubkey: pubkey, consumerID: generatedConsumerID, liveness: .cacheOk)
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
