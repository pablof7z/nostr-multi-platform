package org.nmp.android

import com.google.flatbuffers.FlatBufferBuilder
import nmp.marmot.KeyPackageStatus
import nmp.marmot.MarmotGroupMessages
import nmp.marmot.MarmotGroupRow
import nmp.marmot.MarmotMessageRow
import nmp.marmot.MarmotMessages as FbMarmotMessages
import nmp.marmot.MarmotSnapshot as FbMarmotSnapshot
import nmp.marmot.PendingWelcomeRow
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Contract tests for [TypedMarmotDecoder] (F-05 / #979): the `NMMS`
 * `nmp.marmot.snapshot` and `NMMG` `nmp.marmot.messages` push sidecars decode
 * into [org.nmp.android.model.MarmotSnapshot] / the group-keyed message map,
 * with `has_*` companion bools reproducing null semantics, schema-version
 * gating, and malformed/absent → `null` (caller falls back to generic).
 */
class TypedMarmotDecoderTest {

    private fun hex(b: Int): String = "%02x".format(b and 0xff).repeat(32)

    private fun snapshotBuffer(schemaVersion: UInt = 1u): ByteArray {
        val builder = FlatBufferBuilder(1024)
        // group: one member, unread present, last_msg present.
        val gId = builder.createString(hex(0x01))
        val gName = builder.createString("Team")
        val gDisplay = builder.createString("Team")
        val gInitials = builder.createString("TE")
        val member = builder.createString(hex(0x02))
        val members = MarmotGroupRow.createMembersVector(builder, intArrayOf(member))
        val group = MarmotGroupRow.createMarmotGroupRow(
            builder, gId, gName, gDisplay, gInitials, members,
            1u, // member_count
            true, 4u, // has_unread_count / unread_count
            true, 1_700_000_500UL, // has_last_msg_at / last_msg_at
        )
        val groups = FbMarmotSnapshot.createGroupsVector(builder, intArrayOf(group))

        // pending welcome.
        val wId = builder.createString(hex(0x03))
        val wGroup = builder.createString("Invite")
        val wDisplay = builder.createString("Invite")
        val wInviter = builder.createString(hex(0x04))
        val welcome = PendingWelcomeRow.createPendingWelcomeRow(builder, wId, wGroup, wDisplay, wInviter)
        val welcomes = FbMarmotSnapshot.createPendingWelcomesVector(builder, intArrayOf(welcome))

        // key package: published, no d_tag, age present.
        val subtitle = builder.createString("Published 5d ago")
        val actionLabel = builder.createString("Rotate key package")
        val kp = KeyPackageStatus.createKeyPackageStatus(
            builder,
            true, // published
            false, 0, // has_d_tag
            true, 432_000UL, // has_age_secs / age_secs
            true, // stale
            false, 0, // has_age_display
            subtitle,
            actionLabel,
        )

        val cached = builder.createString(hex(0x05))
        val cachedVec = FbMarmotSnapshot.createCachedKpPubkeysVector(builder, intArrayOf(cached))
        val chip = builder.createString("1 invite")
        val snap = FbMarmotSnapshot.createMarmotSnapshot(
            builder, schemaVersion, groups, welcomes, kp, cachedVec,
            true, chip, // has_invites_chip_label / invites_chip_label
            true, // is_registered
            0u, // orphaned_commit_count
            false, // keyring_unavailable
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

    @Test
    fun snapshotHappyPathMapsAllSubtables() {
        val snap = requireNotNull(TypedMarmotDecoder.decodeSnapshot(snapshotBuffer()))
        val group = snap.groups.single()
        assertEquals(hex(0x01), group.idHex)
        assertEquals(1, group.memberCount)
        assertEquals(listOf(hex(0x02)), group.members)
        assertEquals(4, group.unreadCount) // has_unread_count == true
        assertEquals(1_700_000_500L, group.lastMsgAt)

        assertEquals(hex(0x03), snap.pendingWelcomes.single().idHex)
        assertTrue(snap.keyPackage.published)
        assertNull(snap.keyPackage.dTag) // has_d_tag == false → null
        assertEquals(432_000L, snap.keyPackage.ageSecs)
        assertEquals(listOf(hex(0x05)), snap.cachedKpPubkeys)
        assertEquals("1 invite", snap.invitesChipLabel)
        assertTrue(snap.isRegistered)
    }

    @Test
    fun messagesHappyPathMapsGroupKeyedMap() {
        val map = requireNotNull(TypedMarmotDecoder.decodeMessages(messagesBuffer()))
        assertEquals(setOf(hex(0x01)), map.keys)
        val msg = map.getValue(hex(0x01)).single()
        assertEquals(hex(0x21), msg.id)
        assertEquals("gm", msg.content)
        assertEquals(7L, msg.epoch) // has_epoch == true
    }

    @Test
    fun decodeSelectsByKeyAndSchema() {
        val snapEnv = TypedProjectionEnvelope(
            key = TypedMarmotDecoder.SNAPSHOT_KEY,
            schemaId = TypedMarmotDecoder.SNAPSHOT_SCHEMA_ID,
            schemaVersion = 1u,
            fileIdentifier = TypedMarmotDecoder.SNAPSHOT_FILE_IDENTIFIER,
            payload = snapshotBuffer(),
        )
        assertEquals(1, requireNotNull(TypedMarmotDecoder.decodeSnapshot(listOf(snapEnv))).groups.size)
        assertNull(TypedMarmotDecoder.decodeSnapshot(emptyList()))
        assertNull(TypedMarmotDecoder.decodeMessages(emptyList()))
    }

    @Test
    fun unsupportedSchemaVersionReturnsNull() {
        assertNull(TypedMarmotDecoder.decodeSnapshot(snapshotBuffer(schemaVersion = 2u)))
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
