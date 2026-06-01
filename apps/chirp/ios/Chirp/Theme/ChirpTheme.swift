import SwiftUI

enum ChirpColor {
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
    static let messageOutgoingBackground = accent
    static let messageIncomingBackground = surface
    static let messageOutgoingForeground = Color(.white)
    static let messageIncomingForeground = textPrimary
    static let mediaBackdrop = Color(.black)
    static let mediaForeground = Color(.white)
    static let mediaSecondaryForeground = Color(.systemGray3).opacity(0.7)
    static let inverseForeground = Color(.white)
    static let emphasisForeground = Color(.white)
    static let errorBannerBackground = danger.opacity(0.9)

    /// Deterministic avatar gradient from a hex color string the kernel
    /// supplies (`avatarColor`). Falls back to the accent.
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

/// Fades in when it appears — used by ChirpAvatar to soften image loads.
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
/// plain placeholder fill + initials (D1: never blank).
/// Manages NostrProfileHost claim/release lifecycle for the given pubkey.
struct ChirpAvatar: View {
    @Environment(\.nostrProfileHost) private var profileHost

    let pubkey: String
    let url: String?
    let initials: String
    let colorHex: String
    var size: CGFloat = 44

    @State private var generatedConsumerID: String
    @State private var claimedPubkey: String?

    init(
        pubkey: String,
        url: String? = nil,
        initials: String,
        colorHex: String,
        size: CGFloat = 44
    ) {
        self.pubkey = pubkey
        self.url = url
        self.initials = initials
        self.colorHex = colorHex
        self.size = size
        self._generatedConsumerID = State(initialValue: "chirp-avatar.\(UUID().uuidString)")
        self._claimedPubkey = State(initialValue: nil)
    }

    var body: some View {
        let resolvedUrl = url ?? profileHost?.profile(forPubkey: pubkey)?.pictureUrl

        ZStack {
            Circle().fill(ChirpColor.avatar(from: colorHex))
            if let resolvedUrl, let u = URL(string: resolvedUrl) {
                AsyncImage(url: u) { phase in
                    if let img = phase.image {
                        FadingImage(image: img)
                    }
                }
            }
            if resolvedUrl == nil || resolvedUrl?.isEmpty == true {
                Text(initials)
                    .font(.system(size: size * 0.4, weight: .semibold))
                    .foregroundStyle(.primary)
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .overlay(Circle().stroke(ChirpColor.hairlineSoft, lineWidth: 0.5))
        .task(id: pubkey) {
            await MainActor.run {
                if let claimedPubkey, claimedPubkey != pubkey {
                    profileHost?.releaseProfile(
                        pubkey: claimedPubkey,
                        consumerID: generatedConsumerID
                    )
                }
                claimedPubkey = pubkey
                profileHost?.claimProfile(pubkey: pubkey, consumerID: generatedConsumerID)
            }
        }
        .onDisappear {
            if let claimedPubkey {
                profileHost?.releaseProfile(pubkey: claimedPubkey, consumerID: generatedConsumerID)
                self.claimedPubkey = nil
            }
        }
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
/// Ring fills as the limit approaches and switches to danger when over limit.
struct ComposeProgressRing: View {
    let used: Int
    let limit: Int

    private var fraction: Double { min(Double(used) / Double(limit), 1.0) }
    private var isOver: Bool { used > limit }
    private var remaining: Int { limit - used }
    private var showNumber: Bool { remaining <= 20 }

    private var ringColor: Color {
        if isOver { return ChirpColor.danger }
        if remaining <= 20 { return ChirpColor.warning }
        return ChirpColor.accent
    }

    var body: some View {
        ZStack {
            Circle()
                .stroke(ChirpColor.systemFill, lineWidth: 2.5)
            Circle()
                .trim(from: 0, to: fraction)
                .stroke(ringColor, style: StrokeStyle(lineWidth: 2.5, lineCap: .round))
                .rotationEffect(.degrees(-90))
                .animation(.easeInOut(duration: 0.15), value: fraction)
            if showNumber {
                Text("\(remaining)")
                    .font(.system(size: 11, weight: .semibold, design: .rounded))
                    .foregroundStyle(isOver ? ChirpColor.danger : ChirpColor.textSecondary)
                    .minimumScaleFactor(0.7)
            }
        }
        .frame(width: 24, height: 24)
    }
}

/// Fades a view in from opacity 0 to 1 on first appear.
/// Apply with `.fadeIn()` on any view that should animate into existence.
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
