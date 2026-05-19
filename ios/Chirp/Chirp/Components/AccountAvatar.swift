import SwiftUI

// ── AccountAvatar ─────────────────────────────────────────────────────────
// Derives `initials` and `colorHex` from `AccountSummary` (view-layer only —
// kernel does not supply avatar metadata for accounts at D1). This is pure
// display formatting, not business logic.

extension AccountSummary {
    /// Two-character initials: first char of each word in displayName,
    /// falling back to the first two chars of npub.
    var avatarInitials: String {
        let words = displayName
            .split(separator: " ")
            .prefix(2)
            .compactMap { $0.first.map { String($0).uppercased() } }
        if !words.isEmpty { return words.joined() }
        // npub starts with "npub1…", skip the prefix and take two chars.
        let stripped = npub.hasPrefix("npub1") ? String(npub.dropFirst(5)) : npub
        return String(stripped.prefix(2)).uppercased()
    }

    /// Deterministic color hex derived from the account `id` (a stable
    /// string from the kernel). Picks one of six harmonious accent hues.
    var avatarColorHex: String {
        let palette: [String] = [
            "8566F5",  // violet (matches ChirpColor.accent)
            "34C68D",  // teal-green
            "F5A623",  // amber
            "E8445A",  // rose
            "3B82F6",  // blue
            "A855F7",  // purple
        ]
        let hash = id.unicodeScalars.reduce(0) { ($0 &* 31) &+ Int($1.value) }
        return palette[abs(hash) % palette.count]
    }
}
