import SwiftUI

enum ChirpColor {
    static let accent = Color.accentColor
    static let accentSoft = Color.accentColor.opacity(0.12)
    static let bg = Color(.systemGroupedBackground)
    static let surface = Color(.systemBackground)
    static let surfaceMuted = Color(.secondarySystemGroupedBackground)
    static let hairline = Color(.separator).opacity(0.45)
    static let textPrimary = Color.primary
    static let textSecondary = Color.secondary
    static let textTertiary = Color(.tertiaryLabel)
    static let positive = Color.green
    static let zap = Color.yellow
    static let like = Color.red

    /// Deterministic avatar gradient from a hex color string the kernel
    /// supplies (`avatarColor`). Falls back to the accent.
    static func avatar(from hex: String) -> LinearGradient {
        let base = Color(hex: hex) ?? accent
        return LinearGradient(
            colors: [base, base.opacity(0.65)],
            startPoint: .topLeading, endPoint: .bottomTrailing)
    }
}

enum ChirpFont {
    static let largeTitle = Font.largeTitle.weight(.bold)
    static let title = Font.title2.weight(.semibold)
    static let headline = Font.headline
    static let body = Font.body
    static let callout = Font.callout
    static let caption = Font.caption
    static let mono = Font.footnote.monospaced()
}

enum ChirpSpace {
    static let xs: CGFloat = 4
    static let s: CGFloat = 8
    static let m: CGFloat = 12
    static let l: CGFloat = 16
    static let xl: CGFloat = 24
    static let xxl: CGFloat = 36
    static let radius: CGFloat = 16
    static let radiusSmall: CGFloat = 10
}

struct ChirpBackdrop: View {
    var body: some View {
        ZStack {
            Rectangle().fill(ChirpColor.bg)
            Rectangle().fill(.ultraThinMaterial).opacity(0.22)
        }
        .ignoresSafeArea()
    }
}

struct GlassCard<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        content
            .padding(ChirpSpace.l)
            .frame(maxWidth: .infinity, alignment: .leading)
            .chirpGlass(cornerRadius: ChirpSpace.radius)
    }
}

struct ChirpGlassBackground: ViewModifier {
    var cornerRadius: CGFloat = ChirpSpace.radius
    var interactive = false

    func body(content: Content) -> some View {
        if #available(iOS 26.0, *) {
            content.glassEffect(
                interactive ? .regular.interactive() : .regular,
                in: .rect(cornerRadius: cornerRadius)
            )
        } else {
            content.background(.regularMaterial, in: RoundedRectangle(cornerRadius: cornerRadius))
        }
    }
}

struct ChirpSurfaceBackground: ViewModifier {
    var cornerRadius: CGFloat = ChirpSpace.radius
    var muted = false

    func body(content: Content) -> some View {
        content
            .background {
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .fill(muted ? ChirpColor.surfaceMuted : ChirpColor.surface)
            }
            .overlay {
                RoundedRectangle(cornerRadius: cornerRadius, style: .continuous)
                    .stroke(ChirpColor.hairline, lineWidth: 0.5)
            }
    }
}

extension View {
    func chirpGlass(cornerRadius: CGFloat = ChirpSpace.radius, interactive: Bool = false) -> some View {
        modifier(ChirpGlassBackground(cornerRadius: cornerRadius, interactive: interactive))
    }

    func chirpSurface(cornerRadius: CGFloat = ChirpSpace.radius, muted: Bool = false) -> some View {
        modifier(ChirpSurfaceBackground(cornerRadius: cornerRadius, muted: muted))
    }

    func chirpScreenBackground() -> some View {
        background(ChirpBackdrop())
    }
}

/// Circular avatar — uses the kernel-supplied picture URL with a
/// plain placeholder fill + initials (D1: never blank).
struct ChirpAvatar: View {
    let url: String?
    let initials: String
    let colorHex: String
    var size: CGFloat = 44
    var body: some View {
        ZStack {
            Circle().fill(ChirpColor.avatar(from: colorHex))
            if let url, let u = URL(string: url) {
                AsyncImage(url: u) { img in
                    img.resizable().scaledToFill()
                } placeholder: { Color.clear }
            }
            if url == nil || url?.isEmpty == true {
                Text(initials)
                    .font(.system(size: size * 0.4, weight: .semibold))
                    .foregroundStyle(.primary)
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .overlay(Circle().stroke(.separator.opacity(0.35), lineWidth: 0.5))
    }
}

/// Primary call-to-action button — standard SwiftUI Button.
struct ChirpPrimaryButton: View {
    let title: String
    var systemImage: String? = nil
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            HStack(spacing: ChirpSpace.s) {
                if let systemImage { Image(systemName: systemImage) }
                Text(title)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 14)
        }
        .buttonStyle(ChirpGlassButtonStyle(prominent: true))
    }
}

/// Plain section header.
struct ChirpSectionHeader: View {
    let title: String
    var body: some View {
        Text(title)
            .font(.caption.weight(.semibold))
            .foregroundStyle(.secondary)
            .textCase(.uppercase)
    }
}

/// Standard empty / loading placeholder.
struct ChirpPlaceholder: View {
    let systemImage: String
    let title: String
    var subtitle: String? = nil
    var body: some View {
        VStack(spacing: ChirpSpace.m) {
            Image(systemName: systemImage)
                .font(.system(size: 36, weight: .regular))
                .symbolRenderingMode(.hierarchical)
            Text(title)
                .font(.title3.weight(.semibold))
            if let subtitle {
                Text(subtitle)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(ChirpSpace.xl)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

struct ChirpGlassButtonStyle: ButtonStyle {
    var prominent = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(.callout.weight(.semibold))
            .foregroundStyle(prominent ? Color(.systemBackground) : .primary)
            .padding(.horizontal, ChirpSpace.l)
            .padding(.vertical, ChirpSpace.m)
            .frame(minHeight: 44)
            .background {
                if prominent {
                    RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .fill(.tint)
                }
            }
            .chirpGlass(cornerRadius: 14, interactive: true)
            .opacity(configuration.isPressed ? 0.72 : 1)
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
