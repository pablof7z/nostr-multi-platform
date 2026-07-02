package org.nmp.gallery.bridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.nmp.gallery.registry.LoginBlockSignerState
import org.nmp.gallery.registry.SignerCardTone
import org.nmp.gallery.registry.signerCardUi

/**
 * ADR-0072 Stage 2 — state rendering contract tests for the `NostrLoginBlock`
 * Compose component.
 *
 * These assert on the PRODUCTION `signerCardUi` presentation function — the
 * exact pure function `SignerCard` renders from (no test-side mirror of the
 * render rule). The UI never string-compares `state`: presentation derives
 * only from the pre-computed bool flags (ADR-0072 / D6).
 *
 * Pure JVM tests — no Compose runtime, no Activity.
 */
class NostrLoginBlockStateTest {

    // ── Default state (no active signer session) ───────────────────────────

    @Test
    fun defaultStateHasNoFlagsSet() {
        val state = LoginBlockSignerState()
        assertFalse(state.isReady)
        assertFalse(state.isAwaitingApproval)
        assertFalse(state.isReconnecting)
        assertFalse(state.isUnavailable)
        assertFalse(state.isFailed)
        assertNull(state.reason)
        assertEquals("", state.signerKind)
        assertEquals("", state.state)

        val ui = signerCardUi(state)
        assertEquals(SignerCardTone.Default, ui.tone)
        assertNull(ui.statusLabel)
        assertFalse(ui.showSpinner)
    }

    // ── NIP-55 awaiting approval ───────────────────────────────────────────

    @Test
    fun awaitingApprovalStateIsInProgress() {
        val ui = signerCardUi(
            LoginBlockSignerState(
                signerKind = "nip55",
                state = "awaiting_approval",
                isAwaitingApproval = true,
            ),
        )
        assertEquals(SignerCardTone.InProgress, ui.tone)
        assertEquals("Waiting for approval…", ui.statusLabel)
        assertTrue(ui.showSpinner)
    }

    // ── NIP-55 ready ───────────────────────────────────────────────────────

    @Test
    fun readyStateIsReady() {
        val ui = signerCardUi(
            LoginBlockSignerState(
                signerKind = "nip55",
                state = "ready",
                isReady = true,
            ),
        )
        assertEquals(SignerCardTone.Ready, ui.tone)
        assertEquals("Connected", ui.statusLabel)
        assertFalse(ui.showSpinner)
    }

    // ── NIP-55 unavailable ─────────────────────────────────────────────────

    @Test
    fun unavailableStateIsDegraded() {
        val ui = signerCardUi(
            LoginBlockSignerState(
                signerKind = "nip55",
                state = "unavailable",
                reason = "signer app not installed",
                isUnavailable = true,
            ),
        )
        assertEquals(SignerCardTone.Degraded, ui.tone)
        assertEquals("Signer unavailable", ui.statusLabel)
        assertFalse(ui.showSpinner)
    }

    // ── NIP-55 failed ─────────────────────────────────────────────────────

    @Test
    fun failedStateIsDegraded() {
        val ui = signerCardUi(
            LoginBlockSignerState(
                signerKind = "nip55",
                state = "failed",
                reason = "key mismatch",
                isFailed = true,
            ),
        )
        assertEquals(SignerCardTone.Degraded, ui.tone)
        assertEquals("Connection failed", ui.statusLabel)
        assertFalse(ui.showSpinner)
    }

    // ── Reconnecting (NIP-46 — same unified projection) ───────────────────

    @Test
    fun reconnectingStateIsInProgress() {
        val ui = signerCardUi(
            LoginBlockSignerState(
                signerKind = "nip46",
                state = "reconnecting",
                isReconnecting = true,
            ),
        )
        assertEquals(SignerCardTone.InProgress, ui.tone)
        assertEquals("Reconnecting…", ui.statusLabel)
        assertTrue(ui.showSpinner)
    }

    // ── Degraded wins over in-progress (defensive ordering) ───────────────

    @Test
    fun degradedTakesPrecedenceOverInProgress() {
        val ui = signerCardUi(
            LoginBlockSignerState(
                signerKind = "nip55",
                state = "failed",
                isFailed = true,
                isAwaitingApproval = true, // contradictory flags — degraded wins
            ),
        )
        assertEquals(SignerCardTone.Degraded, ui.tone)
    }

    // ── null state (no signer session active) ─────────────────────────────

    @Test
    fun nullStateShowsDefaultSubtitle() {
        // When signerState is null the card falls back to the default
        // "Sign in with …" subtitle (statusLabel == null) with no spinner.
        val ui = signerCardUi(null)
        assertEquals(SignerCardTone.Default, ui.tone)
        assertNull(ui.statusLabel)
        assertFalse(ui.showSpinner)
    }
}
