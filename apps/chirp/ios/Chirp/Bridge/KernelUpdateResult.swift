import Foundation

// Swift-side timing wrapper for a decoded kernel update frame. Kept separate
// from `KernelUpdateTypes.swift` so the bridge DTO files remain under the
// repository file-size cap.

struct KernelUpdateResult {
    /// Typed home-feed decode result (ADR-0038 typed path). Non-nil when the
    /// snapshot carried a well-formed `NOFS` typed projection that the Swift
    /// `NFCT` decoder could fully populate. `nil` means the generic
    /// `projections.homeFeed` fallback applies (ADR-0037 Commitment 4).
    let typedHomeFeed: OpFeedSnapshot?
    /// Typed `accounts` projection decode (V6 Stage 4 / Wave B `KACC` sidecar).
    /// Non-nil when the snapshot carried a well-formed `accounts` typed sidecar;
    /// `nil` means the generic `projections.accounts` JSON fallback applies.
    let typedAccounts: [AccountSummary]?
    /// Typed `active_account` projection decode (V6 Stage 4 / Wave B `KACT`
    /// sidecar). Non-nil when the snapshot carried a well-formed `active_account`
    /// typed sidecar that resolved to an active pubkey; `nil` means either no
    /// sidecar OR no active account — both defer to the generic
    /// `projections.active_account` JSON fallback (parity-preserving).
    let typedActiveAccount: String?
    /// Typed `configured_relays` projection decode (V6 Stage 4 / Wave B `KCRL`
    /// sidecar). `nil` ⇒ the generic `projections.configured_relays` JSON
    /// fallback applies.
    let typedConfiguredRelays: [AppRelay]?
    /// Typed `relay_role_options` projection decode (`KRRO`). `nil` ⇒ generic
    /// `projections.relay_role_options` JSON fallback.
    let typedRelayRoleOptions: [RelayRoleOption]?
    /// Typed `outbox_summary` projection decode (`KOXS`). `nil` ⇒ generic
    /// `projections.outbox_summary` JSON fallback.
    let typedOutboxSummary: OutboxSummary?
    /// Typed `publish_outbox` projection decode (`KPBO`). `nil` ⇒ generic
    /// `projections.publish_outbox` JSON fallback.
    let typedPublishOutbox: [PublishOutboxItem]?
    /// Typed `publish_queue` projection decode (`KPBQ`). The domain type is a
    /// field-subset of the wire. `nil` ⇒ generic `projections.publish_queue`
    /// JSON fallback.
    let typedPublishQueue: [PublishQueueEntry]?
    /// Typed `relay_diagnostics` projection decode (`KRDG`). `nil` ⇒ generic
    /// `projections.relay_diagnostics` JSON fallback.
    let typedRelayDiagnostics: RelayDiagnosticsSnapshot?
    /// Typed `action_lifecycle` projection decode (`KALC`). `nil` ⇒ generic
    /// `projections.action_lifecycle` JSON fallback.
    let typedActionLifecycle: ActionLifecycleSnapshot?
    /// Typed `nmp.follow_list` projection decode (`NF02`; envelope key
    /// `nmp.follow_list`, schema_id `nmp.nip02.follow_list`). `nil` ⇒ generic
    /// `projections["nmp.follow_list"]` JSON fallback.
    let typedFollowList: FollowListSnapshot?
    /// Typed `nmp.nip29.group_events` projection decode (`NGEV`). `nil` ⇒ generic
    /// `projections["nmp.nip29.group_events"]` JSON fallback.
    let typedGroupChat: GroupEventsSnapshot?
    /// Typed `nmp.nip29.discovered_groups` projection decode (`NDGS`). `nil` ⇒
    /// generic `projections["nmp.nip29.discovered_groups"]` JSON fallback.
    let typedDiscoveredGroups: DiscoveredGroupsSnapshot?
    /// Typed `nmp.nip29.group_defaults` projection decode (`NGDF`, #626).
    /// The app/operator-owned suggested public-group relay URL; read through
    /// the `groupDefaults` accessor and seeded into `NewGroupSheet`'s editable
    /// relay field. The output-only producer registers once at app init, so a
    /// current kernel emits this on every tick. `nil` means the typed sidecar
    /// is absent or malformed; there is no generic JSON projection registered
    /// for this key.
    let typedGroupDefaults: GroupDefaultsSnapshot?
    /// Typed `profile` projection decode (`KPRF`). `nil` ⇒ generic
    /// `projections["profile"]` JSON fallback.
    let typedProfile: ProfileCard?
    // ADR-0063 Lane H: typedClaimedProfiles (KCPR) and typedResolvedProfiles (KRPR)
    // deleted. Profile data is now served via the refs.profile KPRF NRRD row-delta sidecar.
    /// Typed `nmp.nip17.dm_inbox` projection decode (`NDMI`). `nil` ⇒ generic
    /// `projections["nmp.nip17.dm_inbox"]` JSON fallback. Routed to the
    /// `dmInbox` store (typed-first effective value) in `KernelModel.apply`.
    let typedDmInbox: DmInboxSnapshot?
    /// Typed `nmp.nip17.dm_relay_list` projection decode (`NDRL`). `nil` ⇒ generic
    /// `projections["nmp.nip17.dm_relay_list"]` JSON fallback. No Swift read
    /// consumer yet — read through the `dmRelayList` accessor (added for parity).
    let typedDmRelayList: DmRelayListSnapshot?
    /// Typed `refs.event.envelopes` projection decode (`NEMB`, issue #1283).
    /// `nil` means no derived render-envelope map was decoded for this frame.
    /// The kernel-resolved (`nmp_content::resolve_embed_projection`) embed map is
    /// routed to `EmbedHost.update(envelopes:)` in `KernelModel.apply`. Replaces
    /// the deleted in-Swift resolver and fixes the #1299 display_name precedence.
    let typedRefEventEnvelopes: [String: EmbeddedEventEnvelope]?
    /// Typed `bunker_handshake` projection decode (`KBHS`). `nil` ⇒ generic
    /// `projections["bunker_handshake"]` JSON fallback. The producer emits no
    /// sidecar while the handshake slot is idle, so nil is the steady state.
    let typedBunkerHandshake: BunkerHandshake?
    /// Typed `nip46_onboarding` projection decode (`KN46`). `nil` ⇒ generic
    /// `projections["nip46_onboarding"]` JSON fallback. Always present from a
    /// current kernel (the static signer-app table is emitted every tick).
    let typedNip46Onboarding: Nip46Onboarding?
    /// Typed `signer_state` projection decode (`KSST`). ADR-0048 D6 —
    /// generalises the V-14 `bunker_connection_state` sidecar. `nil` while no
    /// remote-signer session is active — the steady state for local-key
    /// accounts; no JSON fallback available because iOS is typed-sidecar-only
    /// (ADR-0037 §4). When non-nil, `isReady` drives the green dot,
    /// `isAwaitingApproval` the "Waiting for Amber…" affordance,
    /// `isReconnecting` the amber badge, and `isUnavailable`/`isFailed` the
    /// red re-auth prompt (ADR-0032 / relay_diagnostics pattern).
    let typedSignerState: SignerState?
    /// Typed `nmp.marmot.snapshot` projection decode (`NMMS`, V-107 / ADR-0039).
    /// `nil` ⇒ generic `projections["nmp.marmot.snapshot"]` JSON fallback. Routed
    /// to `MarmotStore.apply` (typed-first effective value) in `KernelModel.apply`.
    /// The producer emits no sidecar while signed-out, so nil is the steady state.
    let typedMarmotSnapshot: MarmotSnapshot?
    /// Typed `nmp.marmot.messages` projection decode (`NMMG`, V-107 / ADR-0039).
    /// `nil` ⇒ generic `projections["nmp.marmot.messages"]` JSON fallback. The
    /// flattened-vector wire rebuilds the `group_id_hex -> [MarmotMessage]` map.
    /// Routed to `MarmotStore.apply` (typed-first effective value) in
    /// `KernelModel.apply`.
    let typedMarmotMessages: [String: [MarmotMessage]]?
    /// Typed `wallet` projection decode (`NWST`). `nil` ⇒ generic
    /// `projections["wallet"]` JSON fallback. Read typed-first through the
    /// `walletStatus` accessor (`typedWallet ?? snapshot?.walletStatus`) in
    /// `KernelModel+Projections`. The producer emits no sidecar while the wallet
    /// is disconnected (slot is `None`), so nil is the steady state. The
    /// `wallet_pubkey_hex` producer field-add unblocked this flip.
    let typedWallet: WalletStatusData?
    /// Typed `settings_hub` projection decode (`KSHB`, kernel built-in). `nil` ⇒
    /// generic `projections["settings_hub"]` JSON fallback. The single-key
    /// `["relay_count": Int]` dict is read typed-first through the `settingsHub`
    /// accessor in `KernelModel+Projections` and wrapped into `SettingsHubSummary`.
    let typedSettingsHub: [String: Int]?
    /// Wave C: Typed `action_results` projection decode (`KARS`). `nil` ⇒ generic
    /// `projections.action_results` JSON fallback. The per-tick drain array; maps
    /// each `ActionResult` row to `LastActionResult`. NOTE: no read site wired yet
    /// (foundation only; wire typed-first in `KernelModel.apply` as follow-up).
    let typedActionResults: [LastActionResult]?
    /// Wave C: Typed `action_stages` projection decode (`KAST`). `nil` ⇒ generic
    /// `projections.action_stages` JSON fallback. The flat-vector wire rebuilds
    /// the `[correlation_id: [ActionStageEntry]]` dictionary. NOTE: no read site
    /// wired yet (foundation only; wire typed-first in `KernelModel.apply` as
    /// follow-up).
    let typedActionStages: [String: [ActionStageEntry]]?
    // V-112 (ADR-0042): typedAuthorView (AuthorProfileSnapshot) and
    // typedThreadView (ThreadView) deleted — author_view / thread_view typed
    // sidecars removed with AuthorViewState / ThreadViewState.
    /// ADR-0044 Tier-3: the typed `SnapshotFrame` envelope (`rev` / `running` /
    /// `metrics` / `relayStatuses` / `logicalInterests` / `wireSubscriptions` /
    /// `logs`), read directly off the `SnapshotFrame` table. Non-nil when the
    /// frame carried the typed envelope (gated on `metrics`); `nil` ⇒ the
    /// generic JSON `payload` top-level scalars apply (read through the
    /// `KernelModel+Projections` accessors).
    let typedEnvelope: TypedSnapshotEnvelope?
    /// Dynamic per-screen flat feeds keyed as `nmp.feed.author.<pubkey>` or
    /// `nmp.feed.thread.<event_id>`. These keys are opened per navigation
    /// target, so they cannot be codegen'd as fixed projection fields.
    let flatFeeds: [String: OpFeedSnapshot]
    let payloadBytes: Int
    let callbackReceivedAt: ContinuousClock.Instant
    let decodeMicros: Int
    /// R3-S3 (ADR-0055 D7): the set of projection keys whose `projectionRev`
    /// advanced in this frame. `KernelModel.apply(result:)` assigns ONLY the
    /// `@Published` slots corresponding to these keys; slots NOT in the set
    /// keep their prior value (the `ProjectionMergeCache` already retained the
    /// decoded bytes). This is the SwiftUI broad-invalidation kill.
    let changedKeys: Set<String>
    /// R3-S3 (ADR-0055 D3-4): latched `true` when the cache-merge layer
    /// encountered a typed-decode failure for at least one row. The prior cache
    /// entry is retained (no silent corruption), but the host is
    /// known-degraded for that key until the next genuine rev bump. Rung 3
    /// logs this; Rung 4 drains it via `nmp_app_request_full_snapshot()`.
    let needsResync: Bool
    /// ADR-0063 Lane E (#1671): the RAW `refs.profile` / `refs.event` row-delta
    /// batch envelopes for this frame, carried verbatim (NOT decoded) from the
    /// wire so `KernelModel.apply` can feed them into `keyedRefCache.merge` on
    /// `@MainActor` — that is where the cache's per-key `rowChanged` Combine
    /// publisher must fire so it drives SwiftUI. Empty when the frame carried
    /// no keyed-ref projection. The merge needs the frame's identity scalars
    /// (`refsSessionId` / `refsSnapshotEpoch`) to detect a session/epoch bump.
    let refsRowEnvelopes: [TypedProjectionEnvelope]
    /// ADR-0063 Lane E (#1671): the frame's `sessionId` (R3-S3 identity) so the
    /// keyed-ref cache can rebuild on an identity bump (same as the
    /// `ProjectionMergeCache` contract).
    let refsSessionId: UInt64
    /// ADR-0063 Lane E (#1671): the frame's `snapshotEpoch` (R3-S3 identity)
    /// for the keyed-ref cache's session/epoch baseline detection.
    let refsSnapshotEpoch: UInt64
}
