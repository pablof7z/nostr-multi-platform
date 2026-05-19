import SwiftUI
import UIKit

// MARK: - Identity

struct OnboardingIdentityPage: View {
    @Binding var agentName: String
    @Binding var profilePicture: String

    private enum Layout {
        static let avatarSize: CGFloat = 90
        static let avatarIconSize: CGFloat = 36
        static let avatarFontSize: CGFloat = 32
        /// Fixed width reserved for the leading icon in each input field row,
        /// ensuring all TextField prompts left-align regardless of icon width.
        static let fieldIconWidth: CGFloat = 22
    }

    var body: some View {
        VStack(spacing: AppTheme.Spacing.lg) {
            Spacer()

            avatarPreview

            VStack(spacing: AppTheme.Spacing.sm) {
                Text("Name your agent")
                    .font(AppTheme.Typography.largeTitle)
                    .foregroundStyle(.white)

                Text("Give your agent a name and a face. Both are optional and can change later.")
                    .font(AppTheme.Typography.body)
                    .foregroundStyle(.white.opacity(0.8))
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, AppTheme.Spacing.md)
                    .fixedSize(horizontal: false, vertical: true)
            }

            GlassEffectContainer {
                VStack(spacing: AppTheme.Spacing.sm) {
                    fieldRow(icon: "person.fill", placeholder: "Agent name", text: $agentName)
                    fieldRow(icon: "photo.fill", placeholder: "Profile picture URL (optional)", text: $profilePicture, keyboard: .URL)
                }
            }

            Spacer()
        }
        .animation(AppTheme.Animation.springFast, value: validPictureURL)
        .animation(AppTheme.Animation.springFast, value: nameInitial)
    }

    // MARK: - Avatar Preview

    private var validPictureURL: URL? {
        let trimmed = profilePicture.trimmed
        guard !trimmed.isEmpty,
              let url = URL(string: trimmed),
              let scheme = url.scheme?.lowercased(),
              scheme == "http" || scheme == "https"
        else { return nil }
        return url
    }

    private var nameInitial: String {
        agentName.trimmed.first.map(String.init) ?? ""
    }

    @ViewBuilder
    private var avatarPreview: some View {
        ZStack {
            Circle()
                .fill(Color.white.opacity(0.12))
                .frame(width: Layout.avatarSize, height: Layout.avatarSize)
                .glassEffect(.regular, in: .circle)
                .overlay(Circle().strokeBorder(.white.opacity(0.3), lineWidth: 1))

            if let url = validPictureURL {
                CachedAsyncImage(url: url) { phase in
                    switch phase {
                    case .success(let image):
                        image.resizable().scaledToFill()
                    case .failure:
                        initialsOrPlaceholder
                    default:
                        ProgressView().tint(.white)
                    }
                }
                .frame(width: Layout.avatarSize, height: Layout.avatarSize)
                .clipShape(Circle())
            } else {
                initialsOrPlaceholder
            }
        }
        .appShadow(AppTheme.Shadow.lifted)
        .accessibilityLabel(validPictureURL != nil ? "Profile picture preview" : "Default profile placeholder")
    }

    @ViewBuilder
    private var initialsOrPlaceholder: some View {
        if nameInitial.isEmpty {
            Image(systemName: "person.crop.circle.badge.plus")
                .font(.system(size: Layout.avatarIconSize, weight: .semibold))
                .foregroundStyle(.white.opacity(0.8))
        } else {
            Text(nameInitial.uppercased())
                .font(.system(size: Layout.avatarFontSize, weight: .bold, design: .rounded))
                .foregroundStyle(.white)
        }
    }

    // MARK: - Field

    private func fieldRow(icon: String, placeholder: String, text: Binding<String>, keyboard: UIKeyboardType = .default) -> some View {
        HStack(spacing: AppTheme.Spacing.sm) {
            Image(systemName: icon)
                .foregroundStyle(.white.opacity(0.7))
                .frame(width: Layout.fieldIconWidth)
            TextField(placeholder, text: text)
                .textInputAutocapitalization(keyboard == .URL ? .never : .words)
                .autocorrectionDisabled(keyboard == .URL)
                .keyboardType(keyboard)
                .foregroundStyle(.white)
        }
        .padding(.horizontal, AppTheme.Spacing.md)
        .padding(.vertical, OnboardingLayout.fieldVerticalPadding)
        .glassEffect(.regular, in: .rect(cornerRadius: AppTheme.Corner.lg))
        .overlay(
            RoundedRectangle(cornerRadius: AppTheme.Corner.lg, style: .continuous)
                .strokeBorder(.white.opacity(0.25), lineWidth: 1)
        )
    }
}
