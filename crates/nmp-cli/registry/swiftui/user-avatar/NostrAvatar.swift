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

/// Deterministic identicon for a hex pubkey. Apps that don't supply an avatar
/// URL can render the geometric `identiconView(forPubkey:)` (a 5×5 symmetric
/// pixel grid in the GitHub-style — same input always produces the same
/// pattern). `color(forPubkey:)` is still exposed so apps can theme borders
/// or backgrounds with the same palette. Ported from Chirp's djb2-based
/// palette so installed apps stay visually consistent.
public enum NostrIdenticon {
    /// Returns a stable `Color` derived from a hex pubkey (or any string).
    /// Uses the djb2 hash mapped to HSL with fixed S/L for legibility.
    public static func color(forPubkey pubkey: String) -> Color {
        let hue = Double(djb2(pubkey) % 360) / 360.0
        return Color(hue: hue, saturation: 0.55, brightness: 0.75)
    }

    /// Single-character monogram derived from a hex pubkey (first char,
    /// uppercased). Retained for backward compatibility with v0.1 apps that
    /// still render a text initial overlay on top of `color(forPubkey:)`;
    /// new code should prefer `identiconView(forPubkey:)`.
    public static func initials(forPubkey pubkey: String) -> String {
        let trimmed = pubkey.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty { return "?" }
        return String(trimmed.prefix(1)).uppercased()
    }

    /// 5×5 symmetric pixel-grid identicon, GitHub-style. Deterministic from
    /// the pubkey via djb2: the lower 15 bits of the hash decide which of
    /// the 15 left-half cells (3 columns × 5 rows) are filled; the right
    /// half is a horizontal mirror so the result is visually symmetric.
    ///
    /// The cell colour comes from `color(forPubkey:)`; empty cells use the
    /// same hue at 15% opacity so the overall tile reads as a tinted patch
    /// rather than blank.
    public static func identiconView(forPubkey pubkey: String, size: CGFloat = 40) -> some View {
        IdenticonGridView(pubkey: pubkey, size: size)
    }

    /// Returns the 5×5 symmetric fill pattern for a pubkey. Cell at
    /// `(row, col)` is filled iff `cells[row][col]` is true. Exposed for
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
            // 1pt spacing between cells (visually crisp at any size ≥ 20pt).
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
