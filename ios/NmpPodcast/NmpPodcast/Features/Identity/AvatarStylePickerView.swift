import SwiftUI

// MARK: - AvatarStylePickerView
//
// Per identity-05-synthesis §4.4. Six curated dicebear styles on a horizontal
// rail; each preview is generated with the user's pubkey-derived seed so the
// user sees their version of every style before committing. Tap = preview;
// "Use this style" commits and pops.

struct AvatarStylePickerView: View {

    private enum Layout {
        static let tileSize: CGFloat = 92
        static let labelGap: CGFloat = AppTheme.Spacing.xs
        static let tileSpacing: CGFloat = AppTheme.Spacing.md
    }

    @Binding var pictureURL: String
    @Environment(UserIdentityStore.self) private var identity
    @Environment(\.dismiss) private var dismiss
    @State private var preview: DicebearStyle?

    var body: some View {
        VStack(alignment: .leading, spacing: AppTheme.Spacing.lg) {
            Text("Each style is built from your account, so the result is always yours.")
                .font(AppTheme.Typography.subheadline)
                .foregroundStyle(.secondary)
                .padding(.horizontal, AppTheme.Spacing.lg)

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: Layout.tileSpacing) {
                    ForEach(DicebearStyle.allCases) { style in
                        tile(for: style)
                    }
                }
                .padding(.horizontal, AppTheme.Spacing.lg)
            }

            Spacer()

            Button {
                commit()
            } label: {
                Text("Use this style")
                    .font(AppTheme.Typography.headline)
                    .frame(maxWidth: 220)
                    .padding(.vertical, 12)
            }
            .buttonStyle(.glassProminent)
            .frame(maxWidth: .infinity)
            .disabled(preview == nil)
            .padding(.bottom, AppTheme.Spacing.xl)
        }
        .padding(.top, AppTheme.Spacing.lg)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(Color(.systemBackground))
        .navigationTitle("Choose a style")
        .navigationBarTitleDisplayMode(.inline)
    }

    // MARK: - Tile

    private func tile(for style: DicebearStyle) -> some View {
        Button {
            preview = style
            Haptics.selection()
        } label: {
            VStack(spacing: Layout.labelGap) {
                ZStack {
                    Circle()
                        .fill(AppTheme.Tint.surfaceMuted)
                    if let url = previewURL(for: style) {
                        CachedAsyncImage(
                            url: url,
                            targetSize: CGSize(width: Layout.tileSize, height: Layout.tileSize)
                        ) { phase in
                            if case .success(let img) = phase {
                                img.resizable().scaledToFill()
                            } else {
                                ProgressView().controlSize(.small)
                            }
                        }
                        .clipShape(Circle())
                    }
                }
                .frame(width: Layout.tileSize, height: Layout.tileSize)
                .overlay(selectionRing(for: style))

                HStack(spacing: 4) {
                    Text(style.displayName)
                        .font(AppTheme.Typography.caption)
                        .foregroundStyle(preview == style ? .primary : .secondary)
                    if isCurrent(style) {
                        Text("(current)")
                            .font(AppTheme.Typography.caption2)
                            .foregroundStyle(.tertiary)
                    }
                }
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(style.displayName)\(isCurrent(style) ? ", current" : "")")
        .accessibilityAddTraits(.isButton)
    }

    @ViewBuilder
    private func selectionRing(for style: DicebearStyle) -> some View {
        if preview == style {
            Circle()
                .strokeBorder(AppTheme.Gradients.agentAccent, lineWidth: 2)
        }
    }

    // MARK: - Helpers

    private func previewURL(for style: DicebearStyle) -> URL? {
        guard let hex = identity.publicKeyHex else { return nil }
        let seed = String(hex.prefix(16))
        return style.url(seed: seed)
    }

    private func isCurrent(_ style: DicebearStyle) -> Bool {
        // Match by inferring style from the URL path ("9.x/<style>/svg").
        let url = pictureURL.trimmed
        return url.contains("/\(style.rawValue)/")
    }

    private func commit() {
        guard let style = preview, let url = previewURL(for: style) else { return }
        pictureURL = url.absoluteString
        Haptics.success()
        dismiss()
    }
}
