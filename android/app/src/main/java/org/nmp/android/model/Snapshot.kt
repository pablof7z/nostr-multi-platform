package org.nmp.android.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Decoded shape of the kernel JSON snapshot — Android peer of iOS
 * `KernelUpdate` (see `ios/Chirp/.../KernelBridge.swift`). Every field is
 * nullable / defaulted so an older or trimmed kernel build still decodes
 * (D1: best-effort, fail-closed). Property names are camelCase; JSON is
 * snake_case via `JsonNamingStrategy.SnakeCase`.
 *
 * NO derived state lives here — this is a verbatim mirror (D8).
 */
@Serializable
data class KernelUpdate(
    val rev: Long = 0,
    val running: Boolean = false,
    val relayUrl: String = "",
    @SerialName("items") val legacyItems: List<TimelineItem> = emptyList(),
    val modularTimeline: ChirpOpFeedSnapshot = ChirpOpFeedSnapshot(),
    val metrics: KernelMetricsLite? = null,
    val relayStatuses: List<RelayStatus> = emptyList(),
    val lastErrorToast: String? = null,
    val projections: SnapshotProjections? = null,
) {
    // NOTE(#920): the kernel no longer emits a top-level `items` field nor the
    // `"timeline"` projection key (both removed in PR #924), so `legacyItems`
    // is now always empty — the home feed ships solely via `modularTimeline`
    // (the typed `nmp.feed.home` OP-feed). This legacy fallback is retained as
    // a separate UI-path migration; see follow-up issue.
    val items: List<TimelineItem>
        get() = legacyItems

    val activeAccount: String
        get() = projections?.activeAccount.orEmpty()
}

@Serializable
data class SnapshotProjections(
    @SerialName("active_account") val activeAccount: String? = null,
    val accounts: List<AccountSummary> = emptyList(),
    @SerialName("nmp.nip17.dm_inbox") val dmInbox: DmInboxSnapshot? = null,
    @SerialName("wallet_status") val walletStatus: String? = null,
    @SerialName("wallet_balance") val walletBalance: String? = null,
    @SerialName("relay_role_options") val relayRoleOptions: List<RelayRoleOption> = emptyList(),
    @SerialName("claimed_profiles") val claimedProfiles: Map<String, ProfileCard> = emptyMap(),
    @SerialName("mention_profiles") val mentionProfiles: Map<String, ProfileCard> = emptyMap(),
    // Pre-merged profile map shipped by the kernel. The UI reads this single key;
    // claimed_profiles / mention_profiles above are retained for non-UI consumers
    // but no longer merged in the presentation layer.
    @SerialName("resolved_profiles") val resolvedProfiles: Map<String, ProfileCard> = emptyMap(),
    @SerialName("action_results") val actionResults: List<LastActionResult> = emptyList(),
    @SerialName("last_action_result") val lastActionResult: LastActionResult? = null,
    @SerialName("action_stages") val actionStages: Map<String, List<ActionStageEntry>> = emptyMap(),
    @SerialName("action_lifecycle") val actionLifecycle: ActionLifecycleSnapshot? = null,
    val flatFeeds: Map<String, ChirpOpFeedSnapshot> = emptyMap(),
    // Marmot (MLS-over-Nostr) push projections (V-107 / ADR-0039), present only
    // when a Marmot MLS identity is registered. `nmp.marmot.snapshot` carries
    // the group list / welcomes / key-package; `nmp.marmot.messages` is keyed
    // by group_id_hex → newest-N decrypted messages. Both keys contain dots but
    // no underscores, so convertFromSnakeCase leaves them unchanged.
    @SerialName("nmp.marmot.snapshot") val marmotSnapshot: MarmotSnapshot? = null,
    @SerialName("nmp.marmot.messages") val marmotMessages: Map<String, List<MarmotMessage>> = emptyMap(),
)

/**
 * Raw kind:0 profile data as emitted by the kernel snapshot.
 *
 * Mirrors Rust `ProfileCard` (crates/nmp-core/src/kernel/types.rs).
 * All fields are nullable/defaulted — `None` signals "no kind:0 has
 * arrived yet" so the presentation layer can render its own fallback
 * (D1 best-effort; D8 push semantics on next snapshot tick).
 */
@Serializable
data class ProfileCard(
    val pubkey: String = "",
    val npub: String = "",
    val displayName: String? = null,
    val pictureUrl: String? = null,
    val nip05: String = "",
    val about: String = "",
    val hasProfile: Boolean = false,
    val lnurl: String? = null,
)

@Serializable
data class LastActionResult(
    @SerialName("correlation_id") val correlationId: String = "",
    val status: String = "",
    val error: String? = null,
)

@Serializable
data class ActionStageEntry(
    val stage: String = "",
    @SerialName("at_ms") val atMs: Long = 0,
    val reason: String? = null,
)

@Serializable
data class ActionLifecycleEntry(
    @SerialName("correlation_id") val correlationId: String = "",
    val stage: String = "",
    val reason: String? = null,
)

@Serializable
data class ActionLifecycleSnapshot(
    @SerialName("in_flight") val inFlight: List<ActionLifecycleEntry> = emptyList(),
    @SerialName("recent_terminal") val recentTerminal: List<ActionLifecycleEntry> = emptyList(),
)

@Serializable
data class AccountSummary(
    val id: String = "",
    // Full bech32 `npub1…` from the kernel. The kernel never emits `npub_short`
    // (removed per aim.md §2 — the backend ships the canonical identifier, and
    // any abbreviation is a presentation concern). The Compose layer abbreviates
    // for display via `shortHex`, exactly as iOS does (`account.npub.shortHex`,
    // PR #1064). Previously this field read the nonexistent JSON key
    // `npub_short`, so it was always empty — this restores parity (#979).
    val npub: String = "",
    val displayName: String = "",
    val status: String = "",
    val signerLabel: String = "",
)

@Serializable
data class TimelineItem(
    val id: String = "",
    // aim.md §2 — backend ships raw hex pubkey + Unix seconds; the
    // Compose layer is the presentation surface and formats author
    // display / relative-time labels at render time.
    val authorPubkey: String = "",
    val content: String = "",
    val contentPreview: String = "",
    val createdAt: Long = 0,
    val relayCount: Long = 0,
)

@Serializable
data class KernelMetricsLite(
    val storedEvents: Long = 0,
    val visibleItems: Long = 0,
    val eventsRx: Long = 0,
    val updateSequence: Long = 0,
)

@Serializable
data class RelayStatus(
    val role: String = "",
    val relayUrl: String = "",
    val connection: String = "",
    val auth: String = "",
    val activeWireSubscriptions: Int = 0,
    val reconnectCount: Long = 0,
)

@Serializable
data class DmMessage(
    val id: String = "",
    val senderPubkey: String = "",
    val content: String = "",
    val createdAt: Long = 0,
    val replyTo: String? = null,
    val isOutgoing: Boolean = false,
    val sourceRelays: List<String>? = null,
)

@Serializable
data class DmConversation(
    val peerPubkey: String = "",
    val messages: List<DmMessage> = emptyList(),
)

@Serializable
data class DmInboxSnapshot(
    val conversations: List<DmConversation> = emptyList(),
    val remoteSignerUnsupported: Boolean = false,
)
