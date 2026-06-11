import Foundation

@MainActor
extension KernelModel {
    // ADR-0044 Tier-3 (envelope switch): typed-first with JSON fallback. The
    // typed `SnapshotFrame` envelope (`typedEnvelope`, gated on `metrics`) wins
    // when present; the generic JSON `payload` top-level scalar
    // (`snapshot?.<field>`) is the fallback (ADR-0037 Commitment 4). These seven
    // accessors are the LAST consumers of `payload`'s top-level scalars. The
    // perf/diag log lines in `apply(result:)` read raw `update.rev` /
    // `update.metrics.*` — those are guaranteed mirrors (ADR-0032), so the
    // logged value is identical to the effective value; only the UI accessors
    // need the typed-first seam.
    var isRunning: Bool { typedEnvelope?.running ?? snapshot?.running ?? false }
    var modularTimeline: ChirpTimelineSnapshot { typedHomeFeed ?? snapshot?.homeFeed ?? .empty }
    var rev: UInt64 { typedEnvelope?.rev ?? snapshot?.rev ?? 0 }
    // V6 Stage 4 (profile cluster): typed-first with JSON fallback, mirroring
    // `accounts`'s `typedAccounts ?? snapshot?.accounts`. The typed `KPRF` /
    // `KCPR` / `KRPR` sidecars win when present; the generic JSON projection is
    // the fallback (ADR-0037 Commitment 4). All three keys are routed through
    // these accessors below — `mentionProfiles` derives from `resolvedProfileCards`
    // and `KernelModel.profile(forPubkey:)` reads `claimedProfiles`, so flipping
    // the accessors flips every downstream consumer (single effective-value seam).
    var profile: ProfileCard? { typedProfile ?? snapshot?.profile }
    var metrics: KernelMetrics? { typedEnvelope?.metrics ?? snapshot?.metrics }
    var relayStatuses: [RelayStatus] { typedEnvelope?.relayStatuses ?? snapshot?.relayStatuses ?? [] }
    // V6 Stage 4 (Wave B): typed-first with JSON fallback, mirroring
    // `modularTimeline`'s `typedHomeFeed ?? snapshot?.homeFeed`. The typed
    // `KACC` / `KACT` sidecars win when present; the generic JSON projection
    // is the fallback (ADR-0037 Commitment 4).
    var accounts: [AccountSummary] { typedAccounts ?? snapshot?.accounts ?? [] }
    var activeAccount: String? { typedActiveAccount ?? snapshot?.activeAccount }
    // V6 Stage 4 (Wave B batch #2): typed-first with JSON fallback, mirroring
    // `accounts`'s `typedAccounts ?? snapshot?.accounts`. The typed `KCRL` /
    // `KRRO` / `KOXS` / `KPBO` / `KPBQ` sidecars win when present; the generic
    // JSON projection is the fallback (ADR-0037 Commitment 4). These five keys
    // are consumed ONLY through these accessors (no raw `update.<field>` /
    // `snapshot.<field>` side-effect consumers), so the accessor flip is the
    // single effective-value seam — no split-brain to route.
    var publishQueue: [PublishQueueEntry] { typedPublishQueue ?? snapshot?.publishQueue ?? [] }
    var publishOutbox: [PublishOutboxItem] { typedPublishOutbox ?? snapshot?.publishOutbox ?? [] }
    var outboxSummary: OutboxSummary { typedOutboxSummary ?? snapshot?.outboxSummary ?? .empty }
    var configuredRelays: [AppRelay] { typedConfiguredRelays ?? snapshot?.configuredRelays ?? [] }
    var relayRoleOptions: [RelayRoleOption] { typedRelayRoleOptions ?? snapshot?.relayRoleOptions ?? [] }
    var settingsHub: SettingsHubSummary { snapshot?.settingsHub ?? .empty }
    var walletStatus: WalletStatusData? { snapshot?.walletStatus }
    var logicalInterests: [LogicalInterestStatus] { typedEnvelope?.logicalInterests ?? snapshot?.logicalInterests ?? [] }
    var wireSubscriptions: [WireSubscriptionStatus] { typedEnvelope?.wireSubscriptions ?? snapshot?.wireSubscriptions ?? [] }
    // V6 Stage 4 (Wave B batch #3): typed-first with JSON fallback, mirroring
    // `accounts`'s `typedAccounts ?? snapshot?.accounts`. The typed `KRDG` /
    // `KALC` sidecars win when present; the generic JSON projection is the
    // fallback (ADR-0037 Commitment 4). Both keys are consumed ONLY through
    // these accessors (DiagnosticsView reads `relayDiagnostics`; HomeFeedView +
    // the `terminalStage`/`inFlightStage` helpers read `actionLifecycle`) — no
    // raw `update.<field>` / `snapshot.<field>` side-effect consumers, so the
    // accessor flip is the single effective-value seam.
    var relayDiagnostics: RelayDiagnosticsSnapshot { typedRelayDiagnostics ?? snapshot?.relayDiagnostics ?? .empty }
    // V6 Stage 4 (Wave B Tier-1 #4): typed-first with JSON fallback. The typed
    // `NZAP` sidecar wins when present; the generic JSON projection is the
    // fallback (ADR-0037 Commitment 4). `zaps` is accessor-only — its sole read
    // surface is this accessor (the timeline's per-note `RelationCount.zaps` in
    // `NoteRowView` is the unrelated `nmp.feed.home` op-feed field, NOT this
    // `nmp.nip57.zaps` aggregate), so the accessor flip is the single
    // effective-value seam — no store to route. `nil` ⇒ generic JSON path.
    var zaps: ZapsAggregateSnapshot? { typedZaps ?? snapshot?.zaps }
    // NIP-17 DM cluster: typed-first with JSON fallback. The `dmInbox` store and
    // `EmbedHost` (claimed_events) are routed at their effective value in
    // `apply(result:)`, so they need no accessor here. `dmRelayList` has NO Swift
    // read consumer today — this accessor is the single effective-value seam,
    // added for parity so the registry-declared `NDRL` key is read typed-first if
    // a consumer lands. `nil` ⇒ generic `projections["nmp.nip17.dm_relay_list"]`.
    var dmRelayList: DmRelayListSnapshot? { typedDmRelayList ?? snapshot?.projections?.dmRelayList }
    var logs: [String] { typedEnvelope?.logs ?? snapshot?.logs ?? [] }
    // NIP-46 cluster: typed-first with JSON fallback, mirroring `accounts`'s
    // `typedAccounts ?? snapshot?.accounts`. The typed `KBHS` / `KN46` sidecars
    // win when present; the generic JSON projection is the fallback (ADR-0037
    // Commitment 4). `bunker_handshake`'s typed closure emits no sidecar while
    // idle, so `typedBunkerHandshake` is nil in the steady state and the generic
    // JSON `null` (→ `nil`) is read — parity-preserving.
    var bunkerHandshake: BunkerHandshake? { typedBunkerHandshake ?? snapshot?.bunkerHandshake }
    var nip46Onboarding: Nip46Onboarding? { typedNip46Onboarding ?? snapshot?.nip46Onboarding }
    var actionLifecycle: ActionLifecycleSnapshot? { typedActionLifecycle ?? snapshot?.actionLifecycle }

    var mentionProfiles: [String: MentionProfile] {
        // Derived from the effective resolved-profile map so the typed `KRPR`
        // sidecar flows through here too (not the raw `snapshot?.resolvedProfiles`).
        resolvedProfileCards.mapValues(MentionProfile.init(card:))
    }

    var claimedProfiles: [String: ProfileCard] {
        typedClaimedProfiles ?? snapshot?.projections?.claimedProfiles ?? [:]
    }

    var resolvedProfileCards: [String: ProfileCard] {
        typedResolvedProfiles ?? snapshot?.resolvedProfiles ?? [:]
    }

    var hasActiveAccount: Bool { activeAccount != nil }

    var activeAccountSummary: AccountSummary? {
        guard let id = activeAccount else { return nil }
        for account in accounts where account.id == id { return account }
        return nil
    }
}
