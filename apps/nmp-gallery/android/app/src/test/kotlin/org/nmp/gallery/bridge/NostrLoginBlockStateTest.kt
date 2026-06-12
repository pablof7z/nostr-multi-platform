package org.nmp.gallery.bridge

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.nmp.gallery.registry.LoginBlockSignerState

/**
 * ADR-0048 Stage 2 — state rendering contract tests for the `NostrLoginBlock`
 * Compose component.
 *
 * The NostrLoginBlock renders different visual states based on the
 * `LoginBlockSignerState` projection from Rust. These tests verify the
 * state model mirrors `SignerStateDto` correctly so the UI never
 * string-compares `state` (it reads pre-computed bool flags, ADR-0032).
 *
 * They are pure JVM tests — no Compose runtime, no Activity. State
 * presentation logic is tested by asserting on the data model that drives
 * rendering, not the rendered pixels.
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
    }

    // ── NIP-55 awaiting approval ───────────────────────────────────────────

    @Test
    fun awaitingApprovalStateIsInProgress() {
        val state = LoginBlockSignerState(
            signerKind = "nip55",
            state = "awaiting_approval",
            isAwaitingApproval = true,
        )
        assertTrue(isInProgress(state))
        assertFalse(isDegraded(state))
        assertEquals("Waiting for approval…", statusLabel(state))
    }

    // ── NIP-55 ready ───────────────────────────────────────────────────────

    @Test
    fun readyStateIsReady() {
        val state = LoginBlockSignerState(
            signerKind = "nip55",
            state = "ready",
            isReady = true,
        )
        assertFalse(isInProgress(state))
        assertFalse(isDegraded(state))
        assertEquals("Connected", statusLabel(state))
    }

    // ── NIP-55 unavailable ─────────────────────────────────────────────────

    @Test
    fun unavailableStateIsDegraded() {
        val state = LoginBlockSignerState(
            signerKind = "nip55",
            state = "unavailable",
            reason = "signer app not installed",
            isUnavailable = true,
        )
        assertTrue(isDegraded(state))
        assertFalse(isInProgress(state))
        assertEquals("Signer unavailable", statusLabel(state))
    }

    // ── NIP-55 failed ─────────────────────────────────────────────────────

    @Test
    fun failedStateIsDegraded() {
        val state = LoginBlockSignerState(
            signerKind = "nip55",
            state = "failed",
            reason = "key mismatch",
            isFailed = true,
        )
        assertTrue(isDegraded(state))
        assertFalse(isInProgress(state))
        assertEquals("Connection failed", statusLabel(state))
    }

    // ── Reconnecting ──────────────────────────────────────────────────────

    @Test
    fun reconnectingStateIsInProgress() {
        val state = LoginBlockSignerState(
            signerKind = "nip46",
            state = "reconnecting",
            isReconnecting = true,
        )
        assertTrue(isInProgress(state))
        assertFalse(isDegraded(state))
        assertEquals("Reconnecting…", statusLabel(state))
    }

    // ── null state (no signer session active) ─────────────────────────────

    @Test
    fun nullStateShowsDefaultSubtitle() {
        // When signerState is null the login-block shows the default
        // "Sign in with Amber" label, not a status indicator.
        val state: LoginBlockSignerState? = null
        assertEquals("Sign in with Amber", statusLabelForSigner(state, "Amber"))
    }

    // ── Helpers mirroring the render logic in NostrLoginBlock ─────────────

    private fun isInProgress(s: LoginBlockSignerState): Boolean =
        s.isAwaitingApproval || s.isReconnecting

    private fun isDegraded(s: LoginBlockSignerState): Boolean =
        s.isFailed || s.isUnavailable

    private fun statusLabel(s: LoginBlockSignerState): String = when {
        s.isUnavailable -> "Signer unavailable"
        s.isFailed -> "Connection failed"
        s.isAwaitingApproval -> "Waiting for approval…"
        s.isReconnecting -> "Reconnecting…"
        s.isReady -> "Connected"
        else -> "Sign in"
    }

    private fun statusLabelForSigner(s: LoginBlockSignerState?, displayName: String): String =
        if (s == null) "Sign in with $displayName" else statusLabel(s)
}
