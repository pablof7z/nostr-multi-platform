import Foundation

/// Abbreviated `"X ago"` relative-time label for a Unix-seconds timestamp.
///
/// Presentation-layer helper. The kernel projection carries the raw
/// `created_at` (Unix seconds) and the doctrine is explicit that the
/// presentation layer formats it (see `nmp-core` `kernel/types.rs`:
/// "Event `created_at` (Unix seconds). Presentation layer formats…").
///
/// The bucketing mirrors the Rust `nmp_core::display::format_ago_secs`
/// helper exactly (`"3s ago"` / `"12m ago"` / `"5h ago"` / `"2d ago"`,
/// and `"now"` for a zero or future stamp) so every surface — TUI, iOS,
/// Android — speaks the same dialect.
public enum NostrRelativeTime {
    /// `nowSecs` is injected so callers can stay deterministic in tests;
    /// it defaults to the wall clock.
    public static func ago(
        _ thenSecs: UInt64,
        now nowSecs: UInt64 = UInt64(max(0, Date().timeIntervalSince1970))
    ) -> String {
        guard thenSecs != 0, nowSecs > thenSecs else { return "now" }
        let diff = nowSecs - thenSecs
        if diff < 60 { return "\(diff)s ago" }
        if diff < 3_600 { return "\(diff / 60)m ago" }
        if diff < 86_400 { return "\(diff / 3_600)h ago" }
        return "\(diff / 86_400)d ago"
    }
}
