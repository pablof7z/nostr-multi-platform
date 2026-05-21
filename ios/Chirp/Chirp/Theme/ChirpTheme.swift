import SwiftUI

enum ChirpColor {
    static let accent = Color.accentColor
    static let accentSoft = Color.accentColor.opacity(0.12)
    static let bg = Color(.systemBackground)
    static let surface = Color(.secondarySystemBackground)
    static let hairline = Color(.separator)
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
    static let radius: CGFloat = 20
    static let radiusSmall: CGFloat = 12
}

struct ChirpBackdrop: View {
    var body: some View {
        Rectangle().fill(.background)
        .ignoresSafeArea()
    }
}

extension View {
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
        .buttonStyle(.borderedProminent)
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

/// Circular character-count progress ring for the compose sheet.
/// Mimics Twitter/X: ring fills as limit approaches; turns red over limit.
struct ComposeProgressRing: View {
    let used: Int
    let limit: Int

    private var fraction: Double { min(Double(used) / Double(limit), 1.0) }
    private var isOver: Bool { used > limit }
    private var remaining: Int { limit - used }
    private var showNumber: Bool { remaining <= 20 }

    private var ringColor: Color {
        if isOver { return .red }
        if remaining <= 20 { return .orange }
        return .accentColor
    }

    var body: some View {
        ZStack {
            Circle()
                .stroke(Color(.systemFill), lineWidth: 2.5)
            Circle()
                .trim(from: 0, to: fraction)
                .stroke(ringColor, style: StrokeStyle(lineWidth: 2.5, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .animation(.easeInOut(duration: 0.15), value: fraction)
            if showNumber {
                Text("\(remaining)")
                    .font(.system(size: 11, weight: .semibold, design: .rounded))
                    .foregroundStyle(isOver ? .red : .secondary)
                    .minimumScaleFactor(0.7)
            }
        }
        .frame(width: 24, height: 24)
    }
}
