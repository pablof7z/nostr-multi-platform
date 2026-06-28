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
 * Raw data only (aim.md §2): presentation strings ([displayName], [initials],
 * [invitesChipLabel], [displayLabel]) are computed by the shell (Compose /
 * SwiftUI), NOT emitted by Rust. The Compose layer renders the computed values.
 */
@Serializable
data class MarmotGroup(
    @SerialName("id_hex") val idHex: String = "",
    val name: String = "",
    /** Member Nostr pubkeys, hex (64 chars). Presentation layer formats each. */
    val members: List<String> = emptyList(),
    @SerialName("member_count") val memberCount: Int = 0,
    /** Total decrypted message count, or null when zero (read-cursor seam). */
    @SerialName("unread_count") val unreadCount: Int? = null,
    @SerialName("last_msg_at") val lastMsgAt: Long? = null,
) {
    val id: String get() = idHex

    // ── Shell-owned presentation (aim.md §2) ──────────────────────────────

    /** Empty-name fallback applied in the shell (D7 — not emitted by Rust). */
    val displayName: String get() = if (name.isEmpty()) "Untitled group" else name

    /**
     * 2-char uppercase initials for the avatar tile, derived from [name].
     * Returns `"?"` when name is blank. Shell-computed per aim.md §2.
     */
    val initials: String
        get() {
            val nonWhitespace = name.filter { !it.isWhitespace() }
            if (nonWhitespace.isEmpty()) return "?"
            val first = nonWhitespace[0]
            val second = nonWhitespace.getOrNull(1)
            return if (second != null) "$first$second".uppercase() else first.uppercase().toString()
        }
}

@Serializable
data class MarmotPendingWelcome(
    @SerialName("id_hex") val idHex: String = "",
    @SerialName("group_name") val groupName: String = "",
    /** Inviter Nostr pubkey, hex (field name is historical; value is hex). */
    @SerialName("inviter_npub") val inviterNpub: String = "",
) {
    val id: String get() = idHex

    // ── Shell-owned presentation (aim.md §2) ──────────────────────────────

    /** Empty-name fallback applied in the shell (D7 — not emitted by Rust). */
    val displayName: String get() = if (groupName.isEmpty()) "Group invite" else groupName
}

/**
 * KeyPackage publication health. Raw data only (aim.md §2): shells (Compose,
 * SwiftUI) derive subtitle copy and button labels from these raw fields.
 * No pre-formatted display strings live here.
 */
@Serializable
data class MarmotKeyPackage(
    val published: Boolean = false,
    @SerialName("d_tag") val dTag: String? = null,
    @SerialName("age_secs") val ageSecs: Long? = null,
    val stale: Boolean = false,
    /** `true` when built against a registered Marmot signing identity. */
    @SerialName("is_registered") val isRegistered: Boolean = false,
)

/**
 * One op parked in the deferred-completion store waiting for peer key
 * packages (schema_version 2+). Mirrors Rust `PendingOpRow`.
 * FlatBuffers sidecar only — not @Serializable (absent from the JSON path).
 */
data class MarmotPendingOp(
    val correlationId: String = "",
    val opTag: String = "",
    val missingCount: Int = 0,
    /** Wall-clock seconds since the op was parked (now - created_at). */
    val ageSecs: Long = 0,
) {
    // ── Shell-owned presentation (aim.md §2) ──────────────────────────────

    /**
     * Human-readable label derived from [missingCount]. Shell-computed per
     * aim.md §2 — the projection sends raw counts, not display strings.
     */
    val displayLabel: String get() = "Waiting for key packages ($missingCount)…"
}

/**
 * The most recent terminal op FAILURE (deferred-op expiry or a failed retry),
 * or absent when none. Mirrors Rust `LastOpError`. Raw data only (aim.md §2):
 * [reason] is a machine code; the shell maps it to a banner string.
 * FlatBuffers sidecar only — not @Serializable.
 */
data class MarmotLastOpError(
    /** Failing op tag ("create_group" | "invite"). */
    val op: String = "",
    /** Machine code, e.g. "key_package_unavailable". */
    val reason: String = "",
    /** Wall-clock second the failure was recorded. */
    val atSecs: Long = 0,
    /** correlation_id of the failed action. */
    val correlationId: String = "",
)

@Serializable
data class MarmotSnapshot(
    val groups: List<MarmotGroup> = emptyList(),
    @SerialName("pending_welcomes") val pendingWelcomes: List<MarmotPendingWelcome> = emptyList(),
    @SerialName("key_package") val keyPackage: MarmotKeyPackage = MarmotKeyPackage(),
    @SerialName("cached_kp_pubkeys") val cachedKpPubkeys: List<String> = emptyList(),
    /** True when the snapshot came from a registered Marmot signing identity. */
    @SerialName("is_registered") val isRegistered: Boolean = false,
    /** V-61 diagnostic: local MLS state may have diverged from the relay epoch. */
    @SerialName("orphaned_commit_count") val orphanedCommitCount: Int = 0,
    /**
     * #1651 service-init failure machine token (replaces the V-62
     * `keyring_unavailable` bool): "" = none, "keyring_unavailable" (MLS secrets
     * are in-memory only, keyring unavailable), "db_key_lost" (the encrypted MLS
     * DB key was lost, encrypted groups unavailable). FlatBuffers sidecar only.
     */
    @kotlinx.serialization.Transient
    val initErrorKind: String = "",
    /** #1651 raw init-error detail (db_key_lost only), empty otherwise. */
    @kotlinx.serialization.Transient
    val initErrorDetail: String = "",
    /**
     * Ops parked waiting for peer key packages (schema_version 2+). Empty when
     * none. FlatBuffers sidecar only — not present on the JSON path.
     */
    @kotlinx.serialization.Transient
    val pendingOps: List<MarmotPendingOp> = emptyList(),
    /**
     * Most recent terminal op failure (schema_version 2+), or null when none.
     * FlatBuffers sidecar only — not present on the JSON path.
     */
    @kotlinx.serialization.Transient
    val lastOpError: MarmotLastOpError? = null,
) {
    // ── Shell-owned presentation (aim.md §2) ──────────────────────────────

    /**
     * Pluralised label for the pending-invites chip, or null when none.
     * Computed in the shell from raw count (aim.md §2 — pluralisation is
     * presentation, not protocol data).
     */
    val invitesChipLabel: String?
        get() = when (pendingWelcomes.size) {
            0 -> null
            1 -> "1 invite"
            else -> "${pendingWelcomes.size} invites"
        }
}

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
