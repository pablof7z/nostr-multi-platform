package org.nmp.gallery.model

/**
 * Kotlin render traversal state — PD-015 depth + cycle guard.
 *
 * PROJECTION-GAP NOTE (#2): `nmp_content::RenderContext` is non-serde with
 * no FFI projection. The STAGE 2 bundle carries resolution facts only; the
 * depth budget + `visited`-set traversal state is a render-time concern that
 * travels with the renderer. The key passed to this type is the opaque
 * Rust-emitted cycle key; Kotlin must not derive it from Nostr
 * kind/tag/content.
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
