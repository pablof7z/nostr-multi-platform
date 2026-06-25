package org.nmp.android.ui.embed

import org.junit.Assert.assertEquals
import org.nmp.android.model.ArticleProjectionEntry
import org.nmp.android.model.EmbedKindProjectionEntry
import org.nmp.android.model.HighlightProjectionEntry
import org.nmp.android.model.ProfileProjectionEntry
import org.nmp.android.model.ShortNoteProjectionEntry
import org.nmp.android.model.UnknownProjectionEntry
import org.junit.Test

/**
 * Kind-dispatch contract for [NostrKindRegistry] (#984 / F-CR-07). Verifies that
 * the registry selects the correct per-kind renderer [NostrKindRegistry.Slot]
 * from an already-typed [EmbedKindProjectionEntry] — exactly one variant
 * populated — mirroring the iOS `NostrKindRegistry.resolve` switch.
 *
 * This is a pure JVM test (no Compose runtime): the rendering composable is a
 * thin switch over [NostrKindRegistry.slotFor], so testing the slot selection
 * fully covers the dispatch decision.
 */
class NostrKindRegistryTest {

    @Test
    fun shortNoteDispatchesToShortNoteSlot() {
        val projection = EmbedKindProjectionEntry(
            shortNote = ShortNoteProjectionEntry(id = "n1", authorPubkey = "aa".repeat(32)),
        )
        assertEquals(NostrKindRegistry.Slot.SHORT_NOTE, NostrKindRegistry.slotFor(projection))
    }

    @Test
    fun articleDispatchesToArticleSlot() {
        val projection = EmbedKindProjectionEntry(
            article = ArticleProjectionEntry(id = "a1", authorPubkey = "bb".repeat(32)),
        )
        assertEquals(NostrKindRegistry.Slot.ARTICLE, NostrKindRegistry.slotFor(projection))
    }

    @Test
    fun highlightDispatchesToHighlightSlot() {
        val projection = EmbedKindProjectionEntry(
            highlight = HighlightProjectionEntry(id = "h1", authorPubkey = "cc".repeat(32)),
        )
        assertEquals(NostrKindRegistry.Slot.HIGHLIGHT, NostrKindRegistry.slotFor(projection))
    }

    @Test
    fun profileDispatchesToProfileSlot() {
        val projection = EmbedKindProjectionEntry(
            profile = ProfileProjectionEntry(pubkey = "dd".repeat(32)),
        )
        assertEquals(NostrKindRegistry.Slot.PROFILE, NostrKindRegistry.slotFor(projection))
    }

    @Test
    fun unknownDispatchesToUnknownSlot() {
        val projection = EmbedKindProjectionEntry(
            unknown = UnknownProjectionEntry(kind = 31337, authorPubkey = "ee".repeat(32)),
        )
        assertEquals(NostrKindRegistry.Slot.UNKNOWN, NostrKindRegistry.slotFor(projection))
    }

    @Test
    fun allNullProjectionFallsThroughToNone() {
        assertEquals(NostrKindRegistry.Slot.NONE, NostrKindRegistry.slotFor(EmbedKindProjectionEntry()))
    }

    @Test
    fun shortNotePrecedesOtherVariantsWhenMultipleSomehowSet() {
        // Defensive: the wire model guarantees exactly one variant, but if a
        // malformed buffer ever set two, dispatch must be deterministic.
        val projection = EmbedKindProjectionEntry(
            shortNote = ShortNoteProjectionEntry(id = "n1", authorPubkey = "aa".repeat(32)),
            article = ArticleProjectionEntry(id = "a1", authorPubkey = "bb".repeat(32)),
        )
        assertEquals(NostrKindRegistry.Slot.SHORT_NOTE, NostrKindRegistry.slotFor(projection))
    }
}
