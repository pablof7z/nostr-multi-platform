// Compose mirror of the SwiftUI `NostrRelativeTime` and the Rust
// `nmp_core::display::format_ago_secs`. Presentation-layer helper: the kernel
// projection carries the raw `created_at` (Unix seconds) and the doctrine is
// explicit that the presentation layer formats it (see nmp-core
// kernel/types.rs: "Event created_at (Unix seconds). Presentation layer
// formats…"). The bucketing mirrors the Rust helper exactly ("3s ago" /
// "12m ago" / "5h ago" / "2d ago", and "now" for a zero or future stamp) so
// every surface — TUI, iOS, Android — speaks the same dialect.

package org.nmp.gallery.registry

public object NostrRelativeTime {
    /** `nowSecs` is injected so callers can stay deterministic in tests; it
     *  defaults to the wall clock. */
    public fun ago(
        thenSecs: Long,
        nowSecs: Long = System.currentTimeMillis() / 1_000L,
    ): String {
        if (thenSecs <= 0L || nowSecs <= thenSecs) return "now"
        val diff = nowSecs - thenSecs
        return when {
            diff < 60L -> "${diff}s ago"
            diff < 3_600L -> "${diff / 60L}m ago"
            diff < 86_400L -> "${diff / 3_600L}h ago"
            else -> "${diff / 86_400L}d ago"
        }
    }
}
