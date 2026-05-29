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

/// Renders the display-name component using the relay-backed profile.
///
/// Includes a `NostrAvatar` to own the profile claim — this mirrors the
/// real-world pattern where `NostrProfileName` appears alongside an avatar
/// in a note row or profile header, with `NostrAvatar` owning the claim
/// lifecycle. `NostrProfileName` just renders what it receives.
struct UserProfileNamePage: View {
    let pubkey: String
    @Environment(GalleryModel.self) private var model

    var body: some View {
        VStack(spacing: 16) {
            PageFrame(caption: "NostrProfileName(profile:)") {
                NostrProfileName(profile: model.bestEffortProfile)
            }
            PageFrame(caption: "Custom font") {
                NostrProfileName(profile: model.bestEffortProfile, font: .title2)
            }
            PageFrame(caption: "Context: NostrAvatar owns the claim") {
                HStack(spacing: 10) {
                    NostrAvatar(pubkey: pubkey, size: 32)
                    NostrProfileName(profile: model.bestEffortProfile)
                }
            }
        }
    }
}

// MARK: - user-nip05

/// Renders the NIP-05 badge component using the best-effort profile.
///
/// Renders the NIP-05 badge component using the relay-backed profile.
///
/// Includes a `NostrAvatar` to own the profile claim — same lifecycle
/// pattern as `UserProfileNamePage`. The failable initializer returns nil
/// when no NIP-05 is present on the profile, which is the correct
/// degraded state, not a loading state.
struct UserNip05Page: View {
    let pubkey: String
    @Environment(GalleryModel.self) private var model

    var body: some View {
        VStack(spacing: 16) {
            // NostrAvatar owns the profile claim for this page.
            NostrAvatar(pubkey: pubkey, size: 0)
                .frame(width: 0, height: 0)
                .clipped()
            PageFrame(caption: "NostrNip05Badge(profile:)") {
                if let badge = NostrNip05Badge(profile: model.bestEffortProfile) {
                    badge
                } else {
                    Text("(no NIP-05 on this profile)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            PageFrame(caption: "Direct init from profile value") {
                if let nip05 = model.bestEffortProfile.nip05 {
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
