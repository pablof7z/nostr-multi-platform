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

/// Loading placeholder when the demo profile's kind:0 hasn't arrived yet.
private struct ProfileLoading: View {
    var body: some View {
        VStack(spacing: 8) {
            ProgressView()
            Text("Loading profile from relays…")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 30)
    }
}

// MARK: - user-avatar

struct UserAvatarPage: View {
    let profile: ProfileWire?

    var body: some View {
        if let profile {
            VStack(spacing: 16) {
                PageFrame(caption: "NostrAvatar(profile:)") {
                    NostrAvatar(profile: profile, size: 80)
                }
                PageFrame(caption: "Smaller size") {
                    HStack(spacing: 12) {
                        NostrAvatar(profile: profile, size: 32)
                        NostrAvatar(profile: profile, size: 48)
                        NostrAvatar(profile: profile, size: 64)
                    }
                }
                PageFrame(caption: "Identicon fallback (no picture URL)") {
                    NostrAvatar(pubkey: profile.pubkey, pictureUrl: nil, size: 80)
                }
            }
        } else {
            ProfileLoading()
        }
    }
}

// MARK: - user-name

struct UserProfileNamePage: View {
    let profile: ProfileWire?

    var body: some View {
        if let profile {
            VStack(spacing: 16) {
                PageFrame(caption: "NostrProfileName(profile:)") {
                    NostrProfileName(profile: profile)
                }
                PageFrame(caption: "Custom font") {
                    NostrProfileName(profile: profile, font: .title2)
                }
            }
        } else {
            ProfileLoading()
        }
    }
}

// MARK: - user-nip05

struct UserNip05Page: View {
    let profile: ProfileWire?

    var body: some View {
        if let profile {
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
                PageFrame(caption: "Always renders (direct init)") {
                    NostrNip05Badge(nip05: "demo@nmp.dev")
                }
            }
        } else {
            ProfileLoading()
        }
    }
}

// MARK: - user-npub

struct UserNpubPage: View {
    let profile: ProfileWire?

    var body: some View {
        if let profile {
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
        } else {
            ProfileLoading()
        }
    }
}

// MARK: - user-card

struct UserCardPage: View {
    let profile: ProfileWire?

    var body: some View {
        if let profile {
            VStack(spacing: 16) {
                PageFrame(caption: "NostrUserCard(profile:)") {
                    NostrUserCard(profile: profile)
                }
                PageFrame(caption: "Larger avatar") {
                    NostrUserCard(profile: profile, avatarSize: 64)
                }
            }
        } else {
            ProfileLoading()
        }
    }
}
