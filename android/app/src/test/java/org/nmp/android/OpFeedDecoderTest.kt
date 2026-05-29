package org.nmp.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Typed-decode tests for the `NOFS` OP-feed sidecar (ADR-0038 Stage T4 / B4).
 *
 * These pin the Android Kotlin FlatBuffers decoder ([TypedHomeFeedDecoder])
 * against the EXACT golden bytes B1 froze in
 * `crates/nmp-nip01/tests/fixtures/op_feed_{populated,empty}_v1.fb.hex`
 * (produced by `nmp_nip01::encode_op_feed_snapshot`). The hex is embedded inline
 * so the test is a pure JVM unit test — no Android framework, no asset wiring,
 * no instrumentation.
 *
 * PARITY CONTRACT: the typed decoder produces the OP-centric model the generic
 * `Value` path describes (cards + attribution + page), mirroring the iOS T3
 * `OpFeedDecoderTests`. The card fields the typed path cannot fill on Android
 * today — `contentTree` (embedded NFCT bytes; Android has no Kotlin NFCT
 * decoder) and relation counts (a typed sub-table Android does not model at
 * all) — are asserted/observed absent here to DOCUMENT the gap; a Kotlin NFCT
 * decoder is the follow-up that unblocks flipping the runtime preference (see
 * the [TypedHomeFeedDecoder] file header).
 */
class OpFeedDecoderTest {

    /**
     * 32-byte hex id from a single byte, mirroring the Rust fixture's
     * `hex32(byte)` helper (`"03"` -> `"0303…03"`, 64 chars).
     */
    private fun hex32(byte: Int): String = "%02x".format(byte and 0xff).repeat(32)

    private fun bytesFromHex(hex: String): ByteArray {
        val compact = hex.filterNot { it.isWhitespace() }
        require(compact.length % 2 == 0) { "hex fixture must contain whole bytes" }
        return ByteArray(compact.length / 2) { i ->
            compact.substring(i * 2, i * 2 + 2).toInt(16).toByte()
        }
    }

    // ── Populated fixture ──────────────────────────────────────────────────

    @Test
    fun populatedFixtureDecodesToParityModel() {
        val snapshot = requireNotNull(
            TypedHomeFeedDecoder.decode(bytesFromHex(POPULATED_HEX)),
        ) { "NOFS populated golden fixture must decode" }

        // Two root cards: a plain thread root (id 0x03) with two attributions,
        // and a repost-keyed root (id 0x09) with no attribution.
        assertEquals(2, snapshot.cards.size)

        val root = snapshot.cards[0]
        assertEquals(hex32(0x03), root.card.id)
        assertEquals(hex32(0x04), root.card.authorPubkey)
        assertEquals(1, root.card.kind)
        assertEquals(1_700_000_500L, root.card.createdAt)
        assertEquals("a thread root", root.card.content)
        assertEquals("a thread root", root.card.contentPreview)
        // root_card() has absent display mirrors (has_* == false).
        assertNull(root.card.authorDisplayName)
        assertNull(root.card.authorPictureUrl)
        // Documents the typed-path gap (see file header): the content tree stays
        // null on Android (no Kotlin NFCT decoder). The model has no relation-
        // counts field at all, by the same gap intent as the iOS decoder.
        assertNull(root.card.contentTree)

        // Attribution order is verbatim from the encoder (oldest-first).
        assertEquals(2, root.attribution.size)
        assertEquals(hex32(0x10), root.attribution[0].authorPubkey)
        // reply_event_id = hex32(byte + 0x80): 0x10 -> 0x90, 0x11 -> 0x91.
        assertEquals(hex32(0x90), root.attribution[0].replyEventId)
        assertEquals((1_700_000_900L + 0x10).toULong(), root.attribution[0].replyCreatedAt)
        assertEquals("Alice", root.attribution[0].authorDisplayName)
        assertEquals("https://example.com/a.png", root.attribution[0].authorPictureUrl)
        assertEquals(hex32(0x11), root.attribution[1].authorPubkey)
        assertEquals(hex32(0x91), root.attribution[1].replyEventId)
        // attribution(0x11, false): display mirrors absent.
        assertNull(root.attribution[1].authorDisplayName)
        assertNull(root.attribution[1].authorPictureUrl)

        val repost = snapshot.cards[1]
        assertEquals(hex32(0x09), repost.card.id)
        assertEquals(6, repost.card.kind)
        assertEquals("Alice", repost.card.authorDisplayName)
        assertEquals("https://example.com/a.png", repost.card.authorPictureUrl)
        assertTrue(repost.attribution.isEmpty())

        // Page reconstructed from the embedded NFWM feed-window sub-buffer.
        val page = requireNotNull(snapshot.page) { "populated fixture carries a FeedPage" }
        assertEquals(50UL, page.limit)
        assertTrue(page.hasMore)
        assertEquals(2UL, page.totalBlocks)
        val cursor = requireNotNull(page.nextCursor) { "FeedPage carries a next cursor" }
        assertEquals(hex32(0x09), cursor.id)
        assertEquals(1_700_000_000UL, cursor.createdAt)
    }

    // ── Empty fixture ───────────────────────────────────────────────────────

    @Test
    fun emptyFixtureDecodesToEmptyModel() {
        val snapshot = requireNotNull(
            TypedHomeFeedDecoder.decode(bytesFromHex(EMPTY_HEX)),
        ) { "NOFS empty golden fixture must decode" }
        assertTrue(snapshot.cards.isEmpty())
        assertNull("empty snapshot has no paging envelope", snapshot.page)
    }

    // ── Descriptor preference + graceful fallback (ADR-0037 Commitment 4) ────

    @Test
    fun nonOpfeedDescriptorIsIgnored() {
        // A retired NFTS-tagged envelope (or any non-opfeed schema id) is
        // unrecognized → null so the host falls back to the generic path.
        val envelope = TypedProjectionEnvelope(
            key = "nmp.feed.home",
            schemaId = "nmp.nip01.timeline",
            schemaVersion = 1u,
            fileIdentifier = "NFTS",
            payload = bytesFromHex(EMPTY_HEX),
        )
        assertNull(TypedHomeFeedDecoder.decode(listOf(envelope)))
    }

    @Test
    fun wrongFileIdentifierBytesFallBack() {
        // A buffer whose file identifier is not NOFS fails the identifier check
        // → null. Clobber the "NOFS" identifier region (offset 4).
        val garbled = bytesFromHex(EMPTY_HEX)
        garbled[4] = 'X'.code.toByte()
        assertNull(TypedHomeFeedDecoder.decode(garbled))
    }

    @Test
    fun emptyPayloadFallsBack() {
        assertNull(TypedHomeFeedDecoder.decode(ByteArray(0)))
    }

    companion object {
        // B1 fixtures, verbatim
        // (crates/nmp-nip01/tests/fixtures/op_feed_{empty,populated}_v1.fb.hex).
        private const val EMPTY_HEX =
            "100000004e4f46530800080000000400080000000400000000000000"

        private const val POPULATED_HEX =
            "1c0000004e4f46530000000000000e001000000008000c00060007000e00000000000101c400000004000000b8000000100000004e46574d08000e000400080008000000280000000c000000000006000c00040006000000d2040000000000000c001c000c000800070014000c000000000000011c000000320000000000000002000000000000000800100008000400080000000c00000000f1536500000000400000003039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303900000000020000007004000004000000fcf8ffff30000000040000000000000000002200440008000c0010001400380018001c00200024000600280007002c00300034002200000000000101b80100006c010000d403000006000000500100003c00000024000000e8020000240100000001000028010000e401000000f15365000000000000000074f9ffff0c000000040000000000000000000000d0000000140000004e46435400000a00100008000c0007000a000000000000021c00000004000000040000000000000001000000020000000300000004000000800000005c0000003c0000001000000000000a000c000700000008000a00000000000004040000001400000068747470733a2f2f6578616d706c652e636f6d2f00000000ccffffff0400000001000000200000000c000c0007000000000008000c0000000000000304000000050000006e6f737472000000080008000000040008000000040000000600000068656c6c6f2000001900000068747470733a2f2f6578616d706c652e636f6d2f612e706e6700000005000000416c6963650000000b00000068656c6c6f20776f726c64000b00000068656c6c6f20776f726c6400400000003032303230323032303230323032303230323032303230323032303230323032303230323032303230323032303230323032303230323032303230323032303200000000400000003039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303900001200240008000c00060010000700140018001200000000000101480000008c0000003400000010000000c0ae446500000000000000001900000068747470733a2f2f6578616d706c652e636f6d2f612e706e6700000005000000416c69636500000040000000343234323432343234323432343234323432343234323432343234323432343234323432343234323432343234323432343234323432343234323432343234320000000090fcffff000101013c00000028000000040000001900000068747470733a2f2f6578616d706c652e636f6d2f612e706e670000000a0000006e70756231616c696365000005000000416c6963650000000c001400040008000c0010000c000000c4000000300000001400000004000000ecf9ffff08000e000000040008000000010000000000000000000a000c000700000008000a00000000000001040000009efaffff5c00000010000000040000000100000065000000400000006161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616161616100000000150000006e6d702e7265616374696f6e732e73756d6d617279000000080010000000040008000000020000000000000000000000c4fdffff000101013c00000028000000040000001900000068747470733a2f2f6578616d706c652e636f6d2f612e706e670000000a0000006e70756231616c696365000005000000416c69636500000064fdffff680200000400000002000000f80000001800000014001c000400080000000000000000000c0010001400000060000000a40000001000000095f453650000000000000000400000003931393139313931393139313931393139313931393139313931393139313931393139313931393139313931393139313931393139313931393139313931393100000000400000003131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313131313100000000bcfbffff00000001040000000a0000006e707562316361726f6c00001400280008000c00060010000700140018001c00140000000000010194000000e800000038000000140000003c00000094f4536500000000000000001900000068747470733a2f2f6578616d706c652e636f6d2f612e706e6700000005000000416c696365000000400000003930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393039303930393000000000400000003130313031303130313031303130313031303130313031303130313031303130313031303130313031303130313031303130313031303130313031303130313000000000100014000500080006000c000700100010000000000101013c00000028000000040000001900000068747470733a2f2f6578616d706c652e636f6d2f612e706e670000000a0000006e70756231616c696365000005000000416c69636500000020003000040008000c0010002800140018001c00200000000000000000002400200000008c01000040010000b00200000100000020010000340000001c000000c4010000fc000000f4f253650000000008000c0004000800080000000c000000040000000000000000000000d0000000140000004e46435400000a00100008000c0007000a000000000000021c00000004000000040000000000000001000000020000000300000004000000800000005c0000003c0000001000000000000a000c000700000008000a00000000000004040000001400000068747470733a2f2f6578616d706c652e636f6d2f00000000ccffffff0400000001000000200000000c000c0007000000000008000c0000000000000304000000050000006e6f737472000000080008000000040008000000040000000600000068656c6c6f2000000d000000612074687265616420726f6f740000000d000000612074687265616420726f6f740000004000000030343034303430343034303430343034303430343034303430343034303430343034303430343034303430343034303430343034303430343034303430343034000000004000000030333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033000000000c001600040008000c0010000c000000bc000000ac000000a00000001000000000000a000e000700000008000a000000000000011000000000000a001000040008000c000a0000005c000000100000000400000001000000650000004000000030333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033303330333033000000000e0000006e6d702e6e697035372e7a6170730000fcffffff040004000400000008000c00000004000800000001000000000000000c000c0000000000070008000c0000000000000104000000080000006e70756231626f6200000000"
    }
}
