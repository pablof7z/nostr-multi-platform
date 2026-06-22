package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.kernel.RelayDiagnosticsInfo
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
 * kernel-owned `relay_diagnostics` (`KRDG`) snapshot projection (#1099).
 *
 * Updated for #1493: presentation-formatting fields removed from the wire.
 * Raw fields (role, connection, auth as lowercase strings; bytesRx/bytesTx
 * as ULong; discoveryKinds as ulong vector) replace the old pre-formatted
 * strings. The model computes display properties (roleLabel, connectionLabel,
 * authLabel, totalEventsDisplay, bytesRxDisplay, etc.) in the shell.
 *
 * Coverage:
 *  - a KRDG buffer with one relay row decodes raw connection; tone is derived
 *    shell-side from the raw token (#1768, no tone on the wire);
 *  - computed connectionLabel derives from raw connection string;
 *  - nested wireSubs + interests (with relayUrls string vector) decode;
 *  - discoveryKinds decodes as List<Long>;
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
    fun oneRelayRowDecodesRawConnectionAndTone() {
        val out = requireNotNull(TypedRelayDiagnosticsDecoder.decode(diagnosticsBuffer()))
        assertEquals(1, out.relays.size)
        val row = out.relays[0]
        assertEquals("wss://relay.example.com", row.relayUrl)
        // Raw wire fields
        assertEquals("connected", row.connection)
        // #1768 — tone is now derived shell-side from the raw `connection`
        // token ("connected" → "ok"), not carried on the wire.
        assertEquals("ok", row.connectionTone)
        assertEquals("content", row.role)
        assertEquals(3, row.totalSubCount)
        assertEquals(2L, row.totalEventsRx)
        // Computed display properties
        assertEquals("relay.example.com", row.shortUrl)
        assertEquals("Connected", row.connectionLabel)
        assertEquals("Content", row.roleLabel)
    }

    @Test
    fun connectionToneIsGreenMapped() {
        // #1768 — the shell derives the tone from the raw `connection` token:
        // "connected" → "ok", which RelayScreen.toneColor maps to green.
        val out = requireNotNull(TypedRelayDiagnosticsDecoder.decode(diagnosticsBuffer()))
        assertEquals("ok", out.relays[0].connectionTone)
    }

    @Test
    fun nestedWireSubsDecode() {
        val out = requireNotNull(TypedRelayDiagnosticsDecoder.decode(diagnosticsBuffer()))
        val subs = out.relays[0].wireSubs
        assertEquals(1, subs.size)
        assertEquals("sub-1", subs[0].wireId)
        // Raw wire fields
        assertEquals("open", subs[0].state)
        // #1768 — tone derived shell-side from raw `state` ("open" → "ok").
        assertEquals("ok", subs[0].stateTone)
        assertEquals(2, subs[0].consumerCount)
        assertEquals(0L, subs[0].eventsRx)
        assertTrue(subs[0].eoseObserved)
        // Computed display properties
        assertEquals("sub-1", subs[0].shortWireId)
        assertEquals("Open", subs[0].stateLabel)
        assertEquals("2 consumers", subs[0].consumerCountLabel)
        assertNull(subs[0].eventsRxDisplay) // 0 events → null display
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

    @Test
    fun rowWithoutInfoTableDecodesNullInfo() {
        // ADR-0051: the `info` child table is optional; a row that omits it (the
        // JSON `info: null` case — no document fetched yet) must decode to a null
        // `info`, not a default-filled struct.
        val out = requireNotNull(TypedRelayDiagnosticsDecoder.decode(diagnosticsBuffer()))
        assertNull(out.relays[0].info)
    }

    @Test
    fun bytesRawCountersDecodeAndDisplayComputed() {
        val out = requireNotNull(TypedRelayDiagnosticsDecoder.decode(diagnosticsBufferWithBytes()))
        val row = out.relays[0]
        assertEquals(4096L, row.bytesRx)
        assertEquals(0L, row.bytesTx)
        // Lock the exact rendered label (4096 / 1024 = 4.0 KB) so the
        // `KB`-vs-`KiB` cross-shell parity can't silently regress.
        assertEquals("4.0 KB", row.bytesRxDisplay)
        assertNull(row.bytesTxDisplay)           // 0 → null
    }

    @Test
    fun discoveryKindsDecodeAsLongList() {
        val out = requireNotNull(TypedRelayDiagnosticsDecoder.decode(diagnosticsBufferWithDiscoveryKinds()))
        val row = out.relays[0]
        assertEquals(listOf(0L, 3L, 10002L), row.discoveryKinds)
    }

    @Test
    fun rowWithFullInfoTableDecodesAllFields() {
        // ADR-0051: a fully-populated NIP-11 info document decodes field-for-field.
        val out = requireNotNull(TypedRelayDiagnosticsDecoder.decode(diagnosticsBufferWithInfo()))
        val info = requireNotNull(out.relays[0].info)
        assertEquals("Example Relay", info.name)
        assertEquals("A test relay", info.description)
        assertEquals("https://relay.example.com/icon.png", info.icon)
        assertEquals("abcd1234", info.pubkey)
        assertEquals("admin@relay.example.com", info.contact)
        assertEquals("strfry", info.software)
        assertEquals("1.0.0", info.version)
        assertEquals(listOf(1, 11, 42), info.supportedNips)
        assertEquals(true, info.paymentRequired)
        assertEquals(false, info.authRequired)
        assertEquals(true, info.restrictedWrites)
    }

    @Test
    fun rowWithPartialInfoTableLeavesAbsentFieldsNull() {
        // `has_* == false` lifts to null, byte-faithful to the JSON path's `null`.
        val out = requireNotNull(TypedRelayDiagnosticsDecoder.decode(diagnosticsBufferWithPartialInfo()))
        val info = requireNotNull(out.relays[0].info)
        assertEquals("Minimal Relay", info.name)
        assertNull(info.description)
        assertNull(info.icon)
        assertNull(info.pubkey)
        assertNull(info.contact)
        assertNull(info.software)
        assertNull(info.version)
        assertEquals(emptyList<Int>(), info.supportedNips)
        assertNull(info.paymentRequired)
        assertEquals(true, info.authRequired)
        assertNull(info.restrictedWrites)
    }

    // ── builders ───────────────────────────────────────────────────────────────

    private fun diagnosticsBuffer(): ByteArray {
        val b = FlatBufferBuilder(1024)

        // wire sub — raw fields (no shortWireId, stateLabel, consumerCountLabel, eventsRxDisplay)
        val subWireId = b.createString("sub-1")
        val subRelay = b.createString("wss://relay.example.com")
        val subFilter = b.createString("kinds=[1]")
        val subState = b.createString("open")
        RelayDiagnosticsWireSub.startRelayDiagnosticsWireSub(b)
        RelayDiagnosticsWireSub.addWireId(b, subWireId)
        RelayDiagnosticsWireSub.addRelayUrl(b, subRelay)
        RelayDiagnosticsWireSub.addFilterSummary(b, subFilter)
        RelayDiagnosticsWireSub.addState(b, subState)
        RelayDiagnosticsWireSub.addConsumerCount(b, 2u)
        RelayDiagnosticsWireSub.addEventsRx(b, 0UL)
        RelayDiagnosticsWireSub.addEoseObserved(b, true)
        // aim.md §62: raw Unix-epoch-ms timestamp.
        RelayDiagnosticsWireSub.addOpenedMs(b, 1_700_000_000_000UL)
        val sub = RelayDiagnosticsWireSub.endRelayDiagnosticsWireSub(b)
        val wireSubsVec = RelayDiagnosticsRow.createWireSubsVector(b, intArrayOf(sub))

        // relay row — raw fields (no shortUrl, roleLabel, connectionLabel, etc.)
        val relayUrl = b.createString("wss://relay.example.com")
        val role = b.createString("content")
        val conn = b.createString("connected")
        val auth = b.createString("ok")
        RelayDiagnosticsRow.startRelayDiagnosticsRow(b)
        RelayDiagnosticsRow.addRelayUrl(b, relayUrl)
        RelayDiagnosticsRow.addRole(b, role)
        RelayDiagnosticsRow.addConnection(b, conn)
        RelayDiagnosticsRow.addAuth(b, auth)
        RelayDiagnosticsRow.addTotalSubCount(b, 3u)
        RelayDiagnosticsRow.addActiveSubCount(b, 1u)
        RelayDiagnosticsRow.addEosedSubCount(b, 2u)
        RelayDiagnosticsRow.addTotalEventsRx(b, 2UL)
        RelayDiagnosticsRow.addReconnectCount(b, 0u)
        RelayDiagnosticsRow.addWireSubs(b, wireSubsVec)
        val row = RelayDiagnosticsRow.endRelayDiagnosticsRow(b)
        val relaysVec = RelayDiagnosticsSnapshot.createRelaysVector(b, intArrayOf(row))

        // interest with relayUrls string vector
        val iKey = b.createString("home-feed")
        val iState = b.createString("ready")
        val iCoverage = b.createString("full")
        val url0 = b.createString("wss://a.relay")
        val url1 = b.createString("wss://b.relay")
        val urlsVec = RelayDiagnosticsInterest.createRelayUrlsVector(b, intArrayOf(url0, url1))
        RelayDiagnosticsInterest.startRelayDiagnosticsInterest(b)
        RelayDiagnosticsInterest.addKey(b, iKey)
        RelayDiagnosticsInterest.addState(b, iState)
        RelayDiagnosticsInterest.addRefcount(b, 2u)
        RelayDiagnosticsInterest.addCacheCoverage(b, iCoverage)
        RelayDiagnosticsInterest.addRelayUrls(b, urlsVec)
        val interest = RelayDiagnosticsInterest.endRelayDiagnosticsInterest(b)
        val interestsVec = RelayDiagnosticsSnapshot.createInterestsVector(b, intArrayOf(interest))

        val snap = RelayDiagnosticsSnapshot.createRelayDiagnosticsSnapshot(b, relaysVec, interestsVec)
        RelayDiagnosticsSnapshot.finishRelayDiagnosticsSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }

    /** Buffer with bytesRx=4096 and bytesTx=0 to verify computed display properties. */
    private fun diagnosticsBufferWithBytes(): ByteArray {
        val b = FlatBufferBuilder(512)
        val relayUrl = b.createString("wss://relay.example.com")
        val role = b.createString("content")
        val conn = b.createString("connected")
        val auth = b.createString("ok")
        RelayDiagnosticsRow.startRelayDiagnosticsRow(b)
        RelayDiagnosticsRow.addRelayUrl(b, relayUrl)
        RelayDiagnosticsRow.addRole(b, role)
        RelayDiagnosticsRow.addConnection(b, conn)
        RelayDiagnosticsRow.addAuth(b, auth)
        RelayDiagnosticsRow.addBytesRx(b, 4096UL)
        RelayDiagnosticsRow.addBytesTx(b, 0UL)
        val row = RelayDiagnosticsRow.endRelayDiagnosticsRow(b)
        val relaysVec = RelayDiagnosticsSnapshot.createRelaysVector(b, intArrayOf(row))
        val snap = RelayDiagnosticsSnapshot.createRelayDiagnosticsSnapshot(b, relaysVec, 0)
        RelayDiagnosticsSnapshot.finishRelayDiagnosticsSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }

    /** Buffer with discoveryKinds = [0, 3, 10002]. */
    private fun diagnosticsBufferWithDiscoveryKinds(): ByteArray {
        val b = FlatBufferBuilder(512)
        @OptIn(ExperimentalUnsignedTypes::class)
        val kindsVec = RelayDiagnosticsRow.createDiscoveryKindsVector(b, ulongArrayOf(0uL, 3uL, 10002uL))
        val relayUrl = b.createString("wss://relay.example.com")
        val role = b.createString("indexer")
        val conn = b.createString("connected")
        val auth = b.createString("ok")
        RelayDiagnosticsRow.startRelayDiagnosticsRow(b)
        RelayDiagnosticsRow.addRelayUrl(b, relayUrl)
        RelayDiagnosticsRow.addRole(b, role)
        RelayDiagnosticsRow.addConnection(b, conn)
        RelayDiagnosticsRow.addAuth(b, auth)
        RelayDiagnosticsRow.addDiscoveryKinds(b, kindsVec)
        val row = RelayDiagnosticsRow.endRelayDiagnosticsRow(b)
        val relaysVec = RelayDiagnosticsSnapshot.createRelaysVector(b, intArrayOf(row))
        val snap = RelayDiagnosticsSnapshot.createRelayDiagnosticsSnapshot(b, relaysVec, 0)
        RelayDiagnosticsSnapshot.finishRelayDiagnosticsSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }

    /** A snapshot with a single relay row carrying a fully-populated NIP-11 info. */
    private fun diagnosticsBufferWithInfo(): ByteArray = relayWithInfoBuffer(full = true)

    /** A snapshot whose relay row carries only `name` + `auth_required`. */
    private fun diagnosticsBufferWithPartialInfo(): ByteArray = relayWithInfoBuffer(full = false)

    @OptIn(kotlin.ExperimentalUnsignedTypes::class)
    private fun relayWithInfoBuffer(full: Boolean): ByteArray {
        val b = FlatBufferBuilder(1024)

        // info child table
        val info: Int
        if (full) {
            val iName = b.createString("Example Relay")
            val iDesc = b.createString("A test relay")
            val iIcon = b.createString("https://relay.example.com/icon.png")
            val iPubkey = b.createString("abcd1234")
            val iContact = b.createString("admin@relay.example.com")
            val iSoftware = b.createString("strfry")
            val iVersion = b.createString("1.0.0")
            val nipsVec = RelayDiagnosticsInfo.createSupportedNipsVector(b, uintArrayOf(1u, 11u, 42u))
            RelayDiagnosticsInfo.startRelayDiagnosticsInfo(b)
            RelayDiagnosticsInfo.addHasName(b, true)
            RelayDiagnosticsInfo.addName(b, iName)
            RelayDiagnosticsInfo.addHasDescription(b, true)
            RelayDiagnosticsInfo.addDescription(b, iDesc)
            RelayDiagnosticsInfo.addHasIcon(b, true)
            RelayDiagnosticsInfo.addIcon(b, iIcon)
            RelayDiagnosticsInfo.addHasPubkey(b, true)
            RelayDiagnosticsInfo.addPubkey(b, iPubkey)
            RelayDiagnosticsInfo.addHasContact(b, true)
            RelayDiagnosticsInfo.addContact(b, iContact)
            RelayDiagnosticsInfo.addHasSoftware(b, true)
            RelayDiagnosticsInfo.addSoftware(b, iSoftware)
            RelayDiagnosticsInfo.addHasVersion(b, true)
            RelayDiagnosticsInfo.addVersion(b, iVersion)
            RelayDiagnosticsInfo.addSupportedNips(b, nipsVec)
            RelayDiagnosticsInfo.addHasPaymentRequired(b, true)
            RelayDiagnosticsInfo.addPaymentRequired(b, true)
            RelayDiagnosticsInfo.addHasAuthRequired(b, true)
            RelayDiagnosticsInfo.addAuthRequired(b, false)
            RelayDiagnosticsInfo.addHasRestrictedWrites(b, true)
            RelayDiagnosticsInfo.addRestrictedWrites(b, true)
            info = RelayDiagnosticsInfo.endRelayDiagnosticsInfo(b)
        } else {
            val iName = b.createString("Minimal Relay")
            RelayDiagnosticsInfo.startRelayDiagnosticsInfo(b)
            RelayDiagnosticsInfo.addHasName(b, true)
            RelayDiagnosticsInfo.addName(b, iName)
            // description/icon/pubkey/contact/software/version: has_* default false
            // supported_nips: absent vector → empty list
            // payment/restricted: has_* default false; only auth advertised
            RelayDiagnosticsInfo.addHasAuthRequired(b, true)
            RelayDiagnosticsInfo.addAuthRequired(b, true)
            info = RelayDiagnosticsInfo.endRelayDiagnosticsInfo(b)
        }

        // relay row carrying the info table — raw fields
        val relayUrl = b.createString("wss://relay.example.com")
        val role = b.createString("both")
        val conn = b.createString("connected")
        val auth = b.createString("ok")
        RelayDiagnosticsRow.startRelayDiagnosticsRow(b)
        RelayDiagnosticsRow.addRelayUrl(b, relayUrl)
        RelayDiagnosticsRow.addRole(b, role)
        RelayDiagnosticsRow.addConnection(b, conn)
        RelayDiagnosticsRow.addAuth(b, auth)
        RelayDiagnosticsRow.addInfo(b, info)
        val row = RelayDiagnosticsRow.endRelayDiagnosticsRow(b)
        val relaysVec = RelayDiagnosticsSnapshot.createRelaysVector(b, intArrayOf(row))

        val snap = RelayDiagnosticsSnapshot.createRelayDiagnosticsSnapshot(b, relaysVec, 0)
        RelayDiagnosticsSnapshot.finishRelayDiagnosticsSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }
}
