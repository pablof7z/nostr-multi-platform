package org.nmp.android.ui

internal fun shortPubkey(pubkey: String): String = if (pubkey.length >= 16) {
    "${pubkey.take(8)}...${pubkey.takeLast(8)}"
} else {
    pubkey
}

/**
 * Format Unix seconds as relative time (e.g., "5m ago", "2h ago").
 * Mirrors the iOS presentation-layer concern (D8).
 */
internal fun formatRelativeTime(createdAtSeconds: Long): String {
    val deltaSecs = (System.currentTimeMillis() / 1000) - createdAtSeconds
    return when {
        deltaSecs < 60 -> "${deltaSecs}s ago"
        deltaSecs < 3_600 -> "${deltaSecs / 60}m ago"
        deltaSecs < 86_400 -> "${deltaSecs / 3_600}h ago"
        else -> "${deltaSecs / 86_400}d ago"
    }
}
