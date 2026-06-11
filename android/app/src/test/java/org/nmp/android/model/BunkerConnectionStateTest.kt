package org.nmp.android.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * V-14 / #963: JSON decode contract tests for [BunkerConnectionState].
 *
 * Verifies that the Android `@Serializable` model mirrors the Rust
 * `BunkerConnectionStateDto` wire shape: snake_case field names, nullable
 * `reason`, and the three bool flags (`is_connected`, `is_reconnecting`,
 * `is_failed`).
 *
 * Android uses the JSON fallback path (no typed FlatBuffers sidecar), so
 * correct deserialization of the `SnapshotProjections.bunkerConnectionState`
 * field is the acceptance criterion.
 */
class BunkerConnectionStateTest {

    private val json = testJson()

    // ── Connected ─────────────────────────────────────────────────────────

    @Test
    fun connectedStateDecodes() {
        val raw = """
            {
                "state": "connected",
                "reason": null,
                "is_connected": true,
                "is_reconnecting": false,
                "is_failed": false
            }
        """.trimIndent()
        val result = json.decodeFromString<BunkerConnectionState>(raw)
        assertEquals("connected", result.state)
        assertNull(result.reason)
        assertTrue(result.isConnected)
        assertFalse(result.isReconnecting)
        assertFalse(result.isFailed)
    }

    // ── Reconnecting (transient flap) ─────────────────────────────────────

    @Test
    fun reconnectingStateWithReasonDecodes() {
        val raw = """
            {
                "state": "reconnecting",
                "reason": "connection reset by peer",
                "is_connected": false,
                "is_reconnecting": true,
                "is_failed": false
            }
        """.trimIndent()
        val result = json.decodeFromString<BunkerConnectionState>(raw)
        assertEquals("reconnecting", result.state)
        assertEquals("connection reset by peer", result.reason)
        assertFalse(result.isConnected)
        assertTrue(result.isReconnecting)
        assertFalse(result.isFailed)
    }

    // ── Failed (permanent) ────────────────────────────────────────────────

    @Test
    fun failedStateWithReasonDecodes() {
        val raw = """
            {
                "state": "failed",
                "reason": "403 Forbidden",
                "is_connected": false,
                "is_reconnecting": false,
                "is_failed": true
            }
        """.trimIndent()
        val result = json.decodeFromString<BunkerConnectionState>(raw)
        assertEquals("failed", result.state)
        assertEquals("403 Forbidden", result.reason)
        assertFalse(result.isConnected)
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
                "state": "connected",
                "is_connected": true,
                "is_reconnecting": false,
                "is_failed": false
            }
        """.trimIndent()
        val result = json.decodeFromString<BunkerConnectionState>(raw)
        assertNull(result.reason)
        assertTrue(result.isConnected)
    }

    // ── Embedded in SnapshotProjections ──────────────────────────────────

    @Test
    fun bunkerConnectionStateDecodesInsideSnapshotProjections() {
        val raw = """
            {
                "bunker_connection_state": {
                    "state": "connected",
                    "reason": null,
                    "is_connected": true,
                    "is_reconnecting": false,
                    "is_failed": false
                }
            }
        """.trimIndent()
        val projections = json.decodeFromString<SnapshotProjections>(raw)
        val connState = projections.bunkerConnectionState
            ?: error("bunkerConnectionState must not be null")
        assertTrue(connState.isConnected)
        assertFalse(connState.isFailed)
    }

    @Test
    fun nullBunkerConnectionStateDecodesInsideSnapshotProjections() {
        // When no bunker session is active the kernel emits JSON `null` for
        // this key; the field must decode to null (not crash).
        val raw = """
            {
                "bunker_connection_state": null
            }
        """.trimIndent()
        val projections = json.decodeFromString<SnapshotProjections>(raw)
        assertNull(projections.bunkerConnectionState)
    }

    @Test
    fun missingBunkerConnectionStateKeyDefaultsToNull() {
        // Older kernels that predate V-14 omit the key entirely.
        val raw = "{}"
        val projections = json.decodeFromString<SnapshotProjections>(raw)
        assertNull(projections.bunkerConnectionState)
    }

    // ── Default value contract ────────────────────────────────────────────

    @Test
    fun defaultBunkerConnectionStateHasNoFlagsSet() {
        // The Kotlin default constructor must produce a safe zero-value.
        val default = BunkerConnectionState()
        assertFalse(default.isConnected)
        assertFalse(default.isReconnecting)
        assertFalse(default.isFailed)
        assertNull(default.reason)
        assertEquals("", default.state)
    }
}
