import SwiftUI

// MARK: - IdentityRootView
//
// Per identity-05-synthesis §4.2. T0 paper background — the page reads like
// front matter, not a config screen. The Edit profile button is the only T3
// fill on the root. The mode badge is the full-page place where the signer
// flavour appears (T2 capsule, glass.agent only when Bunker).

struct IdentityRootView: View {

    private enum Layout {
        static let avatarSize: CGFloat = 96
        static let editButtonWidth: CGFloat = 220
        static let editButtonVerticalPadding: CGFloat = 12
        static let blockSpacing: CGFloat = AppTheme.Spacing.lg
        static let aboutQuoteSize: CGFloat = 28
    }

    @Environment(UserIdentityStore.self) private var identity
    @State private var editPresented = false

    var body: some View {
        ScrollView {
            VStack(spacing: Layout.blockSpacing) {
                hero
                IdentityRootAboutBlock(profile: profile)
                Divider().padding(.horizontal, AppTheme.Spacing.lg)
                IdentityRootAccountIDBlock(npub: identity.npub, npubShort: identity.npubShort)
                Divider().padding(.horizontal, AppTheme.Spacing.lg)
                advancedRow
            }
            .padding(.vertical, AppTheme.Spacing.lg)
            .frame(maxWidth: .infinity, alignment: .center)
        }
        .background(Color(.systemBackground))
        .navigationTitle("Identity")
        .navigationBarTitleDisplayMode(.inline)
        .navigationDestination(isPresented: $editPresented) {
            EditProfileView()
        }
    }

    // MARK: - Hero

    @ViewBuilder
    private var hero: some View {
        VStack(spacing: AppTheme.Spacing.md) {
            IdentityAvatarView(
                url: profile?.pictureURL,
                initial: profile?.displayName.first,
                size: Layout.avatarSize
            )
            VStack(spacing: AppTheme.Spacing.xs) {
                Text(profile?.displayName ?? "Welcome")
                    .font(AppTheme.Typography.largeTitle)
                    .multilineTextAlignment(.center)
                if let slug = profile?.slug, !slug.isEmpty {
                    Text(slug)
                        .font(AppTheme.Typography.mono)
                        .foregroundStyle(.tertiary)
                }
            }
            ModeBadge(mode: identity.mode, variant: .capsule)
            editButton
        }
        .padding(.horizontal, AppTheme.Spacing.lg)
    }

    private var editButton: some View {
        Button {
            editPresented = true
        } label: {
            Text("Edit profile")
                .font(AppTheme.Typography.headline)
                .frame(width: Layout.editButtonWidth)
                .padding(.vertical, Layout.editButtonVerticalPadding)
        }
        .buttonStyle(.glassProminent)
        .accessibilityLabel("Edit profile")
    }

    // MARK: - Advanced row

    private var advancedRow: some View {
        NavigationLink {
            AdvancedView()
        } label: {
            HStack(spacing: AppTheme.Spacing.sm) {
                Image(systemName: "slider.horizontal.3")
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(.secondary)
                    .frame(width: 24)
                Text("Advanced")
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(.primary)
                Spacer()
                Image(systemName: "chevron.forward")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, AppTheme.Spacing.lg)
            .padding(.vertical, AppTheme.Spacing.sm)
        }
        .accessibilityLabel("Advanced")
    }

    // MARK: - Derived

    private var profile: UserProfileDisplay? {
        UserProfileDisplay.from(identity: identity)
    }
}

// MARK: - About block (editorial pull-quote)

private struct IdentityRootAboutBlock: View {

    let profile: UserProfileDisplay?

    var body: some View {
        VStack(alignment: .leading, spacing: AppTheme.Spacing.sm) {
            sectionHeader("About")
            HStack(alignment: .top, spacing: AppTheme.Spacing.xs) {
                Text("\u{201C}")
                    .font(AppTheme.Typography.largeTitle)
                    .foregroundStyle(.tertiary)
                    .baselineOffset(-8)
                    .accessibilityHidden(true)
                Text(aboutText)
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(aboutIsEmpty ? .secondary : .primary)
                    .italic(aboutIsEmpty)
                    .multilineTextAlignment(.leading)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(.leading, AppTheme.Spacing.xs)
        }
        .padding(.horizontal, AppTheme.Spacing.lg)
    }

    private var aboutIsEmpty: Bool {
        (profile?.about ?? "").isBlank
    }

    private var aboutText: String {
        let raw = profile?.about ?? ""
        if raw.isBlank {
            return "A new account, freshly minted.\nTell people who you are."
        }
        return raw
    }
}

// MARK: - Account ID block

private struct IdentityRootAccountIDBlock: View {

    let npub: String?
    let npubShort: String?
    @State private var copied = false
    @State private var qrPresented = false

    var body: some View {
        VStack(alignment: .leading, spacing: AppTheme.Spacing.sm) {
            sectionHeader("Account ID")
            HStack(spacing: AppTheme.Spacing.sm) {
                Text(npubShort ?? "—")
                    .font(AppTheme.Typography.monoCaption)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer()
                copyChip
                qrChip
            }
            Text("Used to sync your account across apps that use Nostr. You can ignore this unless you know you need it.")
                .font(AppTheme.Typography.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(3)
        }
        .padding(.horizontal, AppTheme.Spacing.lg)
        .sheet(isPresented: $qrPresented) {
            if let npub {
                AgentIdentityQRView(npub: npub, name: "Account ID")
            }
        }
    }

    private var copyChip: some View {
        Button {
            guard let npub else { return }
            copyToClipboard(npub, isCopied: $copied, haptic: { Haptics.success() })
            UIAccessibility.post(notification: .announcement, argument: "Copied")
        } label: {
            Label(copied ? "Copied" : "Copy", systemImage: copied ? "checkmark" : "doc.on.doc")
                .font(AppTheme.Typography.caption)
                .padding(.horizontal, AppTheme.Spacing.sm)
                .padding(.vertical, 6)
        }
        .buttonStyle(.glass)
        .accessibilityLabel("Copy account ID")
        .disabled(npub == nil)
    }

    private var qrChip: some View {
        Button {
            qrPresented = true
        } label: {
            Image(systemName: "qrcode")
                .font(AppTheme.Typography.caption)
                .padding(.horizontal, AppTheme.Spacing.sm)
                .padding(.vertical, 6)
        }
        .buttonStyle(.glass)
        .accessibilityLabel("Show QR code")
        .disabled(npub == nil)
    }
}

// MARK: - Section header

@ViewBuilder
private func sectionHeader(_ title: String) -> some View {
    Text(title)
        .font(AppTheme.Typography.caption2.weight(.semibold))
        .foregroundStyle(.tertiary)
        .textCase(.uppercase)
        .tracking(0.4)
}
