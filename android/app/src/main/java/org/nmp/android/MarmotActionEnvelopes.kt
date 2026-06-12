package org.nmp.android

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

// ─────────────────────────────────────────────────────────────────────────────
// MarmotActionEnvelopes — typed @Serializable DTOs for the "nmp.marmot"
// action namespace. Byte-identical wire shapes with the Rust `MarmotAction`
// enum in `crates/nmp-marmot/src/projection/action.rs`:
//
//   #[serde(tag = "op", rename_all = "snake_case")]
//   pub enum MarmotAction { … }
//
// Each DTO carries a fixed `op` field as a primary constructor parameter
// (NO default value) so `chirpActionJson` (encodeDefaults=false) always
// emits it. The caller instantiates each type directly and chirpActionJson
// encodes it to the wire JSON.
//
// Changing any field name here is a breaking wire change — coordinate with
// nmp-marmot/src/projection/action.rs.
// ─────────────────────────────────────────────────────────────────────────────

/** Publish (or rotate) the local MLS key-package — kind:30443 + kind:443. */
@Serializable
internal data class MarmotPublishKeyPackageEnvelope(
    val op: String,
) {
    constructor() : this(op = "publish_key_package")
}

/**
 * Create a new MLS group. [inviteeText] is the raw text the user typed;
 * Rust tokenises (whitespace / comma / semicolon / newline) and validates
 * each entry. Empty [inviteeText] is fine (group with just the creator).
 */
@Serializable
internal data class MarmotCreateGroupEnvelope(
    val op: String,
    val name: String,
    val description: String = "",
    @SerialName("invitee_text") val inviteeText: String? = null,
    @SerialName("signed_key_package_events_json")
    val signedKeyPackageEventsJson: List<JsonElement> = emptyList(),
) {
    constructor(
        name: String,
        description: String = "",
        inviteeText: String? = null,
        signedKeyPackageEventsJson: List<JsonElement> = emptyList(),
    ) : this(
        op = "create_group",
        name = name,
        description = description,
        inviteeText = inviteeText,
        signedKeyPackageEventsJson = signedKeyPackageEventsJson,
    )
}

/**
 * Invite peers to an existing MLS group. [inviteeText] is the raw text;
 * Rust tokenises and validates.
 */
@Serializable
internal data class MarmotInviteEnvelope(
    val op: String,
    @SerialName("group_id_hex") val groupIdHex: String,
    @SerialName("invitee_text") val inviteeText: String? = null,
    @SerialName("signed_key_package_events_json")
    val signedKeyPackageEventsJson: List<JsonElement> = emptyList(),
) {
    constructor(
        groupIdHex: String,
        inviteeText: String? = null,
        signedKeyPackageEventsJson: List<JsonElement> = emptyList(),
    ) : this(
        op = "invite",
        groupIdHex = groupIdHex,
        inviteeText = inviteeText,
        signedKeyPackageEventsJson = signedKeyPackageEventsJson,
    )
}

/** Send a kind:14 NIP-44 group message. */
@Serializable
internal data class MarmotSendEnvelope(
    val op: String,
    @SerialName("group_id_hex") val groupIdHex: String,
    val text: String,
) {
    constructor(groupIdHex: String, text: String) : this(
        op = "send",
        groupIdHex = groupIdHex,
        text = text,
    )
}

/** Self-remove from a group (MLS SelfRemove proposal + commit). */
@Serializable
internal data class MarmotLeaveEnvelope(
    val op: String,
    @SerialName("group_id_hex") val groupIdHex: String,
) {
    constructor(groupIdHex: String) : this(op = "leave", groupIdHex = groupIdHex)
}

/**
 * Remove other members from the group (MLS Remove proposal + commit).
 * [memberNpubs] accepts raw hex pubkeys — PublicKey::parse accepts both hex
 * and npub, so snapshot member hex strings pass verbatim.
 */
@Serializable
internal data class MarmotRemoveEnvelope(
    val op: String,
    @SerialName("group_id_hex") val groupIdHex: String,
    @SerialName("member_npubs") val memberNpubs: List<String> = emptyList(),
) {
    constructor(groupIdHex: String, memberNpubs: List<String> = emptyList()) : this(
        op = "remove",
        groupIdHex = groupIdHex,
        memberNpubs = memberNpubs,
    )
}

/** Accept a previously-cached pending Welcome (gift-wrap event id hex). */
@Serializable
internal data class MarmotAcceptWelcomeEnvelope(
    val op: String,
    @SerialName("welcome_id_hex") val welcomeIdHex: String,
) {
    constructor(welcomeIdHex: String) : this(op = "accept_welcome", welcomeIdHex = welcomeIdHex)
}

/** Decline a previously-cached pending Welcome. */
@Serializable
internal data class MarmotDeclineWelcomeEnvelope(
    val op: String,
    @SerialName("welcome_id_hex") val welcomeIdHex: String,
) {
    constructor(welcomeIdHex: String) : this(op = "decline_welcome", welcomeIdHex = welcomeIdHex)
}

/**
 * Explicit pending-commit clear (mdk-api.md §7.7) — exposed so a caller
 * that detected a relay-publish failure can unwedge the group.
 */
@Serializable
internal data class MarmotClearPendingEnvelope(
    val op: String,
    @SerialName("group_id_hex") val groupIdHex: String,
) {
    constructor(groupIdHex: String) : this(op = "clear_pending", groupIdHex = groupIdHex)
}
