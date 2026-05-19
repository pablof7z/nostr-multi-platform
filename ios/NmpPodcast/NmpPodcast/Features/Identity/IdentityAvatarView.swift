import SwiftUI

// MARK: - IdentityAvatarView
//
// Reusable avatar for every Identity surface. T0 paper, 1pt hairline ring,
// no breathing/rotation per identity-05-synthesis §4.2. Falls back to the
// display-name initial when the picture URL is empty / fails.

struct IdentityAvatarView: View {

    let url: URL?
    let initial: Character?
    var size: CGFloat = 96
    /// Tints the hairline ring. Default is the muted hairline colour; the
    /// Settings row tints `accent.live` (red) when remote signer failed and
    /// `warning` (orange) when last-acked age > 24h (per §4.1).
    var ringColor: Color = AppTheme.Tint.hairline

    var body: some View {
        ZStack {
            Circle()
                .fill(AppTheme.Tint.surfaceMuted)
            if let url {
                CachedAsyncImage(url: url, targetSize: CGSize(width: size, height: size)) { phase in
                    switch phase {
                    case .success(let image):
                        image.resizable().scaledToFill()
                    default:
                        initialView
                    }
                }
                .clipShape(Circle())
            } else {
                initialView
            }
        }
        .frame(width: size, height: size)
        .overlay(
            Circle()
                .strokeBorder(ringColor, lineWidth: 1)
        )
        .accessibilityHidden(true)
    }

    @ViewBuilder
    private var initialView: some View {
        if let initial {
            Text(String(initial).uppercased())
                .font(.system(size: size * 0.42, weight: .semibold, design: .rounded))
                .foregroundStyle(.secondary)
        } else {
            Image(systemName: "person.crop.circle")
                .font(.system(size: size * 0.6))
                .foregroundStyle(.tertiary)
        }
    }
}
