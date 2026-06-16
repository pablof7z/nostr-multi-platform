package org.nmp.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Golden-frame regression test for issue #1084 (Android completely dark at v0.3.0).
 *
 * **Root cause**: `KernelUpdateFrameDecoder.decodeSnapshot` used the removed
 * generic snapshot value path as its gate, so typed-only frames silently
 * dropped and left Android dark.
 *
 * **Fix**: rebuild the decode spine from the Tier-3 `SnapshotFrame` envelope fields
 * (ADR-0044): `rev`, `running`, `metrics`, `relay_statuses`, `last_error_toast` —
 * exactly as iOS `KernelUpdateFrameDecoder.swift` does.
 *
 * **Oracle**: the fixture `update_frame_tier3_golden_v1.fb.hex` encodes a
 * `SnapshotEnvelope` with every Tier-3 field set to a non-default value (the
 * same `golden_envelope()` from `crates/nmp-core/src/update_envelope/tests.rs`).
 * To regenerate after an intentional schema change, run
 * `cargo test -p nmp-core -- tier3_golden_fixture_matches_encoder --nocapture`;
 * the new hex is printed to stderr only when the fixture drifts (not on every run).
 *
 * TDD evidence: this test FAILED before the fix (the old decoder returned null on
 * typed-only frames) and PASSES after the Tier-3 spine rebuild.
 */
@OptIn(ExperimentalUnsignedTypes::class)
class KernelUpdateFrameDecoderTier3GoldenTest {

    @Test
    fun tier3GoldenFrameDecodesWithNonDefaultFieldValues() {
        val hex = javaClass.classLoader
            ?.getResourceAsStream("fixtures/update_frame_tier3_golden_v1.fb.hex")
            ?.bufferedReader()
            ?.readText()
            ?.trim()
            ?: error("fixture not found on classpath")

        val bytes = bytesFromHex(hex)
        val decoded = KernelUpdateFrameDecoder.decode(bytes)

        // Before the fix: decode() returned null on typed-only frames.
        // After the fix: decode() returns a non-null Snapshot.
        assertNotNull(
            "#1084 regression: decode() must not return null for a typed Tier-3 frame",
            decoded,
        )
        check(decoded is KernelDecodedUpdateFrame.Snapshot) { "expected Snapshot, got $decoded" }

        val update = decoded.update

        // rev = 42 (matches golden_envelope() in Rust tests.rs)
        assertEquals(
            "rev must come from Tier-3 SnapshotFrame.rev",
            42L,
            update.rev,
        )

        // running = true
        assertTrue("running must come from Tier-3 SnapshotFrame.running", update.running)

        // lastErrorToast = "boom"
        assertEquals(
            "lastErrorToast must come from Tier-3 SnapshotFrame.last_error_toast",
            "boom",
            update.lastErrorToast,
        )

        // metrics: visibleItems = 3, eventsRx = 7, updateSequence = 11
        val metrics = update.metrics
        assertNotNull("metrics must come from Tier-3 SnapshotFrame.metrics", metrics)
        assertEquals("metrics.visibleItems", 3L, metrics!!.visibleItems)
        assertEquals("metrics.eventsRx", 7L, metrics.eventsRx)
        assertEquals("metrics.updateSequence", 11L, metrics.updateSequence)

        // relay_statuses: one relay with role="both", url="wss://relay.example",
        // connection="connected", auth="accepted"
        assertEquals("relay_statuses count", 1, update.relayStatuses.size)
        val rs = update.relayStatuses[0]
        assertEquals("relay role", "both", rs.role)
        assertEquals("relay url", "wss://relay.example", rs.relayUrl)
        assertEquals("relay connection", "connected", rs.connection)
        assertEquals("relay auth", "accepted", rs.auth)
    }

    @Test
    fun tier3GoldenFrameHasNoTypedProjections() {
        val hex = javaClass.classLoader
            ?.getResourceAsStream("fixtures/update_frame_tier3_golden_v1.fb.hex")
            ?.bufferedReader()
            ?.readText()
            ?.trim()
            ?: error("fixture not found on classpath")
        val bytes = bytesFromHex(hex)
        val decoded = KernelUpdateFrameDecoder.decode(bytes) as KernelDecodedUpdateFrame.Snapshot
        // The golden frame has no typed sidecars (passed [] to encode_snapshot_frame).
        assertTrue(
            "golden frame has no typed projection sidecars",
            decoded.typedProjections.isEmpty(),
        )
    }

    private fun bytesFromHex(hex: String): ByteArray {
        val compact = hex.filterNot { it.isWhitespace() }
        require(compact.length % 2 == 0) { "hex fixture must contain whole bytes, got ${compact.length} chars" }
        return ByteArray(compact.length / 2) { i ->
            compact.substring(i * 2, i * 2 + 2).toInt(16).toByte()
        }
    }
}
