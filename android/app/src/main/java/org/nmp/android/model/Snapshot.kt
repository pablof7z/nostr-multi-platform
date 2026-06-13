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
    // ADR-0032 / #623: pre-computed by the typed WalletStatus decoder so the UI
    // never branches on raw protocol strings (thin-shell rule). `null` when no
    // wallet is configured on this snapshot tick.
    val walletLabel: String? = null,
    val walletTone: String? = null,
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
    // ADR-0048 D6 (generalises V-14 / #963): unified remote-signer health.
    // Null when no remote-signer session is active (local-key accounts).
    // Covers BOTH NIP-46 bunker sessions and NIP-55 (Amber) external-signer
    // sessions — `signerKind` discriminates. Drives the signer health badge
    // in the sign-in screen. Rust pre-computes every flag (ADR-0032 pattern):
    // isReady = green, isAwaitingApproval/isReconnecting = amber (wait),
    // isUnavailable/isFailed = red (re-auth required). Decoded typed-first from
    // the `signer_state` (`KSST`) sidecar by [TypedSignerStateDecoder] (#1099
    // parity with iOS); `null` when no remote-signer session is active.
    @SerialName("signer_state") val signerState: SignerState? = null,
    // Detailed relay diagnostics — `relay_diagnostics` (`KRDG`) sidecar, decoded
    // by [TypedRelayDiagnosticsDecoder]. `null` when the sidecar is absent. Every
    // label/tone is Rust-precomputed (ADR-0032) so the UI never branches on raw
    // protocol strings; `RelayScreen` prefers `connectionLabel`/`connectionTone`
    // here over the Tier-3 `relayStatuses` raw-string switch.
    @SerialName("relay_diagnostics") val relayDiagnostics: RelayDiagnosticsSnapshot? = null,
)

/**
 * Detailed relay diagnostics — `projections["relay_diagnostics"]` (`KRDG`).
 * Android peer of iOS `RelayDiagnosticsSnapshot` (`TypedProjectionGlue`).
 *
 * Field-for-field mirror of the kernel projection. Every `*Label`/`*Tone`
 * string is Rust-precomputed (ADR-0032 / V-14) so the presentation layer
 * never branches on raw protocol tokens (thin-shell rule). `null` display
 * strings collapse to `""` here, byte-faithful to the typed wire's
 * `has_*`-companion semantics.
 */
@Serializable
data class RelayDiagnosticsSnapshot(
    val relays: List<RelayDiagnosticsRow> = emptyList(),
    val interests: List<RelayDiagnosticsInterest> = emptyList(),
)

@Serializable
data class RelayDiagnosticsRow(
    val relayUrl: String = "",
    val shortUrl: String = "",
    val roleLabel: String = "",
    val roleTone: String = "",
    val connectionLabel: String = "",
    val connectionTone: String = "",
    val authLabel: String = "",
    val authTone: String = "",
    val totalSubCount: Int = 0,
    val activeSubCount: Int = 0,
    val eosedSubCount: Int = 0,
    val totalEventsRx: Long = 0,
    val totalEventsDisplay: String = "",
    val reconnectCount: Int = 0,
    val bytesRxDisplay: String? = null,
    val bytesTxDisplay: String? = null,
    val lastConnectedDisplay: String? = null,
    val lastEventDisplay: String? = null,
    val lastNotice: String? = null,
    val lastError: String? = null,
    val wireSubs: List<RelayDiagnosticsWireSub> = emptyList(),
    // ADR-0051 — the relay's NIP-11 information document. `null` until
    // `nmp-nip11` has fetched it (or the relay serves no document); the typed
    // wire carries this as an OPTIONAL child table (presence is the
    // discriminator — no `has_info` flag), and the JSON path as `info: null`.
    val info: RelayDiagnosticsInfo? = null,
)

/**
 * ADR-0051 relay-information document (NIP-11), Android peer of iOS
 * `RelayDiagnosticsInfo`. Field-for-field mirror of the kernel projection
 * (`crates/nmp-core/src/kernel/relay_diagnostics.rs::RelayDiagnosticsInfo`).
 *
 * Every `Option<String>` collapses to `null` when absent (byte-faithful to the
 * typed wire's `has_*`-companion semantics and the JSON path's `null`). The
 * three `limitation` booleans are tri-state (`null` = the relay did not
 * advertise the limitation). `supportedNips` is a possibly-empty list. The
 * presentation layer renders these directly — no HTTP, no JSON, no NIP-11
 * awareness (thin-shell rule).
 */
@Serializable
data class RelayDiagnosticsInfo(
    val name: String? = null,
    val description: String? = null,
    val icon: String? = null,
    val pubkey: String? = null,
    val contact: String? = null,
    val software: String? = null,
    val version: String? = null,
    @SerialName("supported_nips") val supportedNips: List<Int> = emptyList(),
    @SerialName("payment_required") val paymentRequired: Boolean? = null,
    @SerialName("auth_required") val authRequired: Boolean? = null,
    @SerialName("restricted_writes") val restrictedWrites: Boolean? = null,
)

@Serializable
data class RelayDiagnosticsWireSub(
    val wireId: String = "",
    val shortWireId: String = "",
    val relayUrl: String = "",
    val filterSummary: String = "",
    val stateLabel: String = "",
    val stateTone: String = "",
    val consumerCountLabel: String = "",
    val eventsRxDisplay: String? = null,
    val eoseObserved: Boolean = false,
    val openedDisplay: String = "",
    val lastEventDisplay: String? = null,
    val eoseDisplay: String? = null,
    val closeReason: String? = null,
)

@Serializable
data class RelayDiagnosticsInterest(
    val key: String = "",
    val state: String = "",
    val stateTone: String = "",
    val refcount: Int = 0,
    val cacheCoverage: String = "",
    val relayUrls: List<String> = emptyList(),
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
    // ADR-0050 §D7 decrypt-pipeline policy state (errors-as-state) — the
    // tri-state that replaced the old `remoteSignerUnsupported` bool.
    // "unavailable" (no active account → host hides the DM screen),
    // "limited" (a bunker backfill is pending/throttled by the bounded
    // per-account decrypt queue; `undecryptedCount > 0`), "ok" (settled).
    // Default "unavailable" so an absent field (older Rust build) hides the
    // screen rather than misleadingly reporting "ok".
    val decryptState: String = "unavailable",
    // §D7 — envelopes pending decryption or over the per-account bound.
    // Non-zero exactly when `decryptState == "limited"`.
    val undecryptedCount: Int = 0,
) {
    /** No active account — the host should hide the DM screen entirely (§D7). */
    val isUnavailable: Boolean get() = decryptState == "unavailable"

    /** A signed-in account whose backfill is still pending/throttled (§D7). */
    val isLimited: Boolean get() = decryptState == "limited"
}

/**
 * Unified remote-signer health — `projections["signer_state"]`.
 * ADR-0048 D6 (generalises the V-14 / #963 `bunker_connection_state`
 * projection). Null when no remote-signer session is active (local-key
 * accounts). Covers BOTH NIP-46 bunker sessions and NIP-55 (Amber)
 * external-signer sessions.
 *
 * Rust pre-computes every flag (NIP-46: relay-socket state in
 * `nmp-signer-broker`; NIP-55: Intent/ContentResolver outcomes) so the UI
 * never string-compares `state` (ADR-0032 relay_diagnostics pattern). The
 * states drive distinct presentation:
 *  - `isReady` → green badge ("Connected")
 *  - `isAwaitingApproval` → amber badge ("Waiting for approval…") — approve in
 *    the signer app, do not re-auth
 *  - `isReconnecting` → amber badge ("Reconnecting…") — wait, do not re-auth
 *  - `isUnavailable` → red badge ("Signer unavailable") — re-authenticate
 *  - `isFailed` → red badge ("Connection failed") — re-authenticate
 *
 * `reason` carries an optional human-readable error message on degraded
 * transitions.
 */
@Serializable
data class SignerState(
    /** Signer backend discriminator: `"nip46"` | `"nip55"` | `"local"`. */
    @SerialName("signer_kind") val signerKind: String = "",
    /**
     * Raw state token: `"ready"` | `"awaiting_approval"` | `"reconnecting"`
     * | `"unavailable"` | `"failed"`.
     */
    val state: String = "",
    /** Optional human-readable error/reason text; null when absent. */
    val reason: String? = null,
    @SerialName("is_ready") val isReady: Boolean = false,
    @SerialName("is_awaiting_approval") val isAwaitingApproval: Boolean = false,
    @SerialName("is_reconnecting") val isReconnecting: Boolean = false,
    @SerialName("is_unavailable") val isUnavailable: Boolean = false,
    @SerialName("is_failed") val isFailed: Boolean = false,
    /** Rust-precomputed display label (ADR-0032 / #1099) — rendered verbatim. */
    @SerialName("status_label") val statusLabel: String = "",
    /** Rust-precomputed tone — "active"|"warning"|"error"|"inactive". */
    @SerialName("status_tone") val statusTone: String = "",
)
