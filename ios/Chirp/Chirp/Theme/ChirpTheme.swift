import SwiftUI

// ─────────────────────────────────────────────────────────────────────────
// FROZEN DESIGN SYSTEM — do not edit in Phase 2.
// Phase-2 agents READ this and use these tokens/components. To add a NEW
// component, create your own file under Components/ — never mutate this one
// (last-writer-wins would break sibling screens).
//
// Aesthetic: iOS 26 "liquid glass" — translucent layered materials, soft
// depth, rounded SF Pro, restrained violet accent, dark-first & adaptive.
// ─────────────────────────────────────────────────────────────────────────

enum ChirpColor {
    static let accent = Color(red: 0.52, green: 0.40, blue: 0.96)      // violet
    static let accentSoft = Color(red: 0.52, green: 0.40, blue: 0.96).opacity(0.16)
    static let bg = Color(.systemBackground)
    static let surface = Color(.secondarySystemBackground)
    static let hairline = Color.primary.opacity(0.08)
    static let textPrimary = Color.primary
    static let textSecondary = Color.secondary
    static let textTertiary = Color.primary.opacity(0.45)
    static let positive = Color(red: 0.20, green: 0.78, blue: 0.55)
    static let zap = Color(red: 1.0, green: 0.74, blue: 0.20)
    static let like = Color(red: 0.96, green: 0.28, blue: 0.42)

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
    static let largeTitle = Font.system(.largeTitle, design: .rounded).weight(.bold)
    static let title = Font.system(.title2, design: .rounded).weight(.semibold)
    static let headline = Font.system(.headline, design: .rounded)
    static let body = Font.system(.body, design: .default)
    static let callout = Font.system(.callout, design: .default)
    static let caption = Font.system(.caption, design: .rounded)
    static let mono = Font.system(.footnote, design: .monospaced)
}

enum ChirpSpace {
    static let xs: CGFloat = 4
    static let s: CGFloat = 8
    static let m: CGFloat = 12
    static let l: CGFloat = 16
    static let xl: CGFloat = 24
    static let xxl: CGFloat = 36
    static let radius: CGFloat = 20
    static let radiusSmall: CGFloat = 12
}

// ── Shared frozen components ──────────────────────────────────────────────

/// Glass card surface used for grouped content (compose, settings rows…).
struct GlassCard<Content: View>: View {
    @ViewBuilder var content: Content
    var body: some View {
        content
            .padding(ChirpSpace.l)
            .background(.ultraThinMaterial, in: RoundedRectangle(
                cornerRadius: ChirpSpace.radius, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: ChirpSpace.radius,
                style: .continuous).strokeBorder(ChirpColor.hairline))
}
}

/// Circular avatar — uses the kernel-supplied picture URL with a
/// deterministic gradient + initials placeholder (D1: never blank).
struct ChirpAvatar: View {
    let url: String?
    let initials: String
    let colorHex: String
    var size: CGFloat = 44
    var body: some View {
        ZStack {
            ChirpColor.avatar(from: colorHex)
            if let url, let u = URL(string: url) {
                AsyncImage(url: u) { img in
                    img.resizable().scaledToFill()
                } placeholder: { Color.clear }
            }
            if url == nil || url?.isEmpty == true {
                Text(initials).font(.system(size: size * 0.4,
                    weight: .semibold, design: .rounded))
                    .foregroundStyle(.white)
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .overlay(Circle().strokeBorder(ChirpColor.hairline))
    }
}

/// Pill button — primary call to action with the accent fill.
struct ChirpPrimaryButton: View {
    let title: String
    var systemImage: String? = nil
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            HStack(spacing: ChirpSpace.s) {
                if let systemImage { Image(systemName: systemImage) }
                Text(title).font(ChirpFont.headline)
            }
            .frame(maxWidth: .infinity).padding(.vertical, 14)
            .background(ChirpColor.accent, in: Capsule())
            .foregroundStyle(.white)
        }
        .buttonStyle(.plain)
    }
}

/// Section header used across feature screens for visual consistency.
struct ChirpSectionHeader: View {
    let title: String
    var body: some View {
        Text(title.uppercased())
            .font(.caption.weight(.semibold))
            .foregroundStyle(ChirpColor.textTertiary)
            .tracking(0.8)
    }
}

/// Standard empty / loading placeholder so every screen "feels finished"
/// rather than blank while the kernel warms up (D1).
struct ChirpPlaceholder: View {
    let systemImage: String
    let title: String
    var subtitle: String? = nil
    var body: some View {
        VStack(spacing: ChirpSpace.m) {
            Image(systemName: systemImage)
                .font(.system(size: 44, weight: .light))
                .foregroundStyle(ChirpColor.accent)
            Text(title).font(ChirpFont.title)
            if let subtitle {
                Text(subtitle).font(ChirpFont.callout)
                    .foregroundStyle(ChirpColor.textSecondary)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(ChirpSpace.xl)
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
