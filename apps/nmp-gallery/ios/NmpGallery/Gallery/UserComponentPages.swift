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

private struct ProfileCardClaim: View {
    let pubkey: String
    let consumerID: String
    @Environment(GalleryModel.self) private var model

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .task(id: pubkey) {
                await MainActor.run {
                    model.resolveProfileCard(pubkey: pubkey, consumerID: consumerID)
                }
            }
            .onDisappear {
                model.releaseProfileRef(pubkey: pubkey, consumerID: consumerID)
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
                    .equatable()
            }
            PageFrame(caption: "Smaller size") {
                HStack(spacing: 12) {
                    NostrAvatar(pubkey: pubkey, size: 32)
                        .equatable()
                    NostrAvatar(pubkey: pubkey, size: 48)
                        .equatable()
                    NostrAvatar(pubkey: pubkey, size: 64)
                        .equatable()
                }
            }
            PageFrame(caption: "Identicon fallback (same pubkey, no picture URL)") {
                NostrAvatar(pubkey: pubkey, pictureUrl: nil, size: 80)
                    .equatable()
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
                        .equatable()
                    NostrProfileName(profile: model.bestEffortProfile)
                }
            }
        }
    }
}

// MARK: - user-nip05

/// Renders the NIP-05 badge component using the relay-backed profile.
///
/// Claims a full profile-card projection because the badge reads fields
/// beyond the ref/avatar shape. The failable initializer returns nil when
/// no NIP-05 is present on the profile, which is the correct degraded state,
/// not a loading state.
struct UserNip05Page: View {
    let pubkey: String
    @Environment(GalleryModel.self) private var model

    var body: some View {
        VStack(spacing: 16) {
            ProfileCardClaim(pubkey: pubkey, consumerID: "swiftui/user-nip05")
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
/// `npub` is always Rust-formatted (the fallback value pinned in
/// `GalleryModel.swift` before kind:0 arrives; replaced by the
/// kernel-supplied value once the real profile lands). `npubShort` is
/// `ProfileWire`'s own local truncation of `npub` (#3098).
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
    let pubkey: String
    let profile: ProfileWire

    var body: some View {
        VStack(spacing: 16) {
            ProfileCardClaim(pubkey: pubkey, consumerID: "swiftui/user-card")
            PageFrame(caption: "NostrUserCard(profile:)") {
                NostrUserCard(profile: profile)
            }
            PageFrame(caption: "Larger avatar") {
                NostrUserCard(profile: profile, avatarSize: 64)
            }
        }
    }
}
