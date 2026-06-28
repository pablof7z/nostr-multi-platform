package org.nmp.android.ui.embed

import androidx.compose.runtime.Composable
import org.nmp.android.model.EmbedKindProjectionEntry

/**
 * Single source of truth for kind → Compose renderer dispatch on Android
 * (#984 / F-CR-07). Android peer of iOS
 * `apps/chirp/ios/.../NostrContent/NostrKindRegistry.swift` and the TUI
 * `NostrKindRegistry`.
 *
 * THIN-SHELL (D0 / chirp-thin-shell): this performs NO protocol parsing and NO
 * kind *classification*. The kernel already resolved each embedded event into a
 * typed [EmbedKindProjectionEntry] (exactly one variant non-null); this object
 * only picks the matching @Composable from that already-typed variant. Adding a
 * new embed kind means adding a variant to the kernel's resolver + a Compose
 * case here — never a `when (kind)` over a raw integer in Kotlin.
 *
 * The per-kind composables live in sibling files
 * ([ShortNoteEmbedView]/[ArticleEmbedView]/[HighlightEmbedView]/
 * [ProfileEmbedView]/[UnknownEmbedView]) to keep every hand-authored file under
 * the AGENTS.md size caps.
 */
object NostrKindRegistry {
    /**
     * The selected per-kind renderer slot. Pure value (no Compose), so the
     * dispatch decision is unit-testable on the JVM without a Compose runtime.
     * [NONE] is the defensive case for a malformed all-null entry.
     */
    enum class Slot { SHORT_NOTE, ARTICLE, HIGHLIGHT, PROFILE, UNKNOWN, NONE }

    /**
     * Resolve which [Slot] a typed projection dispatches to. The wire model
     * populates exactly one variant; this only reads which one is present — no
     * protocol parsing, no kind *classification* (the kernel already did that).
     */
    fun slotFor(projection: EmbedKindProjectionEntry): Slot = when {
        projection.shortNote != null -> Slot.SHORT_NOTE
        projection.article != null -> Slot.ARTICLE
        projection.highlight != null -> Slot.HIGHLIGHT
        projection.profile != null -> Slot.PROFILE
        projection.unknown != null -> Slot.UNKNOWN
        else -> Slot.NONE
    }

    /**
     * Render the resolved projection by dispatching to the per-kind composable.
     * Selection is delegated to [slotFor] so the rendering and the decision stay
     * a single source of truth (the composable is then a thin switch).
     */
    @Composable
    fun Render(projection: EmbedKindProjectionEntry) {
        when (slotFor(projection)) {
            Slot.SHORT_NOTE -> ShortNoteEmbedView(projection.shortNote!!)
            Slot.ARTICLE -> ArticleEmbedView(projection.article!!)
            Slot.HIGHLIGHT -> HighlightEmbedView(projection.highlight!!)
            Slot.PROFILE -> ProfileEmbedView(projection.profile!!)
            Slot.UNKNOWN -> UnknownEmbedView(projection.unknown!!)
            Slot.NONE -> Unit
        }
    }
}
