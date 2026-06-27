package org.nmp.android.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Decoded shape of the kernel JSON snapshot — Android peer of iOS
 * `KernelUpdate` (see `apps/chirp/ios/.../KernelBridge.swift`). Every field is
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
    // Rust-computed connected flag (`WalletStatus.is_connected`). The shell binds
    // it verbatim instead of deriving connectedness from `walletTone` (which would
    // be a native branch on a Rust wire discriminant; D7). `null` when no wallet
    // projection is present on this tick. Mirrors iOS `status.isConnected`.
    val walletIsConnected: Boolean? = null,
    @SerialName("relay_role_options") val relayRoleOptions: List<RelayRoleOption> = emptyList(),
    // ADR-0063 Lane H: claimed_profiles / mention_profiles / resolved_profiles JSON snapshot
    // projections deleted. Profile data is now served via the refs.profile KPRF NRRD
    // row-delta sidecar (ADR-0063 / #1671).
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
    // by [TypedRelayDiagnosticsDecoder]. `null` when the sidecar is absent. Raw
    // values are on the wire; the model carries computed display properties;
    // `RelayScreen` uses `connectionLabel`/`connectionTone` (computed) to avoid
    // branching on raw protocol strings.
    @SerialName("relay_diagnostics") val relayDiagnostics: RelayDiagnosticsSnapshot? = null,
    // #1283 / #1335 item 2: typed NEMB embed sidecar — decoded by
    // [TypedEmbedSidecarDecoder] from the `refs.event.envelopes` typed projection.
    // Empty map when the sidecar is absent (ADR-0037 Commitment 4 fail-closed).
    // The map is keyed by `primary_id` (event-id hex or `kind:pubkey:d` coord).
    // DECODE-ONLY: the kernel resolves embed projections; this shell is D0-clean.
    @SerialName("refs.event.envelopes") val refEventEnvelopes: Map<String, EmbedEnvelopeEntry> = emptyMap(),
    @SerialName("nmp.follow_list") val followList: FollowListSnapshot = FollowListSnapshot(),
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
    @SerialName("display_name")
    val displayName: String? = null,
    val name: String? = null,
    @SerialName("raw_display_name")
    val rawDisplayName: String? = null,
    @SerialName("display_name_camel")
    val displayNameCamel: String? = null,
    @SerialName("picture_url")
    val pictureUrl: String? = null,
    val banner: String? = null,
    val website: String? = null,
    val nip05: String = "",
    val about: String = "",
    val lud16: String? = null,
    val lud06: String? = null,
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
    /** English prose failure fallback (always carried for a `failed` stage). */
    val reason: String? = null,
    /**
     * Stable machine code for a CURATED failure reason (#1735); `null` for
     * opaque upstream / diagnostic text (prose-only, mirroring #1711's guard).
     * Read [localizedReason] to get the host-facing string.
     */
    @SerialName("reason_code") val reasonCode: String? = null,
    /** Optional contextual subject interpolated into the localized [reasonCode]. */
    @SerialName("reason_subject") val reasonSubject: String? = null,
) {
    /**
     * The host-facing failure reason: the localized [reasonCode] when present
     * and recognized, else the English [reason] fallback the wire always carries
     * (#1735). Mirrors iOS `ActionLifecycleStage.localizedReason`.
     */
    val localizedReason: String?
        get() = reasonCode
            ?.let { UiLifecycleReasonProse.localized(it, reasonSubject) }
            ?: reason
}

/**
 * Maps a kernel `action_lifecycle` `reason_code` (#1735) to user-facing failure
 * copy — the Android parallel of iOS `UiLifecycleReasonProse`. The kernel ships a
 * stable code only for its OWN curated copy; opaque upstream / diagnostic text
 * stays prose-only (`reason_code` absent), so the caller falls back to the
 * English `reason` the wire carries. Returns `null` for an unrecognized key.
 *
 * Android has no localized string-resource layer yet, so these return inlined
 * English copy (kept in lockstep with the iOS `NSLocalizedString` defaults); the
 * surface is wire-ready for a future `R.string` migration.
 */
object UiLifecycleReasonProse {
    fun localized(code: String, subject: String?): String? = when (code) {
        "lifecycle_no_active_account" -> "Sign in to an account first."
        "lifecycle_publish_no_explicit_target" ->
            "This private note needs an explicit relay to publish to."
        else -> null
    }
}

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
    // Stable signer wire token (`local` | `nip46` | …). `signer_label` was
    // removed from the wire (#1712, D7/D27 — presentation artifact); the shell
    // derives the human-readable label from this token below. Class-body
    // properties are excluded from kotlinx.serialization (not in the primary
    // constructor), so `signerLabel` needs no `@Transient`.
    val signerKind: String = "",
) {
    /** Human-readable signer label derived from the raw `signerKind` token
     * (#1712 / D7). The kernel no longer pre-renders it. */
    val signerLabel: String
        get() = when (signerKind) {
            "local" -> "Local key"
            "nip46" -> "NIP-46"
            else -> signerKind
        }
}

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
    val relayProvenance: List<String> = emptyList(),
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
 * the actor-lane runtime; NIP-55: Intent/ContentResolver outcomes) so the UI
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
    /**
     * Shell-derived display label (#1493 P9) — NOT on the wire. Populated by
     * [org.nmp.android.TypedSignerStateDecoder] from the raw `state` token and
     * rendered verbatim by `SignerStateRow`.
     */
    val statusLabel: String = "",
    /**
     * Shell-derived tone (#1493 P9) — "active"|"warning"|"error"|"inactive".
     * Populated by [org.nmp.android.TypedSignerStateDecoder] from `state`.
     */
    val statusTone: String = "",
)

// ── Typed embed sidecar domain models (#1283 / #1335 item 2) ─────────────────
//
// Android peer of iOS `EmbeddedEventEnvelope` + `EmbedKindProjection`
// (apps/chirp/ios/.../EmbedKindProjection.swift). Decoded ONLY from the typed
// `refs.event.envelopes` (`NEMB`) sidecar by [TypedEmbedSidecarDecoder]. The
// kernel resolves all embed projections on the Rust side (D0-clean: zero kind
// dispatch, zero tag/JSON parsing in Kotlin). Plain-text `content` for
// text-body variants is extracted from the NFCT sub-buffer by reusing the
// existing [TypedHomeFeedDecoder.decodeContentTreeBytes] codec.

/**
 * Decoded typed envelope for one embedded event — the Kotlin peer of
 * iOS `EmbeddedEventEnvelope` and the FFI sidecar's `EmbeddedEventEnvelope`.
 * Keyed by `primaryId` in [SnapshotProjections.refEventEnvelopes].
 * Not a JSON projection — populated exclusively from the typed NEMB sidecar.
 */
@Serializable
data class EmbedEnvelopeEntry(
    val primaryId: String = "",
    val uri: String = "",
    val depth: Int = 0,
    val maxDepth: Int = 4,
    val collapsed: Boolean = false,
    @SerialName("collapse_reason") val collapseReason: String? = null,
    val projection: EmbedKindProjectionEntry? = null,
)

/**
 * Kind-dispatched embed projection — exactly one variant is non-null, selected
 * by the `kind` discriminant from the `NEMB` wire. Mirrors the iOS
 * `EmbedKindProjection` enum. DECODE-ONLY: no resolution logic lives here.
 */
@Serializable
data class EmbedKindProjectionEntry(
    @SerialName("short_note") val shortNote: ShortNoteProjectionEntry? = null,
    val article: ArticleProjectionEntry? = null,
    val highlight: HighlightProjectionEntry? = null,
    val profile: ProfileProjectionEntry? = null,
    val unknown: UnknownProjectionEntry? = null,
)

/** kind:1 short text note projection (mirrors [nmp.embed.ShortNoteProjection]). */
@Serializable
data class ShortNoteProjectionEntry(
    val id: String = "",
    @SerialName("author_pubkey") val authorPubkey: String = "",
    @SerialName("author_display_name") val authorDisplayName: String? = null,
    @SerialName("author_picture_url") val authorPictureUrl: String? = null,
    @SerialName("created_at") val createdAt: Long = 0,
    /** Plain-text from the NFCT sub-buffer; empty when tree is absent. */
    val content: String = "",
    @SerialName("media_urls") val mediaUrls: List<String> = emptyList(),
)

/** kind:30023 long-form article projection (mirrors [nmp.embed.ArticleProjection]). */
@Serializable
data class ArticleProjectionEntry(
    val id: String = "",
    @SerialName("author_pubkey") val authorPubkey: String = "",
    @SerialName("author_display_name") val authorDisplayName: String? = null,
    @SerialName("author_picture_url") val authorPictureUrl: String? = null,
    @SerialName("created_at") val createdAt: Long = 0,
    val title: String? = null,
    val summary: String? = null,
    @SerialName("hero_image_url") val heroImageUrl: String? = null,
    @SerialName("d_tag") val dTag: String = "",
    /** Plain-text from the NFCT sub-buffer; empty when tree is absent. */
    val content: String = "",
)

/** kind:9802 highlight projection (mirrors [nmp.embed.HighlightProjection]). */
@Serializable
data class HighlightProjectionEntry(
    val id: String = "",
    @SerialName("author_pubkey") val authorPubkey: String = "",
    @SerialName("author_display_name") val authorDisplayName: String? = null,
    @SerialName("created_at") val createdAt: Long = 0,
    @SerialName("highlighted_text") val highlightedText: String = "",
    @SerialName("source_event_id") val sourceEventId: String? = null,
    @SerialName("source_event_addr") val sourceEventAddr: String? = null,
    @SerialName("source_url") val sourceUrl: String? = null,
    val context: String? = null,
)

/** kind:0 profile metadata projection (mirrors [nmp.embed.ProfileProjection]). */
@Serializable
data class ProfileProjectionEntry(
    val pubkey: String = "",
    @SerialName("display_name") val displayName: String? = null,
    @SerialName("picture_url") val pictureUrl: String? = null,
    val about: String? = null,
    val nip05: String? = null,
    val lud16: String? = null,
    @SerialName("banner_url") val bannerUrl: String? = null,
)

/** Fallback projection for unknown kinds (mirrors [nmp.embed.UnknownProjection]). */
@Serializable
data class UnknownProjectionEntry(
    val kind: Int = 0,
    @SerialName("author_pubkey") val authorPubkey: String = "",
    @SerialName("author_display_name") val authorDisplayName: String? = null,
    @SerialName("author_picture_url") val authorPictureUrl: String? = null,
    @SerialName("created_at") val createdAt: Long = 0,
    val content: String = "",
    val tags: List<List<String>> = emptyList(),
    @SerialName("alt_text") val altText: String? = null,
)
