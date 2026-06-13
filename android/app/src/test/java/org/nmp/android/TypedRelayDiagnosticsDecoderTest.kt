package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.kernel.RelayDiagnosticsInterest
import nmp.kernel.RelayDiagnosticsRow
import nmp.kernel.RelayDiagnosticsSnapshot
import nmp.kernel.RelayDiagnosticsWireSub
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Tests for [TypedRelayDiagnosticsDecoder] — the typed-first decode of the
 * kernel-owned `relay_diagnostics` (`KRDG`) snapshot projection (#1099). The
 * decoded row carries Rust-precomputed `connectionLabel`/`connectionTone`
 * (ADR-0032 / V-14) which `RelayScreen` renders verbatim instead of switching
 * on the raw Tier-3 connection string. Coverage:
 *  - a KRDG buffer with one relay row decodes connectionLabel/connectionTone;
 *  - connectionTone "active" is the green-mapped tone (verified on the typed
 *    row, not via statusColors);
 *  - nested wireSubs + interests (with relayUrls string vector) decode;
 *  - absent sidecar / wrong identifier → null.
 */
@OptIn(ExperimentalUnsignedTypes::class)
class TypedRelayDiagnosticsDecoderTest {

    @Test
    fun absentSidecarReturnsNull() {
        assertNull(TypedRelayDiagnosticsDecoder.decode(emptyList()))
    }

    @Test
    fun wrongFileIdentifierReturnsNull() {
        val garbled = diagnosticsBuffer().copyOf()
        garbled[4] = 'X'.code.toByte() // clobber KRDG identifier
        assertNull(TypedRelayDiagnosticsDecoder.decode(garbled))
    }

    @Test
    fun oneRelayRowDecodesConnectionLabelAndTone() {
        val out = requireNotNull(TypedRelayDiagnosticsDecoder.decode(diagnosticsBuffer()))
        assertEquals(1, out.relays.size)
        val row = out.relays[0]
        assertEquals("wss://relay.example.com", row.relayUrl)
        assertEquals("relay.example.com", row.shortUrl)
        assertEquals("Connected", row.connectionLabel)
        assertEquals("active", row.connectionTone)
        assertEquals(3, row.totalSubCount)
        assertEquals(2L, row.totalEventsRx)
    }

    @Test
    fun activeToneIsGreenMapped() {
        // "active" tone is what RelayScreen maps to green. We verify the typed
        // row carries it (the colour map lives in RelayScreen.toneColor, which is
        // private; the contract under test is that the decoder surfaces the tone
        // so the UI never inspects the raw connection token).
        val out = requireNotNull(TypedRelayDiagnosticsDecoder.decode(diagnosticsBuffer()))
        assertEquals("active", out.relays[0].connectionTone)
    }

    @Test
    fun nestedWireSubsDecode() {
        val out = requireNotNull(TypedRelayDiagnosticsDecoder.decode(diagnosticsBuffer()))
        val subs = out.relays[0].wireSubs
        assertEquals(1, subs.size)
        assertEquals("sub-1", subs[0].wireId)
        assertEquals("Listening", subs[0].stateLabel)
        assertEquals("active", subs[0].stateTone)
        assertTrue(subs[0].eoseObserved)
    }

    @Test
    fun interestsWithRelayUrlsDecode() {
        val out = requireNotNull(TypedRelayDiagnosticsDecoder.decode(diagnosticsBuffer()))
        assertEquals(1, out.interests.size)
        val interest = out.interests[0]
        assertEquals("home-feed", interest.key)
        assertEquals("ready", interest.state)
        assertEquals(2, interest.refcount)
        assertEquals(listOf("wss://a.relay", "wss://b.relay"), interest.relayUrls)
    }

    // ── builders ───────────────────────────────────────────────────────────────

    private fun diagnosticsBuffer(): ByteArray {
        val b = FlatBufferBuilder(1024)

        // wire sub
        val subWireId = b.createString("sub-1")
        val subShort = b.createString("sub-1")
        val subRelay = b.createString("wss://relay.example.com")
        val subFilter = b.createString("kinds=[1]")
        val subStateLabel = b.createString("Listening")
        val subStateTone = b.createString("active")
        val subConsumer = b.createString("2 consumers")
        val subOpened = b.createString("just now")
        RelayDiagnosticsWireSub.startRelayDiagnosticsWireSub(b)
        RelayDiagnosticsWireSub.addWireId(b, subWireId)
        RelayDiagnosticsWireSub.addShortWireId(b, subShort)
        RelayDiagnosticsWireSub.addRelayUrl(b, subRelay)
        RelayDiagnosticsWireSub.addFilterSummary(b, subFilter)
        RelayDiagnosticsWireSub.addStateLabel(b, subStateLabel)
        RelayDiagnosticsWireSub.addStateTone(b, subStateTone)
        RelayDiagnosticsWireSub.addConsumerCountLabel(b, subConsumer)
        RelayDiagnosticsWireSub.addEoseObserved(b, true)
        RelayDiagnosticsWireSub.addOpenedDisplay(b, subOpened)
        val sub = RelayDiagnosticsWireSub.endRelayDiagnosticsWireSub(b)
        val wireSubsVec = RelayDiagnosticsRow.createWireSubsVector(b, intArrayOf(sub))

        // relay row
        val relayUrl = b.createString("wss://relay.example.com")
        val shortUrl = b.createString("relay.example.com")
        val roleLabel = b.createString("Read/Write")
        val roleTone = b.createString("active")
        val connLabel = b.createString("Connected")
        val connTone = b.createString("active")
        val authLabel = b.createString("Authenticated")
        val authTone = b.createString("active")
        val totalEvents = b.createString("2 events")
        RelayDiagnosticsRow.startRelayDiagnosticsRow(b)
        RelayDiagnosticsRow.addRelayUrl(b, relayUrl)
        RelayDiagnosticsRow.addShortUrl(b, shortUrl)
        RelayDiagnosticsRow.addRoleLabel(b, roleLabel)
        RelayDiagnosticsRow.addRoleTone(b, roleTone)
        RelayDiagnosticsRow.addConnectionLabel(b, connLabel)
        RelayDiagnosticsRow.addConnectionTone(b, connTone)
        RelayDiagnosticsRow.addAuthLabel(b, authLabel)
        RelayDiagnosticsRow.addAuthTone(b, authTone)
        RelayDiagnosticsRow.addTotalSubCount(b, 3u)
        RelayDiagnosticsRow.addActiveSubCount(b, 1u)
        RelayDiagnosticsRow.addEosedSubCount(b, 2u)
        RelayDiagnosticsRow.addTotalEventsRx(b, 2UL)
        RelayDiagnosticsRow.addTotalEventsDisplay(b, totalEvents)
        RelayDiagnosticsRow.addReconnectCount(b, 0u)
        RelayDiagnosticsRow.addWireSubs(b, wireSubsVec)
        val row = RelayDiagnosticsRow.endRelayDiagnosticsRow(b)
        val relaysVec = RelayDiagnosticsSnapshot.createRelaysVector(b, intArrayOf(row))

        // interest with relayUrls string vector
        val iKey = b.createString("home-feed")
        val iState = b.createString("ready")
        val iStateTone = b.createString("active")
        val iCoverage = b.createString("full")
        val url0 = b.createString("wss://a.relay")
        val url1 = b.createString("wss://b.relay")
        val urlsVec = RelayDiagnosticsInterest.createRelayUrlsVector(b, intArrayOf(url0, url1))
        RelayDiagnosticsInterest.startRelayDiagnosticsInterest(b)
        RelayDiagnosticsInterest.addKey(b, iKey)
        RelayDiagnosticsInterest.addState(b, iState)
        RelayDiagnosticsInterest.addStateTone(b, iStateTone)
        RelayDiagnosticsInterest.addRefcount(b, 2u)
        RelayDiagnosticsInterest.addCacheCoverage(b, iCoverage)
        RelayDiagnosticsInterest.addRelayUrls(b, urlsVec)
        val interest = RelayDiagnosticsInterest.endRelayDiagnosticsInterest(b)
        val interestsVec = RelayDiagnosticsSnapshot.createInterestsVector(b, intArrayOf(interest))

        val snap = RelayDiagnosticsSnapshot.createRelayDiagnosticsSnapshot(b, relaysVec, interestsVec)
        RelayDiagnosticsSnapshot.finishRelayDiagnosticsSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }
}
