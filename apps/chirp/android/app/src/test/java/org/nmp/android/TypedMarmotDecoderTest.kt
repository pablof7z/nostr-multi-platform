package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.marmot.KeyPackageStatus
import nmp.marmot.LastOpError as FbLastOpError
import nmp.marmot.MarmotGroupMessages
import nmp.marmot.MarmotGroupRow
import nmp.marmot.MarmotMessageRow
import nmp.marmot.MarmotMessages as FbMarmotMessages
import nmp.marmot.MarmotSnapshot as FbMarmotSnapshot
import nmp.marmot.PendingOpRow as FbPendingOpRow
import nmp.marmot.PendingWelcomeRow
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Contract tests for [TypedMarmotDecoder] (F-05 / #979): the `NMMS`
 * `nmp.marmot.snapshot` and `NMMG` `nmp.marmot.messages` push sidecars decode
 * into [org.nmp.android.model.MarmotSnapshot] / the group-keyed message map,
 * with `has_*` companion bools reproducing null semantics, schema-version
 * gating, and malformed/absent → `null` (caller falls back to generic).
 *
 * Snapshot schema-version coverage:
 *   v1 — original shape (no pending_ops / last_op_error)
 *   v2 — adds pending_ops:[PendingOpRow] (incl. age_secs) + structured
 *        last_op_error:LastOpError {op, reason, at_secs, correlation_id}
 *   v4 — removes display_name/initials/invites_chip_label/display_label
 *        (presentation fields moved to shells; aim.md §2)
 *   v99 — unknown version → decoder fails closed (returns null)
 * Messages wire is unchanged at v1; v2+ on that wire still fails closed.
 */
class TypedMarmotDecoderTest {

    private fun hex(b: Int): String = "%02x".format(b and 0xff).repeat(32)

    /**
     * Build a minimal but complete NMMS buffer using the v4 schema (no
     * presentation fields). Raw data only: name, group_name, op_tag,
     * missing_count, age_secs are the wire shape (aim.md §2).
     *
     * [pendingOpCid] — when non-null, one structured [FbPendingOpRow]
     * (incl. [pendingOpAgeSecs]) is embedded.
     * [lastOpErrorReason] — when non-null, a [FbLastOpError] table is
     * embedded (with [lastOpErrorOp]); otherwise the table is absent (offset 0).
     */
    private fun snapshotBuffer(
        schemaVersion: UInt = 5u,
        pendingOpCid: String? = null,
        pendingOpAgeSecs: ULong = 0u,
        lastOpErrorOp: String = "create_group",
        lastOpErrorReason: String? = null,
        initErrorKind: String = "",
        initErrorDetail: String = "",
    ): ByteArray {
        val builder = FlatBufferBuilder(1024)
        // group: one member, unread present, last_msg present.
        // v4: no display_name / initials — shells compute those from `name`.
        val gId = builder.createString(hex(0x01))
        val gName = builder.createString("Team")
        val member = builder.createString(hex(0x02))
        val members = MarmotGroupRow.createMembersVector(builder, intArrayOf(member))
        val group = MarmotGroupRow.createMarmotGroupRow(
            builder, gId, gName, members,
            1u, // member_count
            true, 4u, // has_unread_count / unread_count
            true, 1_700_000_500UL, // has_last_msg_at / last_msg_at
        )
        val groups = FbMarmotSnapshot.createGroupsVector(builder, intArrayOf(group))

        // pending welcome.
        // v4: no display_name — shells apply "Group invite" fallback from group_name.
        val wId = builder.createString(hex(0x03))
        val wGroup = builder.createString("Invite")
        val wInviter = builder.createString(hex(0x04))
        val welcome = PendingWelcomeRow.createPendingWelcomeRow(builder, wId, wGroup, wInviter)
        val welcomes = FbMarmotSnapshot.createPendingWelcomesVector(builder, intArrayOf(welcome))

        // key package: published, no d_tag, age present. Raw fields only
        // (aim.md §2): shells derive subtitle / action label from these.
        val kp = KeyPackageStatus.createKeyPackageStatus(
            builder,
            true, // published
            false, 0, // has_d_tag
            true, 432_000UL, // has_age_secs / age_secs
            true, // stale
            true, // is_registered
        )

        val cached = builder.createString(hex(0x05))
        val cachedVec = FbMarmotSnapshot.createCachedKpPubkeysVector(builder, intArrayOf(cached))

        // v2 pending_ops:[PendingOpRow] (incl. age_secs). Empty when no op given.
        // v4: no display_label — shells compute from op_tag + missing_count.
        val pendingOpsVec: Int = if (pendingOpCid != null) {
            val cidOff = builder.createString(pendingOpCid)
            val tagOff = builder.createString("create_group")
            val opOff = FbPendingOpRow.createPendingOpRow(
                builder, cidOff, tagOff, 1u, pendingOpAgeSecs,
            )
            FbMarmotSnapshot.createPendingOpsVector(builder, intArrayOf(opOff))
        } else {
            FbMarmotSnapshot.createPendingOpsVector(builder, IntArray(0))
        }

        // v2 last_op_error:LastOpError table (absent → offset 0).
        val lastOpErrorOff: Int = if (lastOpErrorReason != null) {
            val opOff = builder.createString(lastOpErrorOp)
            val reasonOff = builder.createString(lastOpErrorReason)
            val cidOff = builder.createString(hex(0x06))
            FbLastOpError.createLastOpError(builder, opOff, reasonOff, 1_700_000_900UL, cidOff)
        } else {
            0
        }

        // v4: no has_invites_chip_label / invites_chip_label — shells compute
        // plural label from pendingWelcomes.size (aim.md §2).
        // v5 (#1651): init_error_kind / init_error_detail raw tokens (empty = none).
        val initErrorKindOff = builder.createString(initErrorKind)
        val initErrorDetailOff = builder.createString(initErrorDetail)
        val snap = FbMarmotSnapshot.createMarmotSnapshot(
            builder, schemaVersion, groups, welcomes, kp, cachedVec,
            true, // is_registered
            0u, // orphaned_commit_count
            initErrorKindOff, // init_error_kind ("" = no init error)
            initErrorDetailOff, // init_error_detail
            pendingOpsVec, // pending_ops
            lastOpErrorOff, // last_op_error (table offset; 0 = absent)
        )
        FbMarmotSnapshot.finishMarmotSnapshotBuffer(builder, snap)
        return builder.sizedByteArray()
    }

    private fun messagesBuffer(schemaVersion: UInt = 1u): ByteArray {
        val builder = FlatBufferBuilder(512)
        val id = builder.createString(hex(0x21))
        val sender = builder.createString(hex(0x22))
        val content = builder.createString("gm")
        val row = MarmotMessageRow.createMarmotMessageRow(
            builder, id, sender, content, 1_700_000_900UL,
            true, 7UL, // has_epoch / epoch
        )
        val rows = MarmotGroupMessages.createMessagesVector(builder, intArrayOf(row))
        val groupKey = builder.createString(hex(0x01))
        val groupMsgs = MarmotGroupMessages.createMarmotGroupMessages(builder, groupKey, rows)
        val groups = FbMarmotMessages.createGroupsVector(builder, intArrayOf(groupMsgs))
        val msgs = FbMarmotMessages.createMarmotMessages(builder, schemaVersion, groups)
        FbMarmotMessages.finishMarmotMessagesBuffer(builder, msgs)
        return builder.sizedByteArray()
    }

    // ── snapshot happy path ──────────────────────────────────────────────────

    @Test
    fun snapshotHappyPathMapsAllSubtables() {
        val snap = requireNotNull(TypedMarmotDecoder.decodeSnapshot(snapshotBuffer()))
        val group = snap.groups.single()
        assertEquals(hex(0x01), group.idHex)
        // v4: raw name on the wire; shell computes displayName / initials.
        assertEquals("Team", group.name)
        assertEquals("Team", group.displayName) // shell-computed fallback
        assertEquals("TE", group.initials)      // shell-computed 2-char initials
        assertEquals(1, group.memberCount)
        assertEquals(listOf(hex(0x02)), group.members)
        assertEquals(4, group.unreadCount) // has_unread_count == true
        assertEquals(1_700_000_500L, group.lastMsgAt)

        val welcome = snap.pendingWelcomes.single()
        assertEquals(hex(0x03), welcome.idHex)
        // v4: raw group_name; shell computes displayName.
        assertEquals("Invite", welcome.groupName)
        assertEquals("Invite", welcome.displayName) // shell-computed fallback

        assertTrue(snap.keyPackage.published)
        assertNull(snap.keyPackage.dTag) // has_d_tag == false → null
        assertEquals(432_000L, snap.keyPackage.ageSecs)
        assertTrue(snap.keyPackage.isRegistered)
        assertEquals(listOf(hex(0x05)), snap.cachedKpPubkeys)
        // v4: no invites_chip_label on wire; computed by shell from pendingWelcomes.size.
        assertEquals("1 invite", snap.invitesChipLabel)
        assertTrue(snap.isRegistered)

        // v1 buffer: pending_ops empty, last_op_error absent → null.
        assertTrue(snap.pendingOps.isEmpty())
        assertNull(snap.lastOpError)
    }

    // ── schema v2/v4 structured fields ──────────────────────────────────────

    @Test
    fun schemaV2PendingOpsDecodeWithAgeSecs() {
        val cid = "corr-id-abc123"
        val snap = requireNotNull(
            TypedMarmotDecoder.decodeSnapshot(
                snapshotBuffer(
                    schemaVersion = 2u,
                    pendingOpCid = cid,
                    pendingOpAgeSecs = 42u,
                )
            )
        )
        val op = snap.pendingOps.single()
        assertEquals(cid, op.correlationId)
        assertEquals("create_group", op.opTag)
        assertEquals(1, op.missingCount)
        // v4: no display_label on wire; computed by shell from missingCount.
        assertEquals("Waiting for key packages (1)…", op.displayLabel)
        assertEquals(42L, op.ageSecs)
        assertNull(snap.lastOpError)
    }

    @Test
    fun schemaV2LastOpErrorTableDecodes() {
        val snap = requireNotNull(
            TypedMarmotDecoder.decodeSnapshot(
                snapshotBuffer(
                    schemaVersion = 2u,
                    lastOpErrorOp = "invite",
                    lastOpErrorReason = "key_package_unavailable",
                )
            )
        )
        assertTrue(snap.pendingOps.isEmpty())
        val err = requireNotNull(snap.lastOpError)
        assertEquals("invite", err.op)
        assertEquals("key_package_unavailable", err.reason)
        assertEquals(1_700_000_900L, err.atSecs)
        assertEquals(hex(0x06), err.correlationId)
    }

    @Test
    fun schemaV2WithBothPendingOpAndLastOpError() {
        val cid = "cid-xyz"
        val snap = requireNotNull(
            TypedMarmotDecoder.decodeSnapshot(
                snapshotBuffer(
                    schemaVersion = 2u,
                    pendingOpCid = cid,
                    lastOpErrorReason = "key_package_unavailable",
                )
            )
        )
        assertNotNull(snap.pendingOps.single())
        assertEquals(cid, snap.pendingOps.single().correlationId)
        assertEquals("key_package_unavailable", requireNotNull(snap.lastOpError).reason)
    }

    @Test
    fun schemaV4DecodesCorrectly() {
        // Verify the decoder still accepts older v4 buffers (presentation fields
        // removed from wire) — kept so a future bump can't silently drop v4.
        val snap = requireNotNull(TypedMarmotDecoder.decodeSnapshot(snapshotBuffer(schemaVersion = 4u)))
        assertEquals("Team", snap.groups.single().name)
        // Shell-computed properties work even when decoder uses v4 path.
        assertEquals("Team", snap.groups.single().displayName)
        assertEquals("1 invite", snap.invitesChipLabel)
    }

    @Test
    fun schemaV5DecodesAndCarriesInitError() {
        // #1651: the CURRENT wire version (v5) MUST be accepted (the regression
        // delta caught: the decoder rejected v5 and dropped the whole snapshot),
        // and the new init_error_kind/init_error_detail tokens must reach state.
        val snap = requireNotNull(
            TypedMarmotDecoder.decodeSnapshot(
                snapshotBuffer(
                    schemaVersion = 5u,
                    initErrorKind = "db_key_lost",
                    initErrorDetail = "no encryption key found in keyring",
                ),
            ),
        )
        assertEquals("db_key_lost", snap.initErrorKind)
        assertEquals("no encryption key found in keyring", snap.initErrorDetail)

        // The default (no init error) v5 buffer decodes with empty tokens.
        val healthy = requireNotNull(TypedMarmotDecoder.decodeSnapshot(snapshotBuffer()))
        assertEquals("", healthy.initErrorKind)
    }

    // ── messages happy path ──────────────────────────────────────────────────

    @Test
    fun messagesHappyPathMapsGroupKeyedMap() {
        val map = requireNotNull(TypedMarmotDecoder.decodeMessages(messagesBuffer()))
        assertEquals(setOf(hex(0x01)), map.keys)
        val msg = map.getValue(hex(0x01)).single()
        assertEquals(hex(0x21), msg.id)
        assertEquals("gm", msg.content)
        assertEquals(7L, msg.epoch) // has_epoch == true
    }

    // ── envelope selector ────────────────────────────────────────────────────

    @Test
    fun decodeSelectsByKeyAndSchema() {
        val snapEnv = TypedProjectionEnvelope(
            key = TypedMarmotDecoder.SNAPSHOT_KEY,
            schemaId = TypedMarmotDecoder.SNAPSHOT_SCHEMA_ID,
            schemaVersion = 5u,
            fileIdentifier = TypedMarmotDecoder.SNAPSHOT_FILE_IDENTIFIER,
            payload = snapshotBuffer(),
        )
        assertEquals(1, requireNotNull(TypedMarmotDecoder.decodeSnapshot(listOf(snapEnv))).groups.size)
        assertNull(TypedMarmotDecoder.decodeSnapshot(emptyList()))
        assertNull(TypedMarmotDecoder.decodeMessages(emptyList()))
    }

    // ── failure paths ────────────────────────────────────────────────────────

    @Test
    fun unsupportedSchemaVersionReturnsNull() {
        // Snapshot v1–v4 are all supported; v99 is unknown → fail closed.
        assertNull(TypedMarmotDecoder.decodeSnapshot(snapshotBuffer(schemaVersion = 99u)))
        // Messages wire is still v1-only; v2+ fails closed.
        assertNull(TypedMarmotDecoder.decodeMessages(messagesBuffer(schemaVersion = 2u)))
    }

    @Test
    fun malformedBufferReturnsNull() {
        val garbledSnap = snapshotBuffer().copyOf()
        garbledSnap[4] = 'X'.code.toByte() // clobber NMMS identifier
        assertNull(TypedMarmotDecoder.decodeSnapshot(garbledSnap))

        val garbledMsgs = messagesBuffer().copyOf()
        garbledMsgs[4] = 'X'.code.toByte() // clobber NMMG identifier
        assertNull(TypedMarmotDecoder.decodeMessages(garbledMsgs))
    }
}
