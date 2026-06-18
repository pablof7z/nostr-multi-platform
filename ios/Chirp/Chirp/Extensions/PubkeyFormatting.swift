// ─────────────────────────────────────────────────────────────────────────────
// Presentation-layer formatters for raw protocol data.
//
// ADR-0032 (raw-data projection doctrine) — the Rust kernel emits raw protocol
// data (hex pubkeys, Unix timestamps, optional metadata fields). Presentation
// layers are responsible for formatting that data for display. This file is
// where Chirp owns those decisions, instead of pre-formatted strings flowing
// across the FFI boundary.
//
// All helpers here are pure functions of their input — no state, no
// allocations beyond the returned string, no I/O. Safe to call from view-body
// recomputation.
// ─────────────────────────────────────────────────────────────────────────────

import Foundation
import SwiftUI

extension String {
    /// First 8 + last 8 hex characters joined by a horizontal-ellipsis. The
    /// canonical Chirp abbreviation for a 64-char hex pubkey — used in row
    /// headers, reply banners, and any other site that needs a compact
    /// pubkey label. Strings shorter than 16 chars are returned verbatim
    /// (no truncation past the meaningful prefix/suffix split).
    var shortHex: String {
        guard count > 16 else { return self }
        return "\(prefix(8))…\(suffix(8))"
    }

    /// Deterministic SwiftUI `Color` derived from a hex pubkey via djb2 →
    /// hue. The same pubkey always produces the same color. Saturation and
    /// brightness are fixed so the palette stays visually coherent across
    /// rows. Mirrors the avatar-tint algorithm previously computed in Rust
    /// projections (ADR-0032 moved it back to the presentation layer).
    var pubkeyColor: Color {
        var hash: UInt32 = 5381
        for byte in utf8 { hash = hash &* 33 &+ UInt32(byte) }
        return Color(
            hue: Double(hash % 360) / 360.0,
            saturation: 0.6,
            brightness: 0.85
        )
    }

    /// Deterministic 6-hex avatar tint derived from this string, with the
    /// same djb2 hash function as `pubkeyColor` so the two stay in sync.
    /// Returned without a leading `#` — compatible with
    /// `ChirpColor.avatar(from:)` and the Rust-emitted hex format previously
    /// shipped on `*_color_hex` snapshot fields. Mid-range value/saturation
    /// keeps text legible on top of the tile.
    var pubkeyColorHex: String {
        var hash: UInt32 = 5381
        for byte in utf8 { hash = hash &* 33 &+ UInt32(byte) }
        // Keep all three channels in the 0x40…0xBF band so the tile reads as
        // a muted mid-tone rather than a saturated primary.
        let r = UInt32(0x40) + (hash >> 16) & 0x7F
        let g = UInt32(0x40) + (hash >> 8) & 0x7F
        let b = UInt32(0x40) + hash & 0x7F
        return String(format: "%02X%02X%02X", r, g, b)
    }

    /// Avatar-tile initials: up to two uppercase letters drawn from the
    /// first two whitespace-separated words of a display name. Falls back
    /// to the first two characters of the string itself (uppercased) when
    /// there is only one word; falls back to `".."` for empty / single-char
    /// strings. Suitable for both display names and raw hex pubkeys.
    var displayInitials: String {
        let words = split(separator: " ").prefix(2)
        if words.count >= 2 {
            return words.compactMap { $0.first }.map { String($0).uppercased() }.joined()
        }
        return count >= 2 ? String(prefix(2)).uppercased() : ".."
    }
}

// MARK: - Timestamp formatting

/// Thread-local formatter storage for Unix-seconds-since-epoch relative-time
/// labels ("3 seconds ago", "5 minutes ago"). `RelativeDateTimeFormatter` is
/// not `Sendable`, so we cannot share a single instance across actors under
/// Swift 6 strict concurrency. Each thread gets its own instance — created
/// once, reused for the lifetime of that thread.
private func relativeFormatter() -> RelativeDateTimeFormatter {
    let key = "ChirpRelativeDateTimeFormatter"
    if let existing = Thread.current.threadDictionary[key] as? RelativeDateTimeFormatter {
        return existing
    }
    let f = RelativeDateTimeFormatter()
    f.unitsStyle = .abbreviated
    Thread.current.threadDictionary[key] = f
    return f
}

extension UInt64 {
    /// Render this value (interpreted as Unix seconds since epoch) as a
    /// short relative-time label ("3s ago", "5m ago"). Shells format the
    /// raw `created_at` Unix seconds with their own locale (ADR-0032).
    var relativeTimeFromUnixSeconds: String {
        let date = Date(timeIntervalSince1970: TimeInterval(self))
        return relativeFormatter().localizedString(for: date, relativeTo: Date())
    }
}
