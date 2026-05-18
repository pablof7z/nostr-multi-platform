import Foundation

/// Formatting helpers for player time codes (`hh:mm:ss` / `mm:ss`).
///
/// Centralised so every subview renders timestamps identically — matters for
/// the brief's "tabular numerals, never jitters" rule.
enum PlayerTimeFormat {

    /// Renders `seconds` as `mm:ss` for episodes under one hour, `h:mm:ss`
    /// otherwise. Negative or non-finite inputs clamp to `0:00`.
    static func clock(_ seconds: TimeInterval) -> String {
        guard seconds.isFinite, seconds >= 0 else { return "0:00" }
        let total = Int(seconds.rounded(.down))
        let hours = total / 3600
        let minutes = (total % 3600) / 60
        let secs = total % 60
        if hours > 0 {
            return String(format: "%d:%02d:%02d", hours, minutes, secs)
        }
        return String(format: "%d:%02d", minutes, secs)
    }

    /// Combined `current / duration` — used by the waveform footer and mini-bar.
    static func progress(_ current: TimeInterval, _ duration: TimeInterval) -> String {
        "\(clock(current)) / \(clock(duration))"
    }

    /// Remaining time with a leading `-`, e.g. `-12:34`. Returns `""` when
    /// `duration` is 0 so callers can fall back gracefully before the asset loads.
    static func remaining(_ current: TimeInterval, duration: TimeInterval) -> String {
        guard duration > 0 else { return "" }
        let rem = max(0, duration - current)
        return "-\(clock(rem))"
    }
}
