package org.nmp.android.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Decoded shape of the Marmot (MLS-over-Nostr encrypted groups) push
 * projections — Android peer of the iOS `MarmotBridge.swift` DTOs.
 *
 * The kernel emits two projections on every snapshot frame once a Marmot MLS
 * identity is registered (V-107 / ADR-0039):
 *   • `nmp.marmot.snapshot`  → [MarmotSnapshot] (groups / welcomes / key-package)
 *   • `nmp.marmot.messages`  → `Map<groupIdHex, List<MarmotMessage>>`
 *
 * These mirror the Rust DTOs in `crates/nmp-marmot/src/projection/payload.rs`
 * VERBATIM. Field names there are the contract; treat any rename as a breaking
 * change for both native shells. Every field is nullable / defaulted so an
 * older or trimmed kernel build still decodes (D1: best-effort, fail-closed).
 *
 * NO derived state lives here — verbatim mirror (aim.md §2, D8). Pre-formatted
 * display strings (`subtitle`, `actionLabel`, `invitesChipLabel`, `initials`,
 * `displayName`) are Rust-owned; the Compose layer renders them as-is.
 */
@Serializable
data class MarmotGroup(
    @SerialName("id_hex") val idHex: String = "",
    val name: String = "",
    /** Empty-name fallback already applied by Rust ("Untitled group"). */
    @SerialName("display_name") val displayName: String = "",
    /** 2-char Rust-derived avatar initials (free-form metadata, not a banned formatter). */
    val initials: String = "",
    /** Member Nostr pubkeys, hex (64 chars). Presentation layer formats each. */
    val members: List<String> = emptyList(),
    @SerialName("member_count") val memberCount: Int = 0,
    /** Total decrypted message count, or null when zero (read-cursor seam). */
    @SerialName("unread_count") val unreadCount: Int? = null,
    @SerialName("last_msg_at") val lastMsgAt: Long? = null,
) {
    val id: String get() = idHex
}

@Serializable
data class MarmotPendingWelcome(
    @SerialName("id_hex") val idHex: String = "",
    @SerialName("group_name") val groupName: String = "",
    /** Empty-name fallback already applied by Rust ("Group invite"). */
    @SerialName("display_name") val displayName: String = "",
    /** Inviter Nostr pubkey, hex (field name is historical; value is hex). */
    @SerialName("inviter_npub") val inviterNpub: String = "",
) {
    val id: String get() = idHex
}

/**
 * KeyPackage publication health. `subtitle` / `ageDisplay` / `actionLabel` are
 * pre-formatted Rust-owned strings the shell renders verbatim (aim.md §6 AP1).
 */
@Serializable
data class MarmotKeyPackage(
    val published: Boolean = false,
    @SerialName("d_tag") val dTag: String? = null,
    @SerialName("age_secs") val ageSecs: Long? = null,
    val stale: Boolean = false,
    @SerialName("age_display") val ageDisplay: String? = null,
    val subtitle: String = "",
    @SerialName("action_label") val actionLabel: String = "",
)

@Serializable
data class MarmotSnapshot(
    val groups: List<MarmotGroup> = emptyList(),
    @SerialName("pending_welcomes") val pendingWelcomes: List<MarmotPendingWelcome> = emptyList(),
    @SerialName("key_package") val keyPackage: MarmotKeyPackage = MarmotKeyPackage(),
    @SerialName("cached_kp_pubkeys") val cachedKpPubkeys: List<String> = emptyList(),
    /** Rust-owned plural label ("1 invite" / "3 invites"), or null when none. */
    @SerialName("invites_chip_label") val invitesChipLabel: String? = null,
    /** True when the snapshot came from a registered Marmot signing identity. */
    @SerialName("is_registered") val isRegistered: Boolean = false,
    /** V-61 diagnostic: local MLS state may have diverged from the relay epoch. */
    @SerialName("orphaned_commit_count") val orphanedCommitCount: Int = 0,
    /** V-62 diagnostic: MLS secrets are in-memory only (keyring unavailable). */
    @SerialName("keyring_unavailable") val keyringUnavailable: Boolean = false,
)

@Serializable
data class MarmotMessage(
    val id: String = "",
    /** Author Nostr pubkey, hex (64 chars). Presentation layer formats. */
    @SerialName("sender_pubkey_hex") val senderPubkeyHex: String = "",
    val content: String = "",
    /** Rumor created_at (sender clock, Unix seconds). */
    @SerialName("created_at") val createdAt: Long = 0,
    /** MLS epoch the message was decrypted at, or null (pre-epoch msgs). */
    val epoch: Long? = null,
)
