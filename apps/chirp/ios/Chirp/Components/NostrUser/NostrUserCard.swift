import SwiftUI

/// Compact author header: avatar, display name, and optional NIP-05 badge.
///
/// Registry component `swiftui/user-card`, installed into Chirp. Composes the
/// three atomic primitives (`NostrAvatar`, `NostrProfileName`,
/// `NostrNip05Badge`) so every screen renders an identical user header with no
/// per-screen duplication (issue #995).
///
/// The wire is Rust-authored — Swift never resolves display name, NIP-05, or
/// avatar URL itself (aim.md §6.9). Tap routes through `onTap` so the card can
/// sit in any navigation stack.
struct NostrUserCard: View {
    let profile: ProfileWire
    var avatarSize: CGFloat
    var onTap: ((String) -> Void)?

    init(
        profile: ProfileWire,
        avatarSize: CGFloat = 40,
        onTap: ((String) -> Void)? = nil
    ) {
        self.profile = profile
        self.avatarSize = avatarSize
        self.onTap = onTap
    }

    var body: some View {
        HStack(spacing: 10) {
            NostrAvatar(profile: profile, size: avatarSize)
                .equatable()

            VStack(alignment: .leading, spacing: 2) {
                NostrProfileName(profile: profile)

                if let badge = NostrNip05Badge(profile: profile) {
                    badge
                }
            }

            Spacer(minLength: 0)
        }
        .contentShape(Rectangle())
        .onTapGesture { onTap?(profile.pubkey) }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(profile.display), profile")
        .accessibilityAddTraits(.isButton)
    }
}
