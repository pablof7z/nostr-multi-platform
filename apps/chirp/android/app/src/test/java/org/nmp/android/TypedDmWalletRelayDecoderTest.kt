package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.kernel.RelayRoleOption as FbRelayRoleOption
import nmp.kernel.RelayRoleOptionsSnapshot
import nmp.nip17.DmConversation as FbDmConversation
import nmp.nip17.DmInboxSnapshot as FbDmInboxSnapshot
import nmp.nip17.DmMessage as FbDmMessage
import nmp.nip47.WalletStatus as FbWalletStatus
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Contract tests for the NIP-17 [TypedDmInboxDecoder], NIP-47
 * [TypedWalletDecoder], and kernel [TypedRelayRoleOptionsDecoder] (F-05 / #979).
 * Each: happy-path mapping, presence-flag null semantics, envelope selection,
 * and malformed/absent sidecar → `null` (caller falls back to generic).
 */
class TypedDmWalletRelayDecoderTest {

    private fun hex(b: Int): String = "%02x".format(b and 0xff).repeat(32)

    // ── DM inbox ─────────────────────────────────────────────────────────────

    private fun dmBuffer(): ByteArray {
        val builder = FlatBufferBuilder(512)
        val id = builder.createString(hex(0x11))
        val sender = builder.createString(hex(0x12))
        val content = builder.createString("hi there")
        val relayUrl = builder.createString("wss://relay.example")
        val relays = FbDmMessage.createSourceRelaysVector(builder, intArrayOf(relayUrl))
        // present message, no reply.
        val msg = FbDmMessage.createDmMessage(
            builder,
            id,
            sender,
            content,
            1_700_000_000UL,
            false, // has_reply_to
            0,
            true, // is_outgoing
            relays,
        )
        val msgsVec = FbDmConversation.createMessagesVector(builder, intArrayOf(msg))
        val peer = builder.createString(hex(0x12))
        val conv = FbDmConversation.createDmConversation(builder, peer, msgsVec)
        val convVec = FbDmInboxSnapshot.createConversationsVector(builder, intArrayOf(conv))
        // §D7: decrypt_state "ok" (a settled, signed-in account), no pending.
        val decryptState = builder.createString("ok")
        val snap = FbDmInboxSnapshot.createDmInboxSnapshot(builder, convVec, decryptState, 0)
        FbDmInboxSnapshot.finishDmInboxSnapshotBuffer(builder, snap)
        return builder.sizedByteArray()
    }

    @Test
    fun dmHappyPathMapsConversationAndMessage() {
        val inbox = requireNotNull(TypedDmInboxDecoder.decode(dmBuffer()))
        assertEquals("ok", inbox.decryptState)
        assertEquals(0, inbox.undecryptedCount)
        val conv = inbox.conversations.single()
        assertEquals(hex(0x12), conv.peerPubkey)
        val msg = conv.messages.single()
        assertEquals(hex(0x11), msg.id)
        assertEquals("hi there", msg.content)
        assertEquals(1_700_000_000L, msg.createdAt)
        assertTrue(msg.isOutgoing)
        assertNull(msg.replyTo) // has_reply_to == false → null
        assertEquals(listOf("wss://relay.example"), msg.sourceRelays)
    }

    @Test
    fun dmDecodeSelectsByKeyAndSchema() {
        val env = TypedProjectionEnvelope(
            key = TypedDmInboxDecoder.KEY,
            schemaId = TypedDmInboxDecoder.SCHEMA_ID,
            schemaVersion = 1u,
            fileIdentifier = TypedDmInboxDecoder.FILE_IDENTIFIER,
            payload = dmBuffer(),
        )
        assertEquals(1, requireNotNull(TypedDmInboxDecoder.decode(listOf(env))).conversations.size)
        assertNull(TypedDmInboxDecoder.decode(emptyList()))
    }

    @Test
    fun dmMalformedBufferReturnsNull() {
        val garbled = dmBuffer().copyOf()
        garbled[4] = 'X'.code.toByte() // clobber NDMI identifier
        assertNull(TypedDmInboxDecoder.decode(garbled))
    }

    // ── wallet ───────────────────────────────────────────────────────────────

    // RAW-DATA DOCTRINE (aim.md §2 / ADR-0032): the wire carries only the raw
    // `status` token + raw `balance_sats`. The decoder derives label/tone and
    // formats the balance shell-side — no presentation strings on the buffer.
    private fun walletBuffer(
        balanceSats: ULong?,
        wireStatus: String = "ready",
        isReady: Boolean = true,
        isConnected: Boolean = true,
    ): ByteArray {
        val builder = FlatBufferBuilder(256)
        val status = builder.createString(wireStatus)
        val relayUrl = builder.createString("wss://nwc.example")
        val npub = builder.createString("npub1wallet")
        val pkHex = builder.createString(hex(0x44))
        val w = FbWalletStatus.createWalletStatus(
            builder,
            status,
            relayUrl,
            npub,
            false, 0UL, // msats
            balanceSats != null, balanceSats ?: 0UL, // sats
            isReady, isConnected, // is_ready, is_connected
            false, 0u, // connection_state
            pkHex,
        )
        FbWalletStatus.finishWalletStatusBuffer(builder, w)
        return builder.sizedByteArray()
    }

    @Test
    fun walletHappyPathMapsStatusAndFormatsBalanceShellSide() {
        val out = requireNotNull(TypedWalletDecoder.decode(walletBuffer(1_234UL)))
        assertEquals("ready", out.status)
        // The shell formats the raw balance_sats with thousands separators.
        assertEquals("1,234", out.balanceDisplay)
    }

    @Test
    fun walletAbsentBalanceIsNull() {
        val out = requireNotNull(TypedWalletDecoder.decode(walletBuffer(null)))
        assertEquals("ready", out.status)
        assertNull(out.balanceDisplay) // has_balance_sats == false → null
    }

    @Test
    fun walletSurfacesRustIsConnectedVerbatim() {
        // #1493 P4: the shell binds the Rust-computed `is_connected` flag rather
        // than re-deriving connectedness from `statusTone` in Kotlin (native
        // branch on a wire discriminant; D7).
        val connected = requireNotNull(
            TypedWalletDecoder.decode(
                walletBuffer(balanceSats = 5UL, wireStatus = "ready", isConnected = true),
            ),
        )
        assertTrue(connected.isConnected)

        val disconnected = requireNotNull(
            TypedWalletDecoder.decode(
                walletBuffer(
                    balanceSats = null,
                    wireStatus = "disconnected",
                    isReady = false,
                    isConnected = false,
                ),
            ),
        )
        assertFalse(disconnected.isConnected)
    }

    @Test
    fun walletDerivesLabelAndToneFromRawStatusToken() {
        // RAW-DATA DOCTRINE: label/tone are derived shell-side from the raw wire
        // status token (parity with iOS WalletStatusTone), never read off the
        // buffer.
        val ready = requireNotNull(TypedWalletDecoder.decode(walletBuffer(5UL, "ready")))
        assertEquals("Ready", ready.statusLabel)
        assertEquals("active", ready.statusTone)

        val connecting = requireNotNull(
            TypedWalletDecoder.decode(walletBuffer(null, "connecting")),
        )
        assertEquals("Connecting", connecting.statusLabel)
        assertEquals("warning", connecting.statusTone)

        val errored = requireNotNull(
            TypedWalletDecoder.decode(walletBuffer(null, "error")),
        )
        assertEquals("Error", errored.statusLabel)
        assertEquals("error", errored.statusTone)
    }

    @Test
    fun walletDecodeSelectsByKeyAndSchema() {
        val env = TypedProjectionEnvelope(
            key = TypedWalletDecoder.KEY,
            schemaId = TypedWalletDecoder.SCHEMA_ID, // "nmp.nip47.wallet" (≠ key)
            schemaVersion = 1u,
            fileIdentifier = TypedWalletDecoder.FILE_IDENTIFIER,
            payload = walletBuffer(9UL),
        )
        assertEquals("9", requireNotNull(TypedWalletDecoder.decode(listOf(env))).balanceDisplay)
        // Wrong schema id (matching key only) → no match.
        assertNull(
            TypedWalletDecoder.decode(
                listOf(env.copy(schemaId = "wallet")),
            ),
        )
    }

    @Test
    fun walletMalformedBufferReturnsNull() {
        val garbled = walletBuffer(1UL).copyOf()
        garbled[4] = 'Z'.code.toByte() // clobber NWST identifier
        assertNull(TypedWalletDecoder.decode(garbled))
    }

    // ── relay role options ───────────────────────────────────────────────────

    private fun relayRoleBuffer(): ByteArray {
        val builder = FlatBufferBuilder(256)
        fun opt(value: String, tint: String, isDefault: Boolean): Int {
            val v = builder.createString(value)
            val t = builder.createString(tint)
            return FbRelayRoleOption.createRelayRoleOption(builder, v, t, isDefault)
        }
        val o1 = opt("both", "accent", true)
        val o2 = opt("read", "info", false)
        val vec = RelayRoleOptionsSnapshot.createOptionsVector(builder, intArrayOf(o1, o2))
        val snap = RelayRoleOptionsSnapshot.createRelayRoleOptionsSnapshot(builder, vec)
        RelayRoleOptionsSnapshot.finishRelayRoleOptionsSnapshotBuffer(builder, snap)
        return builder.sizedByteArray()
    }

    @Test
    fun relayRoleHappyPathPreservesOrderAndDefault() {
        val opts = requireNotNull(TypedRelayRoleOptionsDecoder.decode(relayRoleBuffer()))
        assertEquals(listOf("both", "read"), opts.map { it.value })
        assertEquals("Both", opts[0].label)
        assertEquals("accent", opts[0].tint)
        assertTrue(opts[0].isDefault)
        assertFalse(opts[1].isDefault)
    }

    @Test
    fun relayRoleDecodeSelectsAndFallsBack() {
        val env = TypedProjectionEnvelope(
            key = TypedRelayRoleOptionsDecoder.KEY,
            schemaId = TypedRelayRoleOptionsDecoder.SCHEMA_ID,
            schemaVersion = 1u,
            fileIdentifier = TypedRelayRoleOptionsDecoder.FILE_IDENTIFIER,
            payload = relayRoleBuffer(),
        )
        assertEquals(2, requireNotNull(TypedRelayRoleOptionsDecoder.decode(listOf(env))).size)
        assertNull(TypedRelayRoleOptionsDecoder.decode(emptyList()))
    }

    @Test
    fun relayRoleMalformedBufferReturnsNull() {
        val garbled = relayRoleBuffer().copyOf()
        garbled[4] = 'Q'.code.toByte() // clobber KRRO identifier
        assertNull(TypedRelayRoleOptionsDecoder.decode(garbled))
    }
}
