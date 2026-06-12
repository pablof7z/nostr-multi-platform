package org.nmp.android

import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Wire-shape contract tests for [MarmotActionEnvelopes].
 *
 * Each test asserts that the encoded JSON matches the exact wire format that
 * the Rust `MarmotAction` enum deserialiser (`#[serde(tag = "op",
 * rename_all = "snake_case")]`) expects, as documented in the Rust round-trip
 * test at `crates/nmp-marmot/src/projection/action.rs::ios_legacy_envelope_round_trips`.
 *
 * These tests are the regression net that replaces the hand-rolled `escapeJson`
 * approach: if any field name or `op` discriminator drifts, these break first.
 *
 * Uses [chirpActionJson] (encodeDefaults=false, explicitNulls=false) so the
 * encoded strings are minimal — matching the Rust `#[serde(default)]` tolerant
 * deserialisation.
 */
class MarmotActionEnvelopesTest {

    private val json = chirpActionJson

    // ── Helper ────────────────────────────────────────────────────────────────

    private fun parse(encoded: String): JsonObject =
        Json.decodeFromString(JsonObject.serializer(), encoded)

    private fun op(obj: JsonObject): String =
        obj["op"]!!.jsonPrimitive.content

    // ── publish_key_package ───────────────────────────────────────────────────

    @Test
    fun publishKeyPackage_opDiscriminator() {
        val encoded = json.encodeToString(MarmotPublishKeyPackageEnvelope())
        val obj = parse(encoded)
        assertEquals("publish_key_package", op(obj))
    }

    @Test
    fun publishKeyPackage_matchesRustWireExample() {
        // Rust example: {"op":"publish_key_package"}
        val encoded = json.encodeToString(MarmotPublishKeyPackageEnvelope())
        val obj = parse(encoded)
        assertEquals("publish_key_package", op(obj))
        // No extra fields beyond op (encodeDefaults=false drops the empty relays list)
        assertEquals(setOf("op"), obj.keys)
    }

    // ── create_group ──────────────────────────────────────────────────────────

    @Test
    fun createGroup_opDiscriminator() {
        val envelope = MarmotCreateGroupEnvelope(name = "engineering")
        val obj = parse(json.encodeToString(envelope))
        assertEquals("create_group", op(obj))
    }

    @Test
    fun createGroup_includesNameAndDescription() {
        val envelope = MarmotCreateGroupEnvelope(
            name = "engineering",
            description = "the eng group",
            inviteeText = "npub1abc npub1def",
        )
        val obj = parse(json.encodeToString(envelope))
        assertEquals("create_group", op(obj))
        assertEquals("engineering", obj["name"]!!.jsonPrimitive.content)
        assertEquals("the eng group", obj["description"]!!.jsonPrimitive.content)
        assertEquals("npub1abc npub1def", obj["invitee_text"]!!.jsonPrimitive.content)
    }

    @Test
    fun createGroup_nullInviteeTextOmitted() {
        // Rust `#[serde(default)]` tolerates absence of invitee_text.
        val envelope = MarmotCreateGroupEnvelope(name = "solo-group", inviteeText = null)
        val obj = parse(json.encodeToString(envelope))
        assertFalse("invitee_text should be absent when null", obj.containsKey("invitee_text"))
    }

    @Test
    fun createGroup_specialCharsInNameEncodedCorrectly() {
        // kotlinx.serialization must escape quotes/backslashes — not escapeJson.
        val envelope = MarmotCreateGroupEnvelope(name = "eng \"best\" group")
        val encoded = json.encodeToString(envelope)
        // The encoded JSON string should parse back to the original name.
        val obj = parse(encoded)
        assertEquals("eng \"best\" group", obj["name"]!!.jsonPrimitive.content)
    }

    @Test
    fun createGroup_newlineInDescriptionEncodedCorrectly() {
        val envelope = MarmotCreateGroupEnvelope(
            name = "grp",
            description = "line1\nline2",
        )
        val encoded = json.encodeToString(envelope)
        val obj = parse(encoded)
        assertEquals("line1\nline2", obj["description"]!!.jsonPrimitive.content)
    }

    // ── invite ────────────────────────────────────────────────────────────────

    @Test
    fun invite_opDiscriminator() {
        val envelope = MarmotInviteEnvelope(
            groupIdHex = "aa00bb11",
            inviteeText = "npub1ghi",
        )
        val obj = parse(json.encodeToString(envelope))
        assertEquals("invite", op(obj))
    }

    @Test
    fun invite_groupIdHexAndInviteeText() {
        val envelope = MarmotInviteEnvelope(
            groupIdHex = "aa00bb11",
            inviteeText = "npub1ghi",
        )
        val obj = parse(json.encodeToString(envelope))
        assertEquals("aa00bb11", obj["group_id_hex"]!!.jsonPrimitive.content)
        assertEquals("npub1ghi", obj["invitee_text"]!!.jsonPrimitive.content)
    }

    // ── send ──────────────────────────────────────────────────────────────────

    @Test
    fun send_opDiscriminator() {
        val envelope = MarmotSendEnvelope(groupIdHex = "aa00bb11", text = "hello")
        val obj = parse(json.encodeToString(envelope))
        assertEquals("send", op(obj))
    }

    @Test
    fun send_matchesRustWireExample() {
        // Rust example: {"op":"send","group_id_hex":"aa00bb11","text":"hello"}
        val envelope = MarmotSendEnvelope(groupIdHex = "aa00bb11", text = "hello")
        val obj = parse(json.encodeToString(envelope))
        assertEquals("aa00bb11", obj["group_id_hex"]!!.jsonPrimitive.content)
        assertEquals("hello", obj["text"]!!.jsonPrimitive.content)
    }

    @Test
    fun send_specialCharsInTextEncodedCorrectly() {
        val envelope = MarmotSendEnvelope(groupIdHex = "aa00bb11", text = "say \"hi\"\nand bye")
        val obj = parse(json.encodeToString(envelope))
        assertEquals("say \"hi\"\nand bye", obj["text"]!!.jsonPrimitive.content)
    }

    // ── leave ─────────────────────────────────────────────────────────────────

    @Test
    fun leave_opDiscriminator() {
        val envelope = MarmotLeaveEnvelope(groupIdHex = "aa00bb11")
        val obj = parse(json.encodeToString(envelope))
        assertEquals("leave", op(obj))
    }

    @Test
    fun leave_matchesRustWireExample() {
        // Rust example: {"op":"leave","group_id_hex":"aa00bb11"}
        val envelope = MarmotLeaveEnvelope(groupIdHex = "aa00bb11")
        val obj = parse(json.encodeToString(envelope))
        assertEquals("aa00bb11", obj["group_id_hex"]!!.jsonPrimitive.content)
    }

    // ── remove ────────────────────────────────────────────────────────────────

    @Test
    fun remove_opDiscriminator() {
        val envelope = MarmotRemoveEnvelope(groupIdHex = "aa00bb11", memberNpubs = listOf("npub1ghi"))
        val obj = parse(json.encodeToString(envelope))
        assertEquals("remove", op(obj))
    }

    @Test
    fun remove_matchesRustWireExample() {
        // Rust example: {"op":"remove","group_id_hex":"aa00bb11","member_npubs":["npub1ghi"]}
        val envelope = MarmotRemoveEnvelope(
            groupIdHex = "aa00bb11",
            memberNpubs = listOf("npub1ghi"),
        )
        val encoded = json.encodeToString(envelope)
        val obj = parse(encoded)
        assertEquals("aa00bb11", obj["group_id_hex"]!!.jsonPrimitive.content)
        assertTrue(encoded.contains("\"member_npubs\""))
        assertTrue(encoded.contains("npub1ghi"))
    }

    @Test
    fun remove_acceptsHexPubkeys() {
        // PublicKey::parse accepts hex pubkeys verbatim — snapshot member hex
        // strings are valid.
        val hexPubkey = "deadbeef".repeat(8)
        val envelope = MarmotRemoveEnvelope(
            groupIdHex = "aa00bb11",
            memberNpubs = listOf(hexPubkey),
        )
        val obj = parse(json.encodeToString(envelope))
        assertEquals("remove", op(obj))
        assertTrue(json.encodeToString(envelope).contains(hexPubkey))
    }

    // ── accept_welcome ────────────────────────────────────────────────────────

    @Test
    fun acceptWelcome_opDiscriminator() {
        val envelope = MarmotAcceptWelcomeEnvelope(welcomeIdHex = "cc22dd33")
        val obj = parse(json.encodeToString(envelope))
        assertEquals("accept_welcome", op(obj))
    }

    @Test
    fun acceptWelcome_matchesRustWireExample() {
        // Rust example: {"op":"accept_welcome","welcome_id_hex":"cc22dd33"}
        val envelope = MarmotAcceptWelcomeEnvelope(welcomeIdHex = "cc22dd33")
        val obj = parse(json.encodeToString(envelope))
        assertEquals("cc22dd33", obj["welcome_id_hex"]!!.jsonPrimitive.content)
    }

    // ── decline_welcome ───────────────────────────────────────────────────────

    @Test
    fun declineWelcome_opDiscriminator() {
        val envelope = MarmotDeclineWelcomeEnvelope(welcomeIdHex = "cc22dd33")
        val obj = parse(json.encodeToString(envelope))
        assertEquals("decline_welcome", op(obj))
    }

    @Test
    fun declineWelcome_matchesRustWireExample() {
        // Rust example: {"op":"decline_welcome","welcome_id_hex":"cc22dd33"}
        val envelope = MarmotDeclineWelcomeEnvelope(welcomeIdHex = "cc22dd33")
        val obj = parse(json.encodeToString(envelope))
        assertEquals("cc22dd33", obj["welcome_id_hex"]!!.jsonPrimitive.content)
    }

    // ── clear_pending ─────────────────────────────────────────────────────────

    @Test
    fun clearPending_opDiscriminator() {
        val envelope = MarmotClearPendingEnvelope(groupIdHex = "aa00bb11")
        val obj = parse(json.encodeToString(envelope))
        assertEquals("clear_pending", op(obj))
    }

    @Test
    fun clearPending_matchesRustWireExample() {
        // Rust example: {"op":"clear_pending","group_id_hex":"aa00bb11"}
        val envelope = MarmotClearPendingEnvelope(groupIdHex = "aa00bb11")
        val obj = parse(json.encodeToString(envelope))
        assertEquals("aa00bb11", obj["group_id_hex"]!!.jsonPrimitive.content)
    }

    // ── escapeJson elimination guard ──────────────────────────────────────────

    @Test
    fun allOpsUseSerializationNotStringConcatenation() {
        // Regression guard: if any future maintainer accidentally reintroduces
        // string interpolation for the JSON body, these round-trips would fail
        // on strings with quotes or backslashes. This test encodes a payload
        // that would produce malformed JSON under naive string concat.
        val tricky = MarmotCreateGroupEnvelope(
            name = "test\\escape\"me",
            description = "line1\nline2\ttabbed",
            inviteeText = "npub1abc",
        )
        val encoded = json.encodeToString(tricky)
        // Must round-trip via a JSON parser without error (malformed JSON throws).
        val obj = parse(encoded)
        assertEquals("test\\escape\"me", obj["name"]!!.jsonPrimitive.content)
        assertEquals("line1\nline2\ttabbed", obj["description"]!!.jsonPrimitive.content)
    }
}
