import SwiftUI

/// Twitter-style slide-in sidebar. Triggered by tapping the user avatar in the
/// navigation bar. Shows a left-anchored panel with the user's identity and
/// navigation shortcuts. Tap the darkened overlay to dismiss.
struct AppSidebarView: View {
    @Binding var selectedTab: RootTab
    @Binding var isPresented: Bool

    @Environment(UserIdentityStore.self) private var userIdentity

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            navSection
                .padding(.top, AppTheme.Spacing.sm)
            Spacer()
        }
        .safeAreaPadding(.top)
        .safeAreaPadding(.bottom)
        .background(Color(.systemBackground).ignoresSafeArea())
    }

    // MARK: - Header

    private var header: some View {
        let profile = UserProfileDisplay.from(identity: userIdentity)
        return VStack(alignment: .leading, spacing: AppTheme.Spacing.sm) {
            IdentityAvatarView(
                url: profile?.pictureURL,
                initial: profile?.displayName.first,
                size: 72
            )
            VStack(alignment: .leading, spacing: 3) {
                Text(profile?.displayName ?? "Welcome")
                    .font(AppTheme.Typography.title3)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                if let slug = profile?.slug, !slug.isEmpty {
                    Text("@\(slug)")
                        .font(AppTheme.Typography.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
        }
        .padding(.horizontal, AppTheme.Spacing.lg)
        .padding(.top, AppTheme.Spacing.lg)
        .padding(.bottom, AppTheme.Spacing.lg)
    }

    // MARK: - Main navigation

    private var navSection: some View {
        VStack(alignment: .leading, spacing: 0) {
            Divider()
                .padding(.horizontal, AppTheme.Spacing.md)
                .padding(.bottom, AppTheme.Spacing.xs)

            navRow("Home", icon: "house.fill", isActive: selectedTab == .home) {
                selectedTab = .home
                dismiss()
            }
            navRow("Library", icon: "tray.fill", isActive: selectedTab == .library) {
                selectedTab = .library
                dismiss()
            }
            navRow("Bookmarks", icon: "bookmark.fill", isActive: selectedTab == .bookmarks) {
                selectedTab = .bookmarks
                dismiss()
            }
            navRow("Clippings", icon: "scissors", isActive: selectedTab == .clippings) {
                selectedTab = .clippings
                dismiss()
            }
            navRow("Wiki", icon: "book.closed.fill", isActive: selectedTab == .wiki) {
                selectedTab = .wiki
                dismiss()
            }
        }
    }

    // MARK: - Row

    private func navRow(
        _ title: String,
        icon: String,
        isActive: Bool,
        action: @escaping () -> Void
    ) -> some View {
        Button(action: action) {
            HStack(spacing: AppTheme.Spacing.md) {
                Image(systemName: icon)
                    .font(.system(size: 19, weight: .medium))
                    .foregroundStyle(isActive ? Color.accentColor : .primary)
                    .frame(width: 26, alignment: .center)
                Text(title)
                    .font(AppTheme.Typography.title3)
                    .fontWeight(isActive ? .bold : .semibold)
                    .foregroundStyle(isActive ? Color.accentColor : .primary)
                Spacer()
            }
            .padding(.horizontal, AppTheme.Spacing.lg)
            .frame(height: 52)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .background {
            if isActive {
                RoundedRectangle(cornerRadius: AppTheme.Corner.sm)
                    .fill(Color.accentColor.opacity(0.08))
                    .padding(.horizontal, AppTheme.Spacing.sm)
            }
        }
    }

    // MARK: - Dismiss

    private func dismiss() {
        Haptics.selection()
        withAnimation(AppTheme.Animation.spring) {
            isPresented = false
        }
    }
}
