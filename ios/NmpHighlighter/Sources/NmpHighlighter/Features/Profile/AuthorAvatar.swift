import Kingfisher
import SwiftUI

/// Circular avatar with a deterministic gradient fallback when no image is
/// available — same approach as the web's `User.Avatar`. The gradient is
/// derived from the pubkey so the same person always gets the same fallback.
struct AuthorAvatar: View {
    let pubkey: String
    let pictureURL: String
    let displayInitial: String
    let size: CGFloat
    let ringWidth: CGFloat

    init(
        pubkey: String,
        pictureURL: String = "",
        displayInitial: String = "",
        size: CGFloat = 40,
        ringWidth: CGFloat = 0
    ) {
        self.pubkey = pubkey
        self.pictureURL = pictureURL
        self.displayInitial = displayInitial
        self.size = size
        self.ringWidth = ringWidth
    }

    var body: some View {
        ZStack {
            gradient
            if let initial = initialCharacter {
                Text(initial)
                    .font(.system(size: size * 0.42, weight: .semibold, design: .default))
                    .foregroundStyle(.white.opacity(0.92))
            }

            if let url = avatarURL {
                KFImage(url)
                    .placeholder { Color.clear }
                    .fade(duration: 0.15)
                    .resizable()
                    .scaledToFill()
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .overlay(
            Circle()
                .strokeBorder(Color.white, lineWidth: ringWidth)
        )
        .accessibilityHidden(true)
    }

    private var avatarURL: URL? {
        guard !pictureURL.isEmpty else { return nil }
        return URL(string: pictureURL)
    }

    private var initialCharacter: String? {
        let source = displayInitial.isEmpty ? pubkey : displayInitial
        return source.trimmingCharacters(in: .whitespaces).first.map { String($0).uppercased() }
    }

    private var gradient: LinearGradient {
        let hashed = pubkey.unicodeScalars.reduce(UInt32(0)) { acc, scalar in
            acc &+ scalar.value
        }
        let hueA = Double(hashed % 360) / 360.0
        let hueB = Double((hashed &* 37 + 120) % 360) / 360.0
        return LinearGradient(
            colors: [
                Color(hue: hueA, saturation: 0.55, brightness: 0.72),
                Color(hue: hueB, saturation: 0.65, brightness: 0.55)
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
    }
}

#Preview {
    HStack(spacing: 16) {
        AuthorAvatar(pubkey: "abc123", displayInitial: "A", size: 56)
        AuthorAvatar(
            pubkey: "def456",
            pictureURL: "https://nostr.build/i/nostr.build_nonexistent.png",
            displayInitial: "B",
            size: 88,
            ringWidth: 4
        )
    }
    .padding()
}
