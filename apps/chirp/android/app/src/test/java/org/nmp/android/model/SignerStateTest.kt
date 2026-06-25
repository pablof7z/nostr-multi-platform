package org.nmp.android.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * ADR-0048 D6 (generalises V-14 / #963): JSON decode contract tests for
 * [SignerState].
 *
 * Verifies that the Android `@Serializable` model mirrors the Rust
 * `SignerStateDto` wire shape: snake_case field names, the `signer_kind`
 * discriminator, nullable `reason`, and the five bool flags (`is_ready`,
 * `is_awaiting_approval`, `is_reconnecting`, `is_unavailable`, `is_failed`).
 *
 * #1493 P9 (labels-to-shells): the wire carries only the raw `state` token +
 * flags. The English `statusLabel` / semantic `statusTone` are NOT serialized —
 * the shell (`TypedSignerStateDecoder`) derives them from `state`. At runtime
 * the typed FlatBuffers sidecar is authoritative (`KernelUpdateFrameDecoder`);
 * these tests pin the structural JSON contract of the shared model.
 */
class SignerStateTest {

    private val json = testJson()

    // ── Ready (NIP-46) ────────────────────────────────────────────────────

    @Test
    fun nip46ReadyStateDecodes() {
        val raw = """
            {
                "signer_kind": "nip46",
                "state": "ready",
                "reason": null,
                "is_ready": true,
                "is_awaiting_approval": false,
                "is_reconnecting": false,
                "is_unavailable": false,
                "is_failed": false
            }
        """.trimIndent()
        val result = json.decodeFromString<SignerState>(raw)
        assertEquals("nip46", result.signerKind)
        assertEquals("ready", result.state)
        assertNull(result.reason)
        assertTrue(result.isReady)
        assertFalse(result.isAwaitingApproval)
        assertFalse(result.isReconnecting)
        assertFalse(result.isUnavailable)
        assertFalse(result.isFailed)
    }

    // ── Reconnecting (NIP-46 transient flap) ──────────────────────────────

    @Test
    fun reconnectingStateWithReasonDecodes() {
        val raw = """
            {
                "signer_kind": "nip46",
                "state": "reconnecting",
                "reason": "connection reset by peer",
                "is_ready": false,
                "is_awaiting_approval": false,
                "is_reconnecting": true,
                "is_unavailable": false,
                "is_failed": false
            }
        """.trimIndent()
        val result = json.decodeFromString<SignerState>(raw)
        assertEquals("reconnecting", result.state)
        assertEquals("connection reset by peer", result.reason)
        assertFalse(result.isReady)
        assertTrue(result.isReconnecting)
        assertFalse(result.isFailed)
    }

    // ── Awaiting approval (NIP-55 Intent round-trip) ──────────────────────

    @Test
    fun nip55AwaitingApprovalStateDecodes() {
        val raw = """
            {
                "signer_kind": "nip55",
                "state": "awaiting_approval",
                "reason": null,
                "is_ready": false,
                "is_awaiting_approval": true,
                "is_reconnecting": false,
                "is_unavailable": false,
                "is_failed": false
            }
        """.trimIndent()
        val result = json.decodeFromString<SignerState>(raw)
        assertEquals("nip55", result.signerKind)
        assertEquals("awaiting_approval", result.state)
        assertTrue(result.isAwaitingApproval)
        assertFalse(result.isReady)
        assertFalse(result.isUnavailable)
    }

    // ── Unavailable (NIP-55 signer app missing) ───────────────────────────

    @Test
    fun nip55UnavailableStateWithReasonDecodes() {
        val raw = """
            {
                "signer_kind": "nip55",
                "state": "unavailable",
                "reason": "signer app not installed",
                "is_ready": false,
                "is_awaiting_approval": false,
                "is_reconnecting": false,
                "is_unavailable": true,
                "is_failed": false
            }
        """.trimIndent()
        val result = json.decodeFromString<SignerState>(raw)
        assertEquals("unavailable", result.state)
        assertEquals("signer app not installed", result.reason)
        assertTrue(result.isUnavailable)
        assertFalse(result.isFailed)
    }

    // ── Failed (permanent) ────────────────────────────────────────────────

    @Test
    fun failedStateWithReasonDecodes() {
        val raw = """
            {
                "signer_kind": "nip46",
                "state": "failed",
                "reason": "403 Forbidden",
                "is_ready": false,
                "is_awaiting_approval": false,
                "is_reconnecting": false,
                "is_unavailable": false,
                "is_failed": true
            }
        """.trimIndent()
        val result = json.decodeFromString<SignerState>(raw)
        assertEquals("failed", result.state)
        assertEquals("403 Forbidden", result.reason)
        assertFalse(result.isReady)
        assertFalse(result.isReconnecting)
        assertTrue(result.isFailed)
    }

    // ── Missing / omitted reason ──────────────────────────────────────────

    @Test
    fun absentReasonDefaultsToNull() {
        // The Rust projection omits the `reason` key entirely when `None` on
        // some code paths; the @Serializable default must tolerate absence.
        val raw = """
            {
                "signer_kind": "nip46",
                "state": "ready",
                "is_ready": true,
                "is_awaiting_approval": false,
                "is_reconnecting": false,
                "is_unavailable": false,
                "is_failed": false
            }
        """.trimIndent()
        val result = json.decodeFromString<SignerState>(raw)
        assertNull(result.reason)
        assertTrue(result.isReady)
    }

    // ── Embedded in SnapshotProjections ──────────────────────────────────

    @Test
    fun signerStateDecodesInsideSnapshotProjections() {
        val raw = """
            {
                "signer_state": {
                    "signer_kind": "nip55",
                    "state": "ready",
                    "reason": null,
                    "is_ready": true,
                    "is_awaiting_approval": false,
                    "is_reconnecting": false,
                    "is_unavailable": false,
                    "is_failed": false
                }
            }
        """.trimIndent()
        val projections = json.decodeFromString<SnapshotProjections>(raw)
        val signerState = projections.signerState
            ?: error("signerState must not be null")
        assertEquals("nip55", signerState.signerKind)
        assertTrue(signerState.isReady)
        assertFalse(signerState.isFailed)
    }

    @Test
    fun nullSignerStateDecodesInsideSnapshotProjections() {
        // When no remote-signer session is active the kernel emits JSON `null`
        // for this key; the field must decode to null (not crash).
        val raw = """
            {
                "signer_state": null
            }
        """.trimIndent()
        val projections = json.decodeFromString<SnapshotProjections>(raw)
        assertNull(projections.signerState)
    }

    @Test
    fun missingSignerStateKeyDefaultsToNull() {
        // Older kernels omit the key entirely.
        val raw = "{}"
        val projections = json.decodeFromString<SnapshotProjections>(raw)
        assertNull(projections.signerState)
    }

    // ── Default value contract ────────────────────────────────────────────

    @Test
    fun defaultSignerStateHasNoFlagsSet() {
        // The Kotlin default constructor must produce a safe zero-value.
        val default = SignerState()
        assertFalse(default.isReady)
        assertFalse(default.isAwaitingApproval)
        assertFalse(default.isReconnecting)
        assertFalse(default.isUnavailable)
        assertFalse(default.isFailed)
        assertNull(default.reason)
        assertEquals("", default.signerKind)
        assertEquals("", default.state)
        // #1493 P9: statusLabel/statusTone are shell-derived presentation, not
        // on the wire; the data-class default is empty and the typed decoder
        // (TypedSignerStateDecoder) fills them from `state`.
        assertEquals("", default.statusLabel)
        assertEquals("", default.statusTone)
    }

    // ── Shell-derived label/tone are NOT on the wire (#1493 P9) ────────────

    @Test
    fun labelAndToneAreNotDeserializedFromJson() {
        // #1493 P9 (labels-to-shells): the JSON projection no longer carries
        // status_label/status_tone. Even if a stray buffer did, the model does
        // not map those keys — they stay empty and are filled by the shell's
        // TypedSignerStateDecoder from the raw `state` token instead.
        val raw = """
            {
                "signer_kind": "nip46",
                "state": "ready",
                "is_ready": true,
                "is_awaiting_approval": false,
                "is_reconnecting": false,
                "is_unavailable": false,
                "is_failed": false
            }
        """.trimIndent()
        val result = json.decodeFromString<SignerState>(raw)
        assertEquals("", result.statusLabel)
        assertEquals("", result.statusTone)
    }
}
