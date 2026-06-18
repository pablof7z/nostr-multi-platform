package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.kernel.ClaimedProfileEntry
import nmp.kernel.ClaimedProfilesSnapshot
import nmp.kernel.ProfileCard as FbProfileCard
import nmp.kernel.ResolvedProfileEntry
import nmp.kernel.ResolvedProfilesSnapshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Contract tests for [TypedProfilesDecoder] (F-05 / #979): the typed `KRPR`
 * `resolved_profiles` and `KCPR` `claimed_profiles` sidecars decode into the
 * pubkey -> [org.nmp.android.model.ProfileCard] maps, with the `has_*`
 * companion bools reproducing JSON `null`-when-absent semantics, and a
 * malformed/absent sidecar yielding `null` so the caller falls back.
 */
class TypedProfilesDecoderTest {

    private fun hex(b: Int): String = "%02x".format(b and 0xff).repeat(32)

    /** Build a `ProfileCard` row offset with the given presence/value choices. */
    private fun profileCardOffset(
        builder: FlatBufferBuilder,
        pubkey: String,
        displayName: String?,
        pictureUrl: String?,
        lnurl: String?,
    ): Int {
        val pubkeyOff = builder.createString(pubkey)
        // V-115 / ADR-0032: `npub` removed from profile_card.fbs schema.
        val dnOff = if (displayName != null) builder.createString(displayName) else 0
        val pxOff = if (pictureUrl != null) builder.createString(pictureUrl) else 0
        val nip05Off = builder.createString("nip05@example")
        val aboutOff = builder.createString("about")
        val lnurlOff = if (lnurl != null) builder.createString(lnurl) else 0
        return FbProfileCard.createProfileCard(
            builder,
            pubkeyOff,
            displayName != null,
            dnOff,
            pictureUrl != null,
            pxOff,
            nip05Off,
            aboutOff,
            lnurl != null,
            lnurlOff,
        )
    }

    private fun resolvedBuffer(): ByteArray {
        val builder = FlatBufferBuilder(512)
        val keyA = builder.createString(hex(0x01))
        val cardA = profileCardOffset(builder, hex(0x01), "Alice", "https://a/p.png", "alice@ln")
        val entryA = ResolvedProfileEntry.createResolvedProfileEntry(builder, keyA, cardA)
        val keyB = builder.createString(hex(0x02))
        // displayName / pictureUrl / lnurl absent → has_* == false → null.
        val cardB = profileCardOffset(builder, hex(0x02), null, null, null)
        val entryB = ResolvedProfileEntry.createResolvedProfileEntry(builder, keyB, cardB)
        val entries = ResolvedProfilesSnapshot.createEntriesVector(builder, intArrayOf(entryA, entryB))
        val snap = ResolvedProfilesSnapshot.createResolvedProfilesSnapshot(builder, entries)
        ResolvedProfilesSnapshot.finishResolvedProfilesSnapshotBuffer(builder, snap)
        return builder.sizedByteArray()
    }

    private fun claimedBuffer(): ByteArray {
        val builder = FlatBufferBuilder(256)
        val key = builder.createString(hex(0x07))
        val card = profileCardOffset(builder, hex(0x07), "Carol", null, null)
        val entry = ClaimedProfileEntry.createClaimedProfileEntry(builder, key, card)
        val entries = ClaimedProfilesSnapshot.createEntriesVector(builder, intArrayOf(entry))
        val snap = ClaimedProfilesSnapshot.createClaimedProfilesSnapshot(builder, entries)
        ClaimedProfilesSnapshot.finishClaimedProfilesSnapshotBuffer(builder, snap)
        return builder.sizedByteArray()
    }

    @Test
    fun resolvedHappyPathMapsCardsAndPresenceFlags() {
        val map = requireNotNull(TypedProfilesDecoder.decodeResolvedBytes(resolvedBuffer())) {
            "valid KRPR buffer must decode"
        }
        assertEquals(setOf(hex(0x01), hex(0x02)), map.keys)
        val a = map.getValue(hex(0x01))
        assertEquals("Alice", a.displayName)
        assertEquals("https://a/p.png", a.pictureUrl)
        assertEquals("alice@ln", a.lnurl)
        val b = map.getValue(hex(0x02))
        // has_* == false round-trips to null (ADR-0032).
        assertNull(b.displayName)
        assertNull(b.pictureUrl)
        assertNull(b.lnurl)
    }

    @Test
    fun claimedHappyPathMapsSingleEntry() {
        val map = requireNotNull(TypedProfilesDecoder.decodeClaimedBytes(claimedBuffer()))
        assertEquals(setOf(hex(0x07)), map.keys)
        assertEquals("Carol", map.getValue(hex(0x07)).displayName)
    }

    @Test
    fun resolvedDecodeSelectsByKeyAndSchema() {
        val env = TypedProjectionEnvelope(
            key = TypedProfilesDecoder.RESOLVED_KEY,
            schemaId = TypedProfilesDecoder.RESOLVED_SCHEMA_ID,
            schemaVersion = 1u,
            fileIdentifier = TypedProfilesDecoder.RESOLVED_FILE_IDENTIFIER,
            payload = resolvedBuffer(),
        )
        val map = requireNotNull(TypedProfilesDecoder.decodeResolved(listOf(env)))
        assertTrue(map.containsKey(hex(0x01)))
    }

    @Test
    fun absentSidecarReturnsNull() {
        // Empty list → no envelope → null (caller falls back to generic).
        assertNull(TypedProfilesDecoder.decodeResolved(emptyList()))
        assertNull(TypedProfilesDecoder.decodeClaimed(emptyList()))
    }

    @Test
    fun wrongSchemaVersionReturnsNull() {
        val env = TypedProjectionEnvelope(
            key = TypedProfilesDecoder.RESOLVED_KEY,
            schemaId = TypedProfilesDecoder.RESOLVED_SCHEMA_ID,
            schemaVersion = 2u, // unsupported
            fileIdentifier = TypedProfilesDecoder.RESOLVED_FILE_IDENTIFIER,
            payload = resolvedBuffer(),
        )
        assertNull(TypedProfilesDecoder.decodeResolved(listOf(env)))
    }

    @Test
    fun malformedBufferReturnsNull() {
        val garbled = resolvedBuffer().copyOf()
        garbled[4] = 'X'.code.toByte() // clobber the KRPR file identifier
        assertNull(TypedProfilesDecoder.decodeResolvedBytes(garbled))
    }
}
