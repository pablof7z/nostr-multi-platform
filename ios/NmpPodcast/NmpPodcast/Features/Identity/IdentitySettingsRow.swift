import SwiftUI

// MARK: - IdentitySettingsRow
//
// First row in `SettingsView`, sitting above Library. Per identity-05-synthesis
// §4.1: T1 clear single quiet lift, 60pt avatar, hairline ring, mode badge +
// npub fragment in mono caption. Tap pushes `IdentityRootView`.

struct IdentitySettingsRow: View {

    private enum Layout {
        static let avatarSize: CGFloat = 60
        static let rowSpacing: CGFloat = 14
        static let labelSpacing: CGFloat = 4
    }

    @Environment(UserIdentityStore.self) private var identity

    var body: some View {
        NavigationLink {
            IdentityRootView()
        } label: {
            HStack(spacing: Layout.rowSpacing) {
                IdentityAvatarView(
                    url: profile?.pictureURL,
                    initial: profile?.displayName.first,
                    size: Layout.avatarSize
                )
                VStack(alignment: .leading, spacing: Layout.labelSpacing) {
                    Text(topLine)
                        .font(AppTheme.Typography.headline)
                        .foregroundStyle(.primary)
                        .lineLimit(1)
                    secondLine
                        .lineLimit(1)
                }
                Spacer(minLength: AppTheme.Spacing.xs)
            }
            .padding(.vertical, AppTheme.Spacing.xs)
            .accessibilityElement(children: .combine)
            .accessibilityLabel(accessibilityLabel)
            .accessibilityAddTraits(.isButton)
        }
    }

    // MARK: - Composition

    private var profile: UserProfileDisplay? {
        UserProfileDisplay.from(identity: identity)
    }

    /// `display_name` → `name` slug → npub short fragment (per §4.1).
    private var topLine: String {
        if let p = profile, !p.displayName.isEmpty { return p.displayName }
        if let p = profile, !p.slug.isEmpty       { return p.slug }
        return identity.npubShort ?? "Identity"
    }

    /// Mode badge (plain) + npub fragment in mono.
    private var secondLine: some View {
        HStack(spacing: 6) {
            ModeBadge(mode: identity.mode, variant: .plain)
                .font(AppTheme.Typography.caption2)
                .foregroundStyle(.secondary)
            Text("·")
                .font(AppTheme.Typography.caption2)
                .foregroundStyle(.tertiary)
            if let short = identity.npubShort {
                Text(short)
                    .font(AppTheme.Typography.mono)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var accessibilityLabel: String {
        let mode: String
        switch identity.mode {
        case .remoteSigner: mode = "Bunker via Amber"
        case .localKey:     mode = "Local key"
        case .none:         mode = "No identity"
        }
        let pub = identity.npubShort ?? ""
        return "Identity. \(topLine). \(mode). \(pub)"
    }
}
