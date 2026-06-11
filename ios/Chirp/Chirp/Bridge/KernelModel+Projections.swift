import Foundation

@MainActor
extension KernelModel {
    var isRunning: Bool { snapshot?.running ?? false }
    var modularTimeline: ChirpTimelineSnapshot { typedHomeFeed ?? snapshot?.homeFeed ?? .empty }
    var rev: UInt64 { snapshot?.rev ?? 0 }
    var profile: ProfileCard? { snapshot?.profile }
    var metrics: KernelMetrics? { snapshot?.metrics }
    var relayStatuses: [RelayStatus] { snapshot?.relayStatuses ?? [] }
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
    var logicalInterests: [LogicalInterestStatus] { snapshot?.logicalInterests ?? [] }
    var wireSubscriptions: [WireSubscriptionStatus] { snapshot?.wireSubscriptions ?? [] }
    var relayDiagnostics: RelayDiagnosticsSnapshot { snapshot?.relayDiagnostics ?? .empty }
    var logs: [String] { snapshot?.logs ?? [] }
    var bunkerHandshake: BunkerHandshake? { snapshot?.bunkerHandshake }
    var nip46Onboarding: Nip46Onboarding? { snapshot?.nip46Onboarding }
    var actionLifecycle: ActionLifecycleSnapshot? { snapshot?.actionLifecycle }

    var mentionProfiles: [String: MentionProfile] {
        guard let cards = snapshot?.resolvedProfiles else { return [:] }
        return cards.mapValues(MentionProfile.init(card:))
    }

    var claimedProfiles: [String: ProfileCard] {
        snapshot?.projections?.claimedProfiles ?? [:]
    }

    var resolvedProfileCards: [String: ProfileCard] {
        snapshot?.resolvedProfiles ?? [:]
    }

    var hasActiveAccount: Bool { activeAccount != nil }

    var activeAccountSummary: AccountSummary? {
        guard let id = activeAccount else { return nil }
        for account in accounts where account.id == id { return account }
        return nil
    }
}
