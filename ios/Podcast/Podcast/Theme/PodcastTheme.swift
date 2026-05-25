import SwiftUI

enum PodcastColor {
    static let accent = Color.accentColor
    static let accentSoft = accent.opacity(0.12)
    static let bg = Color(.systemBackground)
    static let surface = Color(.secondarySystemBackground)
    static let surfaceElevated = Color(.tertiarySystemBackground)
    static let hairline = Color(.separator)
    static let hairlineSoft = hairline.opacity(0.35)
    static let transparent = Color.clear
    static let textPrimary = Color.primary
    static let textSecondary = Color.secondary
    static let textTertiary = Color(.tertiaryLabel)
    static let link = Color(.link)
    static let success = Color(.systemGreen)
    static let warning = Color(.systemOrange)
    static let danger = Color(.systemRed)
    static let network = Color(.systemCyan)
    static let positive = success
    static let zap = Color(.systemYellow)
    static let like = Color(.systemRed)
    static let systemFill = Color(.systemFill)
    static let secondaryFill = Color(.secondarySystemFill)
    static let controlDisabledBackground = textSecondary.opacity(0.2)
    static let focusedBackground = accent.opacity(0.06)
    static let focusedLine = accent.opacity(0.28)
    static let mediaBackdrop = Color(.black)
    static let mediaForeground = Color(.white)
    static let mediaSecondaryForeground = Color(.systemGray3).opacity(0.7)
    static let inverseForeground = Color(.white)
    static let emphasisForeground = Color(.white)
    static let errorBannerBackground = danger.opacity(0.9)

    /// Deterministic avatar gradient from a hex color string. Falls back to accent.
    static func avatar(from hex: String) -> LinearGradient {
        avatarGradient(base: avatarBase(from: hex))
    }

    static func avatarHeader(from hex: String?) -> LinearGradient {
        avatarGradient(base: avatarBase(from: hex))
    }

    static func avatarBase(from hex: String?) -> Color {
        guard let hex, let color = Color(hex: hex) else { return accent }
        return color
    }

    private static func avatarGradient(base: Color) -> LinearGradient {
        return LinearGradient(
            colors: [base, base.opacity(0.65)],
            startPoint: .topLeading, endPoint: .bottomTrailing)
    }
}

enum PodcastFont {
    static let largeTitle = Font.largeTitle.weight(.bold)
    static let title = Font.title2.weight(.semibold)
    static let headline = Font.headline
    static let body = Font.body
    static let callout = Font.callout
    static let caption = Font.caption
    static let mono = Font.footnote.monospaced()
}

enum PodcastSpace {
    static let xs: CGFloat = 4
    static let s: CGFloat = 8
    static let m: CGFloat = 12
    static let l: CGFloat = 16
    static let xl: CGFloat = 24
    static let xxl: CGFloat = 36
    static let radius: CGFloat = 20
    static let radiusSmall: CGFloat = 12
}

struct PodcastBackdrop: View {
    var body: some View {
        Rectangle().fill(.background)
        .ignoresSafeArea()
    }
}

extension View {
    func podcastScreenBackground() -> some View {
        background(PodcastBackdrop())
    }
}

/// Fades in when it appears — used by PodcastAvatar to soften image loads.
private struct FadingImage: View {
    let image: Image
    @State private var opacity: Double = 0
    var body: some View {
        image.resizable().scaledToFill()
            .opacity(opacity)
            .onAppear { withAnimation(.easeInOut(duration: 0.2)) { opacity = 1 } }
    }
}

/// Circular avatar — uses the kernel-supplied picture URL with a
/// plain placeholder fill + initials (never blank).
struct PodcastAvatar: View {
    let url: String?
    let initials: String
    let colorHex: String
    var size: CGFloat = 44
    var body: some View {
        ZStack {
            Circle().fill(PodcastColor.avatar(from: colorHex))
            if let url, let u = URL(string: url) {
                AsyncImage(url: u) { phase in
                    if let img = phase.image {
                        FadingImage(image: img)
                    }
                }
            }
            if url == nil || url?.isEmpty == true {
                Text(initials)
                    .font(.system(size: size * 0.4, weight: .semibold))
                    .foregroundStyle(.primary)
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .overlay(Circle().stroke(PodcastColor.hairlineSoft, lineWidth: 0.5))
    }
}

/// Primary call-to-action button.
struct PodcastPrimaryButton: View {
    let title: String
    var systemImage: String? = nil
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            HStack(spacing: PodcastSpace.s) {
                if let systemImage { Image(systemName: systemImage) }
                Text(title)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 14)
        }
        .buttonStyle(.borderedProminent)
    }
}

/// Plain section header.
struct PodcastSectionHeader: View {
    let title: String
    var body: some View {
        Text(title)
            .font(.caption.weight(.semibold))
            .foregroundStyle(.secondary)
            .textCase(.uppercase)
    }
}

/// Standard empty / loading placeholder.
struct PodcastPlaceholder: View {
    let systemImage: String
    let title: String
    var subtitle: String? = nil
    var body: some View {
        VStack(spacing: PodcastSpace.m) {
            Image(systemName: systemImage)
                .font(.system(size: 44, weight: .light))
                .symbolRenderingMode(.hierarchical)
            Text(title)
                .font(.headline)
            if let subtitle {
                Text(subtitle)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(PodcastSpace.xl)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

extension Color {
    /// Parse "#RRGGBB" / "RRGGBB" — used for kernel-supplied avatar colors.
    init?(hex: String) {
        var s = hex.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("#") { s.removeFirst() }
        guard s.count == 6, let v = UInt64(s, radix: 16) else { return nil }
        self = Color(
            red: Double((v >> 16) & 0xFF) / 255,
            green: Double((v >> 8) & 0xFF) / 255,
            blue: Double(v & 0xFF) / 255)
    }
}

/// Fades a view in from opacity 0 to 1 on first appear.
private struct FadeInModifier: ViewModifier {
    @State private var opacity: Double = 0

    func body(content: Content) -> some View {
        content
            .opacity(opacity)
            .onAppear {
                withAnimation(.easeInOut(duration: 0.3)) { opacity = 1 }
            }
    }
}

extension View {
    func fadeIn() -> some View {
        modifier(FadeInModifier())
    }
}
