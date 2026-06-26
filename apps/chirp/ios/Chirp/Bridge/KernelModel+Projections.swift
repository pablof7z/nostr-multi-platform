import Foundation

@MainActor
extension KernelModel {
    // Typed-only projection accessors. Each reads its dedicated typed slot
    // (assigned in `apply(result:)` from the typed sidecars / the Tier-3
    // `SnapshotFrame` envelope) and collapses to an empty default when the slot
    // is nil — there is NO generic `payload`/`snapshot` JSON fallback. Chirp no
    // longer decodes the generic `payload:Value` whole-payload tree; the typed
    // sidecars are authoritative (the producer-completeness gate proves every
    // non-null generic key has a typed sidecar, and the Tier-3 envelope fields
    // are written unconditionally on every production frame).
    var isRunning: Bool { typedEnvelope?.running ?? false }
    var modularTimeline: OpFeedSnapshot { typedHomeFeed ?? .empty }
    var rev: UInt64 { typedEnvelope?.rev ?? 0 }
    var metrics: KernelMetrics? { typedEnvelope?.metrics }
    var relayStatuses: [RelayStatus] { typedEnvelope?.relayStatuses ?? [] }
    var accounts: [AccountSummary] { typedAccounts ?? [] }
    var activeAccount: String? { typedActiveAccount }
    var publishQueue: [PublishQueueEntry] { typedPublishQueue ?? [] }
    var publishOutbox: [PublishOutboxItem] { typedPublishOutbox ?? [] }
    var outboxSummary: OutboxSummary { typedOutboxSummary ?? .empty }
    var configuredRelays: [AppRelay] { typedConfiguredRelays ?? [] }
    var relayRoleOptions: [RelayRoleOption] { typedRelayRoleOptions ?? [] }
    // The typed `KSHB` sidecar is a single-key `["relay_count": Int]` dict;
    // wrap it into `SettingsHubSummary`. `nil` slot ⇒ `.empty`.
    var settingsHub: SettingsHubSummary {
        typedSettingsHub.map { SettingsHubSummary(relayCount: $0["relay_count"] ?? 0) } ?? .empty
    }
    var walletStatus: WalletStatusData? { typedWallet }
    var logicalInterests: [LogicalInterestStatus] { typedEnvelope?.logicalInterests ?? [] }
    var wireSubscriptions: [WireSubscriptionStatus] { typedEnvelope?.wireSubscriptions ?? [] }
    var relayDiagnostics: RelayDiagnosticsSnapshot { typedRelayDiagnostics ?? .empty }
    // #626/#1924: app/operator-owned NIP-29 public-group create defaults. `.empty`
    // (suggestedRelayUrl == "") until the output-only projection's first
    // snapshot tick lands — `NewGroupSheet` pre-fills the relay field once the
    // suggested URL arrives.
    var groupDefaults: GroupDefaultsSnapshot { typedGroupDefaults ?? .empty }
    // `dmRelayList` has no Swift read consumer today — the accessor exists for
    // parity so the registry-declared `NDRL` key is surfaced if a consumer lands.
    var dmRelayList: DmRelayListSnapshot? { typedDmRelayList }
    var logs: [String] { typedEnvelope?.logs ?? [] }
    var bunkerHandshake: BunkerHandshake? { typedBunkerHandshake }
    var nip46Onboarding: Nip46Onboarding? { typedNip46Onboarding }
    /// ADR-0048 D6: unified remote-signer health (generalises the V-14 / #963
    /// bunker connection health). `nil` while no remote-signer session is
    /// active (local-key accounts). Drives `SignerStateRow` in `AccountsView`
    /// for BOTH NIP-46 bunker and NIP-55 (Amber) accounts.
    var signerState: SignerState? { typedSignerState }
    var actionLifecycle: ActionLifecycleSnapshot? { typedActionLifecycle }

    // ADR-0063 Lane E (#1671): the whole-map profile accessors (`profile`,
    // `mentionProfiles`, `claimedProfiles`, `resolvedProfileCards`) are REMOVED.
    // They read the removed `@Published` profile-cluster slots and a profile
    // update through any of them re-rendered the whole view tree. The shell now
    // reads profiles per-key from the `keyedRefCache` via
    // `profile(forPubkey:)` / `profileCard(forPubkey:)`, observed per-key via
    // `profileRowChanged` (D4) — no whole-map broadcast.

    var hasActiveAccount: Bool { activeAccount != nil }

    var activeAccountSummary: AccountSummary? {
        guard let id = activeAccount else { return nil }
        for account in accounts where account.id == id { return account }
        return nil
    }
}
