import SwiftUI

/// Shared chrome for any component page: caption label + the component
/// centered in a card. Keeps the per-component pages tight.
private struct PageFrame<Content: View>: View {
    let caption: String
    @ViewBuilder var content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(caption)
                .font(.caption)
                .foregroundStyle(.secondary)
            VStack {
                content()
            }
            .frame(maxWidth: .infinity)
            .padding(20)
            .background(Color(.secondarySystemGroupedBackground))
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
    }
}

// MARK: - user-avatar

/// Renders the avatar component from only a pubkey.
///
/// The page does not pre-hydrate `ProfileWire`. `NostrAvatar` claims the
/// profile through `NostrProfileHost`, reads the current projection, and
/// falls back to a deterministic identicon until kind:0 arrives.
struct UserAvatarPage: View {
    let pubkey: String

    var body: some View {
        VStack(spacing: 16) {
            PageFrame(caption: "NostrAvatar(pubkey:)") {
                NostrAvatar(pubkey: pubkey, size: 80)
            }
            PageFrame(caption: "Smaller size") {
                HStack(spacing: 12) {
                    NostrAvatar(pubkey: pubkey, size: 32)
                    NostrAvatar(pubkey: pubkey, size: 48)
                    NostrAvatar(pubkey: pubkey, size: 64)
                }
            }
            PageFrame(caption: "Identicon fallback (same pubkey, no picture URL)") {
                NostrAvatar(pubkey: pubkey, pictureUrl: nil, size: 80)
            }
        }
    }
}

// MARK: - user-name

/// Renders the display-name component using the best-effort profile.
///
/// When `displayName` is nil (kind:0 not yet arrived) the registry
/// component falls back to the Rust-formatted `npubShort` — never a
/// Swift-side abbreviation (aim.md §6.9).
struct UserProfileNamePage: View {
    let profile: ProfileWire

    var body: some View {
        VStack(spacing: 16) {
            PageFrame(caption: "NostrProfileName(profile:)") {
                NostrProfileName(profile: profile)
            }
            PageFrame(caption: "Custom font") {
                NostrProfileName(profile: profile, font: .title2)
            }
        }
    }
}

// MARK: - user-nip05

/// Renders the NIP-05 badge component using the best-effort profile.
///
/// When the profile has no `nip05` field the failable initializer
/// returns nil and the first `PageFrame` shows the documented
/// "(no NIP-05 on this profile)" hint — that is the correct showcase for the
/// real-world behaviour, not a loading state. The second `PageFrame`
/// renders the direct initializer only when the same relay-backed profile
/// supplies a NIP-05 value.
struct UserNip05Page: View {
    let profile: ProfileWire

    var body: some View {
        VStack(spacing: 16) {
            PageFrame(caption: "NostrNip05Badge(profile:)") {
                if let badge = NostrNip05Badge(profile: profile) {
                    badge
                } else {
                    Text("(no NIP-05 on this profile)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            PageFrame(caption: "Direct init from profile value") {
                if let nip05 = profile.nip05 {
                    NostrNip05Badge(nip05: nip05)
                } else {
                    Text("(no NIP-05 on this profile)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }
}

// MARK: - user-npub

/// Renders the npub-chip component using the best-effort profile.
///
/// `npub` and `npubShort` are always Rust-formatted (fallback values
/// pinned in `GalleryModel.swift` match `nmp_core::display::short_npub`
/// before kind:0 arrives; replaced by the kernel-supplied values once
/// the real profile lands).
struct UserNpubPage: View {
    let profile: ProfileWire

    var body: some View {
        VStack(spacing: 16) {
            PageFrame(caption: "NostrNpubChip(profile:)") {
                NostrNpubChip(profile: profile)
            }
            PageFrame(caption: "Full npub (for reference)") {
                Text(profile.npub)
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
        }
    }
}

// MARK: - user-card

/// Renders the user-card component using the best-effort profile.
///
/// Composes `NostrAvatar` + `NostrProfileName` + `NostrNip05Badge`; each
/// piece degrades gracefully on missing fields, so the card renders on
/// the first frame (identicon + truncated npub, no badge) and upgrades
/// in place when kind:0 arrives.
struct UserCardPage: View {
    let profile: ProfileWire

    var body: some View {
        VStack(spacing: 16) {
            PageFrame(caption: "NostrUserCard(profile:)") {
                NostrUserCard(profile: profile)
            }
            PageFrame(caption: "Larger avatar") {
                NostrUserCard(profile: profile, avatarSize: 64)
            }
        }
    }
}
