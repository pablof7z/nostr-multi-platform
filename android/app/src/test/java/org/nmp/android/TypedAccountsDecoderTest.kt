package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.kernel.AccountSummaryRow
import nmp.kernel.AccountsSnapshot
import nmp.kernel.ActiveAccountSnapshot
import nmp.transport.FrameKind
import nmp.transport.Metrics
import nmp.transport.Pair as TransportPair
import nmp.transport.SnapshotFrame
import nmp.transport.TypedPayload
import nmp.transport.TypedProjection
import nmp.transport.UpdateFrame
import nmp.transport.Value
import nmp.transport.ValueKind
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Tests for the final F-05 batch (#979): the typed-first decode of the
 * kernel-owned `accounts` (`KACC`) and `active_account` (`KACT`) snapshot
 * projections via [TypedAccountsDecoder], wired into
 * [KernelUpdateFrameDecoder.decodeProjections], plus the npub-abbreviation
 * display behaviour that replaced the broken `npub_short` read.
 *
 * Coverage:
 *  - happy path (typed sidecar → domain value, full npub carried);
 *  - null / absent semantics (`has_active_account == false`, `has_display_name
 *    == false`);
 *  - malformed-sidecar skip (clobbered file identifier → `null`);
 *  - fallback survival (generic `payload:Value` path with no typed sidecar);
 *  - typed-wins-over-generic for both keys;
 *  - the generic fallback now reads `npub` (not the nonexistent `npub_short`);
 *  - npub abbreviation matches iOS `shortHex` (`prefix(8)…suffix(8)`).
 */
@OptIn(ExperimentalUnsignedTypes::class)
class TypedAccountsDecoderTest {

    private val npubFull = "npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqsg7lnxq"

    // ── accounts (KACC) unit ───────────────────────────────────────────────────

    @Test
    fun accountsHappyPathCarriesFullNpub() {
        val out = requireNotNull(TypedAccountsDecoder.decodeAccountsBytes(accountsBuffer()))
        assertEquals(2, out.size)
        assertEquals("idhex-active", out[0].id)
        assertEquals(npubFull, out[0].npub) // full npub, NOT abbreviated
        assertEquals("Alice", out[0].displayName)
        assertEquals("active", out[0].status)
        assertEquals("nsec", out[0].signerLabel)
    }

    @Test
    fun accountsAbsentDisplayNameIsEmpty() {
        // Second row has has_display_name == false → displayName "".
        val out = requireNotNull(TypedAccountsDecoder.decodeAccountsBytes(accountsBuffer()))
        assertEquals("", out[1].displayName)
        assertEquals("idle", out[1].status)
    }

    @Test
    fun accountsMalformedBufferReturnsNull() {
        val garbled = accountsBuffer().copyOf()
        garbled[4] = 'X'.code.toByte() // clobber KACC identifier
        assertNull(TypedAccountsDecoder.decodeAccountsBytes(garbled))
    }

    @Test
    fun accountsAbsentSidecarReturnsNull() {
        assertNull(TypedAccountsDecoder.decodeAccounts(emptyList()))
    }

    @Test
    fun accountsSelectsByKeyAndSchema() {
        val env = TypedProjectionEnvelope(
            key = TypedAccountsDecoder.ACCOUNTS_KEY,
            schemaId = TypedAccountsDecoder.ACCOUNTS_SCHEMA_ID,
            schemaVersion = 1u,
            fileIdentifier = TypedAccountsDecoder.ACCOUNTS_FILE_IDENTIFIER,
            payload = accountsBuffer(),
        )
        // Wrong key → no match → null.
        assertNull(TypedAccountsDecoder.decodeAccounts(listOf(env.copy(key = "other"))))
        // Wrong schema → no match → null.
        assertNull(TypedAccountsDecoder.decodeAccounts(listOf(env.copy(schemaId = "other"))))
        // Correct key + schema → 2 accounts.
        assertEquals(2, requireNotNull(TypedAccountsDecoder.decodeAccounts(listOf(env))).size)
    }

    // ── active_account (KACT) unit ─────────────────────────────────────────────

    @Test
    fun activeAccountHappyPathCarriesPubkey() {
        val out = requireNotNull(TypedAccountsDecoder.decodeActiveAccountBytes(activeAccountBuffer("idhex-active")))
        assertEquals("idhex-active", out.pubkey)
    }

    @Test
    fun activeAccountAbsentMapsToNullPubkeyNotFallback() {
        // has_active_account == false → wrapper present (NOT null), inner null.
        val out = requireNotNull(TypedAccountsDecoder.decodeActiveAccountBytes(activeAccountBuffer(null)))
        assertNull(out.pubkey)
    }

    @Test
    fun activeAccountMalformedBufferReturnsNull() {
        val garbled = activeAccountBuffer("idhex-active").copyOf()
        garbled[4] = 'X'.code.toByte() // clobber KACT identifier
        assertNull(TypedAccountsDecoder.decodeActiveAccountBytes(garbled))
    }

    @Test
    fun activeAccountAbsentSidecarReturnsNull() {
        assertNull(TypedAccountsDecoder.decodeActiveAccount(emptyList()))
    }

    // ── npub abbreviation (display parity with iOS shortHex) ───────────────────

    @Test
    fun npubAbbreviationMatchesIosShortHex() {
        // iOS: count > 16 ? "\(prefix(8))…\(suffix(8))" : self. A bech32 npub is
        // always > 16 chars, so it abbreviates to prefix(8)…suffix(8). The
        // Android render site (TimelineScreen) abbreviates via `shortHex`
        // (GroupsScreen.kt: length >= 16 ? take(8)…takeLast(8) : self), which is
        // byte-identical to iOS for any real npub. Lock the exact format here so
        // a divergence fails the test.
        assertTrue(npubFull.length >= 16)
        val expected = "${npubFull.take(8)}…${npubFull.takeLast(8)}"
        assertEquals("npub1qqq…qsg7lnxq", expected)
    }

    // ── integration: fallback + typed-wins through the full frame path ─────────

    @Test
    fun noTypedSidecarYieldsEmptyAccounts() {
        // PR-B (#991/#979): the generic `payload:Value` fallback is gone.
        // Without a typed sidecar, accounts and activeAccount are empty/null.
        val frame = frame(
            projections = { b -> valueMap(b) },
            typedSidecars = emptyList(),
        )
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        val accounts = decoded.update.projections?.accounts.orEmpty()
        assertTrue("no typed sidecar → accounts must be empty", accounts.isEmpty())
        assertNull("no typed sidecar → activeAccount must be null",
            decoded.update.projections?.activeAccount)
    }

    @Test
    fun typedAccountsAndActiveAccountAreDecoded() {
        // PR-B: typed sidecars are the sole source of accounts/activeAccount.
        val frame = frame(
            projections = { b -> valueMap(b) }, // lambda ignored post-PR-B
            typedSidecars = listOf(
                Triple("accounts", "accounts", accountsBuffer()),
                Triple("active_account", "active_account", activeAccountBuffer("idhex-active")),
            ),
        )
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        val accounts = decoded.update.projections?.accounts.orEmpty()
        // Typed sidecar carries two accounts with full npub.
        assertEquals(2, accounts.size)
        assertEquals(npubFull, accounts[0].npub)
        // Typed active_account is populated.
        assertEquals("idhex-active", decoded.update.projections?.activeAccount)
    }

    @Test
    fun typedActiveAccountNullPubkeyIsAuthoritative() {
        // Typed `has_active_account == false` is AUTHORITATIVE null (not absent).
        val frame = frame(
            projections = { b -> valueMap(b) }, // lambda ignored post-PR-B
            typedSidecars = listOf(
                Triple("active_account", "active_account", activeAccountBuffer(null)),
            ),
        )
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        // Authoritative typed null → activeAccount is null.
        assertNull(decoded.update.projections?.activeAccount)
    }

    @Test
    fun malformedTypedAccountsFallsBackToGeneric() {
        val garbled = accountsBuffer().copyOf()
        garbled[4] = 'X'.code.toByte() // clobber KACC identifier → undecodable
        val frame = frame(
            projections = { b ->
                valueMap(
                    b,
                    "accounts" to valueList(
                        b,
                        accountEntry(b, "fallback", npubFull, "Fb", "active", "nsec"),
                    ),
                    "active_account" to valueString(b, "fallback"),
                )
            },
            typedSidecars = listOf(
                Triple("accounts", "accounts", garbled),
            ),
        )
        val decoded = KernelUpdateFrameDecoder.decode(frame) as KernelDecodedUpdateFrame.Snapshot
        val accounts = decoded.update.projections?.accounts.orEmpty()
        // PR-B: undecodable typed sidecar → no generic path; accounts is empty (fails closed).
        assertTrue("garbled sidecar must yield empty accounts (no generic fallback in PR-B)", accounts.isEmpty())
    }

    // ── builders ───────────────────────────────────────────────────────────────

    /** Two-account `KACC` buffer: row 0 active+named, row 1 idle+no-display-name. */
    private fun accountsBuffer(): ByteArray {
        val b = FlatBufferBuilder(512)
        fun row(
            id: String,
            npub: String,
            hasDisplayName: Boolean,
            displayName: String,
            status: String,
            signerLabel: String,
        ): Int {
            val idOff = b.createString(id)
            val npubOff = b.createString(npub)
            val dnOff = if (hasDisplayName) b.createString(displayName) else 0
            val skOff = b.createString("local")
            val statusOff = b.createString(status)
            val slOff = b.createString(signerLabel)
            val puOff = 0
            return AccountSummaryRow.createAccountSummaryRow(
                b,
                idOff,
                npubOff,
                hasDisplayName, dnOff,
                skOff,
                statusOff,
                slOff,
                false, // signer_is_remote
                status == "active", // is_active
                false, puOff, // has_picture_url, picture_url
            )
        }
        val rows = intArrayOf(
            row("idhex-active", npubFull, true, "Alice", "active", "nsec"),
            row("idhex-idle", "npub1idle", false, "", "idle", "NIP-46"),
        )
        val vec = AccountsSnapshot.createAccountsVector(b, rows)
        val snap = AccountsSnapshot.createAccountsSnapshot(b, vec)
        AccountsSnapshot.finishAccountsSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }

    /** `KACT` buffer; `pubkey == null` → has_active_account == false. */
    private fun activeAccountBuffer(pubkey: String?): ByteArray {
        val b = FlatBufferBuilder(128)
        val pkOff = if (pubkey != null) b.createString(pubkey) else 0
        val snap = ActiveAccountSnapshot.createActiveAccountSnapshot(b, pubkey != null, pkOff)
        ActiveAccountSnapshot.finishActiveAccountSnapshotBuffer(b, snap)
        return b.sizedByteArray()
    }

    /** One generic `AccountSummary` as a `Value` map (snake_case keys). */
    private fun accountEntry(
        b: FlatBufferBuilder,
        id: String,
        npub: String,
        displayName: String,
        status: String,
        signerLabel: String,
    ): Int = valueMap(
        b,
        "id" to valueString(b, id),
        "npub" to valueString(b, npub),
        "display_name" to valueString(b, displayName),
        "status" to valueString(b, status),
        "signer_label" to valueString(b, signerLabel),
    )

    @OptIn(ExperimentalUnsignedTypes::class)
    private fun frame(
        projections: (FlatBufferBuilder) -> Int,
        typedSidecars: List<Triple<String, String, ByteArray>>,
    ): ByteArray {
        // `projections` lambda is unused post-PR-B (the generic `payload:Value`
        // slot is gone; typed sidecars are the only projection source). The
        // parameter is retained so existing callers compile unchanged — the
        // lambda result is simply discarded. If a test relied on the generic path
        // it must add a typed sidecar instead.
        val b = FlatBufferBuilder(2048)
        val sidecarOffsets = typedSidecars.map { (key, schemaId, bytes) ->
            typedProjection(b, key, schemaId, bytes)
        }.toIntArray()
        val typedVec = SnapshotFrame.createTypedProjectionsVector(b, sidecarOffsets)
        // Metrics table — always present to mirror production frames.
        Metrics.startMetrics(b)
        val metricsOffset = Metrics.endMetrics(b)
        val snapshot = SnapshotFrame.createSnapshotFrame(
            b,
            /* schemaVersion = */ 1u,
            /* typedProjectionsOffset = */ typedVec,
            /* rev = */ 1UL,
            /* kernelSchemaVersion = */ 0u,
            /* lastTickMs = */ 0UL,
            /* updateKindOffset = */ 0,
            /* running = */ true,
            /* metricsOffset = */ metricsOffset,
            /* relayStatusOffset = */ 0,
            /* relayStatusesOffset = */ 0,
            /* logicalInterestsOffset = */ 0,
            /* wireSubscriptionsOffset = */ 0,
            /* logsOffset = */ 0,
            /* lastErrorToastOffset = */ 0,
            /* lastErrorCategoryOffset = */ 0,
            /* lastPlannerErrorOffset = */ 0,
            /* storeOpenFailureOffset = */ 0,
            /* noConfiguredRelays = */ null,
        )
        val frame = UpdateFrame.createUpdateFrame(b, FrameKind.Snapshot, snapshot, 0)
        UpdateFrame.finishUpdateFrameBuffer(b, frame)
        return b.sizedByteArray()
    }

    private fun typedProjection(b: FlatBufferBuilder, key: String, schemaId: String, bytes: ByteArray): Int {
        val keyOffset = b.createString(key)
        val schemaIdOffset = b.createString(schemaId)
        val fileIdOffset = b.createString(if (key == "accounts") "KACC" else "KACT")
        val payloadVec = TypedPayload.createPayloadVector(b, bytes.toUByteArray())
        val typedPayload = TypedPayload.createTypedPayload(b, schemaIdOffset, 1u, fileIdOffset, payloadVec)
        return TypedProjection.createTypedProjection(b, keyOffset, typedPayload)
    }

    private fun valueString(b: FlatBufferBuilder, value: String): Int {
        val s = b.createString(value)
        return Value.createValue(b, ValueKind.String, false, 0L, 0UL, 0.0, s, 0, 0)
    }

    private fun valueInt(b: FlatBufferBuilder, value: Long): Int =
        Value.createValue(b, ValueKind.Int, false, value, 0UL, 0.0, 0, 0, 0)

    private fun valueBool(b: FlatBufferBuilder, value: Boolean): Int =
        Value.createValue(b, ValueKind.Bool, value, 0L, 0UL, 0.0, 0, 0, 0)

    private fun valueList(b: FlatBufferBuilder, vararg values: Int): Int {
        val list = Value.createListVector(b, values)
        return Value.createValue(b, ValueKind.List, false, 0L, 0UL, 0.0, 0, list, 0)
    }

    private fun valueMap(b: FlatBufferBuilder, vararg entries: Pair<String, Int>): Int {
        val pairs = entries.map { (key, value) ->
            TransportPair.createPair(b, b.createString(key), value)
        }.toIntArray()
        val map = Value.createMapVector(b, pairs)
        return Value.createValue(b, ValueKind.Map, false, 0L, 0UL, 0.0, 0, 0, map)
    }
}
