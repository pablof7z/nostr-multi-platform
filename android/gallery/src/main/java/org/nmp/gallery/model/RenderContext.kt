package org.nmp.gallery.model

/**
 * Kotlin port of Swift `RenderContext` — PD-015 depth + cycle guard.
 *
 * PROJECTION-GAP NOTE (#2): `nmp_content::RenderContext` is non-serde with
 * no FFI projection. The STAGE 2 bundle carries resolution facts only; the
 * depth budget + `visited`-set cycle guard is a render-time concern that
 * travels with the renderer's traversal. This is the faithful mirror of
 * `RenderContext::should_collapse`:
 *
 *   depth >= max_depth (default 4)  OR  visited.contains(into)
 */
data class RenderContext(
    val depth: Int = 0,
    val maxDepth: Int = 4,
    val visited: Set<String> = emptySet(),
) {
    /** Returns (collapse, reason). reason is "cycle" or "depth" when true. */
    fun shouldCollapse(key: String): Pair<Boolean, String?> {
        if (visited.contains(key)) return true to "cycle"
        if (depth >= maxDepth) return true to "depth"
        return false to null
    }

    fun descend(key: String): RenderContext =
        copy(depth = depth + 1, visited = visited + key)
}

/**
 * Stable visited/cycle key for a resolved embed event, mirroring how the
 * Rust renderer keys `RenderContext.visited`. Addressable events
 * (replaceable param. + relay list metadata kind 10002) key by
 * `kind:pubkey:d`; regular events key by id hex.
 */
fun visitedKey(ev: SignedEventJson): String {
    val addressable = ev.kind in 30_000 until 40_000 || ev.kind == 10_002
    return if (addressable) {
        val d = ev.tags.firstOrNull { it.firstOrNull() == "d" }?.getOrNull(1) ?: ""
        "${ev.kind}:${ev.pubkey}:$d"
    } else {
        ev.id
    }
}
