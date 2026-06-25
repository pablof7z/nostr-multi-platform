package org.nmp.android.ui.embed

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.nmp.android.model.EmbedEnvelopeEntry
import org.nmp.android.model.EmbedKindProjectionEntry
import org.nmp.android.model.ShortNoteProjectionEntry
import org.junit.Test

/**
 * Claim-lifecycle and render-state contract for the [EmbeddedEvent] embed slot
 * (#984 / F-CR-07). These are the pure surfaces the composable delegates to, so
 * the wiring is verified on the JVM without a Compose runtime:
 *  - [EventClaimHandle] is what the `DisposableEffect` enters/disposes.
 *  - [embedRenderState] is what the render `when` switches on.
 */
class EmbeddedEventTest {

    /** Records every claim/release call so the contract can be asserted. */
    private class RecordingClaimer {
        val calls = mutableListOf<Triple<String, String, Boolean>>()
        val claimer: EventClaimer = { uri, consumerId, claim ->
            calls.add(Triple(uri, consumerId, claim))
        }
    }

    @Test
    fun enterClaimsThenDisposeReleasesSameUriAndConsumer() {
        val rec = RecordingClaimer()
        val handle = EventClaimHandle(rec.claimer, uri = "nostr:nevent1xyz", consumerId = "embed-ref-abc")

        handle.enter()
        handle.dispose()

        assertEquals(
            listOf(
                Triple("nostr:nevent1xyz", "embed-ref-abc", true),
                Triple("nostr:nevent1xyz", "embed-ref-abc", false),
            ),
            rec.calls,
        )
    }

    @Test
    fun claimUsesVerbatimUriNotPrimaryId() {
        // The kernel resolves the verbatim EventRef URI; the primaryId is only
        // the snapshot lookup key. The claim must pass the URI.
        val rec = RecordingClaimer()
        EventClaimHandle(rec.claimer, uri = "nostr:naddr1aaa", consumerId = "c").enter()
        assertEquals("nostr:naddr1aaa", rec.calls.single().first)
    }

    @Test
    fun nullClaimerMakesLifecycleANoOp() {
        // No provider scope (LocalEventClaimer.current == null) → no crash, no
        // calls. The composable then shows a static loading placeholder.
        val handle = EventClaimHandle(claimer = null, uri = "u", consumerId = "c")
        handle.enter()
        handle.dispose()
        // Reaching here without throwing is the assertion.
        assertTrue(true)
    }

    @Test
    fun absentEnvelopeRendersLoadingNotPermanentPlaceholder() {
        // The whole point of #984: an out-of-feed ref that the kernel has not
        // yet resolved is LOADING (transient), not a permanent "Event pending".
        assertEquals(EmbedRenderState.LOADING, embedRenderState(null))
    }

    @Test
    fun collapsedEnvelopeRendersCollapsed() {
        val envelope = EmbedEnvelopeEntry(
            primaryId = "abc",
            uri = "nostr:nevent1xyz",
            collapsed = true,
            collapseReason = "max_depth",
            projection = EmbedKindProjectionEntry(
                shortNote = ShortNoteProjectionEntry(id = "abc", authorPubkey = "aa".repeat(32)),
            ),
        )
        assertEquals(EmbedRenderState.COLLAPSED, embedRenderState(envelope))
    }

    @Test
    fun envelopeWithoutProjectionRendersLoading() {
        val envelope = EmbedEnvelopeEntry(primaryId = "abc", uri = "nostr:nevent1xyz", projection = null)
        assertEquals(EmbedRenderState.LOADING, embedRenderState(envelope))
    }

    @Test
    fun resolvedEnvelopeRendersResolved() {
        val envelope = EmbedEnvelopeEntry(
            primaryId = "abc",
            uri = "nostr:nevent1xyz",
            projection = EmbedKindProjectionEntry(
                shortNote = ShortNoteProjectionEntry(
                    id = "abc",
                    authorPubkey = "aa".repeat(32),
                    content = "hello from an embedded note",
                ),
            ),
        )
        assertEquals(EmbedRenderState.RESOLVED, embedRenderState(envelope))
    }
}
