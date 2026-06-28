import SwiftUI
import Foundation

/// Circular avatar for a Nostr pubkey. Shows the profile picture when the
/// host projection has it; falls back to a deterministic 5×5 symmetric
/// identicon derived from `pubkey` (same algorithm as `content-core`).
///
/// Replace `AsyncImage` with your own image cache (Kingfisher, Nuke, etc.)
/// if you already have one — the identicon fallback is self-contained.
///
/// Depends on `swiftui/user-avatar` for `ProfileWire` and `NostrProfileHost`.
public struct NostrAvatar: View {
    @Environment(\.nostrProfileHost) private var profileHost

    public let pubkey: String
    public let pictureUrl: URL?
    public let size: CGFloat
    public let consumerID: String?
    @State private var generatedConsumerID: String
    @State private var claimedPubkey: String?

    public init(
        pubkey: String,
        pictureUrl: URL? = nil,
        size: CGFloat = 40,
        consumerID: String? = nil
    ) {
        self.pubkey = pubkey
        self.pictureUrl = pictureUrl
        self.size = size
        self.consumerID = consumerID
        self._generatedConsumerID = State(
            initialValue: consumerID ?? "nostr-avatar.\(UUID().uuidString)"
        )
        self._claimedPubkey = State(initialValue: nil)
    }

    public init(profile: ProfileWire, size: CGFloat = 40) {
        self.pubkey = profile.pubkey
        self.pictureUrl = profile.avatarURL
        self.size = size
        self.consumerID = nil
        self._generatedConsumerID = State(
            initialValue: "nostr-avatar.static.\(UUID().uuidString)"
        )
        self._claimedPubkey = State(initialValue: nil)
    }

    public var body: some View {
        let url = pictureUrl ?? profileHost?.profile(forPubkey: pubkey)?.avatarURL

        Group {
            if let url {
                AsyncImage(url: url) { phase in
                    switch phase {
                    case .success(let image):
                        image.resizable().scaledToFill()
                    default:
                        identicon
                    }
                }
            } else {
                identicon
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .accessibilityHidden(true)
        .task(id: pubkey) {
            await MainActor.run {
                if let claimedPubkey, claimedPubkey != pubkey {
                    profileHost?.releaseProfileRef(
                        pubkey: claimedPubkey,
                        consumerID: generatedConsumerID
                    )
                }
                claimedPubkey = pubkey
                profileHost?.resolveProfileRef(pubkey: pubkey, consumerID: generatedConsumerID)
            }
        }
        .onDisappear {
            if let claimedPubkey {
                profileHost?.releaseProfileRef(pubkey: claimedPubkey, consumerID: generatedConsumerID)
                self.claimedPubkey = nil
            }
        }
    }

    private var identicon: some View {
        NostrIdenticon.identiconView(forPubkey: pubkey, size: size)
    }
}

// MARK: - Identicon

/// 5×5 symmetric pixel-grid identicon for a hex pubkey. Algorithm:
///   1. djb2 hash of the pubkey's UTF-8 bytes (32-bit unsigned, wrapping).
///   2. Color: `hue = hash % 360 / 360`, HSB with S=0.55, B=0.75.
///   3. Cells: lower 15 bits of hash encode a 5-row × 3-col left half;
///      columns 3–4 mirror columns 1–0, yielding a horizontally symmetric grid.
///
/// This algorithm is byte-identical to the Kotlin implementation in
/// `compose/user-avatar/NostrAvatar.kt` and
/// `compose/content-core/ContentTreeWire.kt`. Same pubkey → same grid and
/// color on every platform.
public enum NostrIdenticon {
    /// Returns a stable `Color` derived from a hex pubkey via djb2 → HSB.
    public static func color(forPubkey pubkey: String) -> Color {
        let hue = Double(djb2(pubkey) % 360) / 360.0
        return Color(hue: hue, saturation: 0.55, brightness: 0.75)
    }

    /// Returns the 5×5 symmetric fill pattern for a pubkey. Cell at
    /// `(row, col)` is filled iff `cells[row][col]` is `true`. Exposed for
    /// snapshot tests and apps that want to render the same pattern in a
    /// non-SwiftUI surface (e.g. `Canvas` exports).
    public static func cells(forPubkey pubkey: String) -> [[Bool]] {
        let hash = djb2(pubkey)
        var rows: [[Bool]] = []
        for row in 0..<5 {
            var line: [Bool] = Array(repeating: false, count: 5)
            for col in 0..<3 {
                let bit = row * 3 + col
                let filled = (hash >> UInt32(bit)) & 1 == 1
                line[col] = filled
                // Mirror columns 0 and 1 to columns 4 and 3 respectively;
                // column 2 is the centre and stays unchanged.
                if col < 2 {
                    line[4 - col] = filled
                }
            }
            rows.append(line)
        }
        return rows
    }

    /// Returns a SwiftUI view rendering the 5×5 identicon grid at `size`×`size`
    /// points. The view is NOT clipped — callers apply `.clipShape(Circle())`
    /// or any other clip as needed; `NostrAvatar` does this automatically.
    public static func identiconView(forPubkey pubkey: String, size: CGFloat = 40) -> some View {
        IdenticonGridView(pubkey: pubkey, size: size)
    }

    private static func djb2(_ value: String) -> UInt32 {
        var hash: UInt32 = 5381
        for byte in value.utf8 {
            hash = (hash &* 33) &+ UInt32(byte)
        }
        return hash
    }
}

/// Internal renderer for `NostrIdenticon.identiconView`. Uses `Canvas` so the
/// whole identicon collapses to a single drawing pass — cheap enough to live
/// inside a `ForEach` row without measurable layout cost.
private struct IdenticonGridView: View {
    let pubkey: String
    let size: CGFloat

    var body: some View {
        let cells = NostrIdenticon.cells(forPubkey: pubkey)
        let color = NostrIdenticon.color(forPubkey: pubkey)
        return Canvas { context, canvasSize in
            let gridCount: CGFloat = 5
            // 1pt spacing between cells (visually crisp at any size >= 20pt).
            let spacing: CGFloat = 1
            let totalSpacing = spacing * (gridCount - 1)
            let cell = max(0, (min(canvasSize.width, canvasSize.height) - totalSpacing) / gridCount)
            for row in 0..<5 {
                for col in 0..<5 where cells[row][col] {
                    let x = CGFloat(col) * (cell + spacing)
                    let y = CGFloat(row) * (cell + spacing)
                    let rect = CGRect(x: x, y: y, width: cell, height: cell)
                    context.fill(Path(rect), with: .color(color))
                }
            }
        }
        .background(color.opacity(0.15))
        .frame(width: size, height: size)
    }
}
