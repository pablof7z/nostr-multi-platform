// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-core --features codegen-schema --bin dump_projection_schemas \
//       | cargo run -p nmp-codegen -- gen swift --out <path>
//
// Source of truth: the Rust projection types listed in the per-struct
// provenance comments below. The CI gate (`.github/workflows/codegen-drift.yml`)
// fails any PR whose generated Swift differs from a fresh run.
//
// Stage 1 pilot — 7 flat-record types (V6, docs/architecture-audit/
// docs/retired/codegen-v6.md §6b). Stage 2 expands to the dotted-projection-key
// registry; Stage 3 sweeps the remaining hand-written Decodables.
// ─────────────────────────────────────────────────────────────────────────────

import Foundation

// MARK: - KernelMetrics
// Source: nmp_core::kernel::types::Metrics
public struct KernelMetrics: Decodable, Equatable, Sendable {
    public let actorQueueDepth: UInt32
    public let bytesRx: UInt64
    public let bytesTx: UInt64
    public let claimDropsTotal: UInt64
    public let closedRx: UInt64
    public let contactsAuthors: Int
    public let deleteEvents: UInt64
    public let diagnosticFirehoseEvents: UInt64
    public let duplicateEvents: UInt64
    public let emitHzConfigured: UInt32
    public let eoseRx: UInt64
    public let estimatedStoreBytes: Int
    public let eventsRx: UInt64
    public let eventsSinceLastUpdate: UInt64
    public let firstEventMs: UInt64?
    public let framesRx: UInt64
    public let generatedEvents: UInt64
    public let insertedCount: Int
    public let lastEventToEmitMs: UInt64?
    public let makeUpdateUs: UInt64
    public let maxEventToEmitMs: UInt64
    public let maxEventsPerUpdate: UInt64
    public let noteEvents: UInt64
    public let noticesRx: UInt64
    public let openViews: UInt32
    public let payloadBytes: Int
    public let profileEvents: UInt64
    public let removedCount: Int
    public let serializeUs: UInt64
    public let storeToPayloadRatio: Double
    public let storedEvents: Int
    public let targetProfileLoadedMs: UInt64?
    public let timelineAuthors: Int
    public let timelineFirstItemMs: UInt64?
    public let timelineOpenedMs: UInt64?
    public let tombstones: Int
    public let updateEmittedMs: UInt64?
    public let updateFrameDegradationsTotal: UInt64
    public let updateSequence: UInt64
    public let updatedCount: Int
    public let visibleItems: Int
    public let visiblePlaceholderAvatarItems: Int
    public let visibleProfiledItems: Int
}

// MARK: - RelayStatus
// Source: nmp_core::kernel::types::RelayStatus
public struct RelayStatus: Decodable, Equatable, Identifiable, Sendable {
    public let activeWireSubscriptions: Int
    public let auth: String
    public let bytesRx: UInt64
    public let bytesTx: UInt64
    public let connection: String
    public let denied: Bool
    public let errorCategory: String?
    public let eventsRx: UInt64
    public let lastCloseReason: String?
    public let lastConnectedAtMs: UInt64?
    public let lastError: String?
    public let lastEventAtMs: UInt64?
    public let lastNotice: String?
    public let negentropyProbe: String
    public let noticesRx: UInt64
    public let reconnectCount: UInt32
    public let relayUrl: String
    public let role: String

    public var id: String { relayUrl }
}

// MARK: - LogicalInterestStatus
// Source: nmp_core::kernel::types::LogicalInterestStatus
public struct LogicalInterestStatus: Decodable, Equatable, Identifiable, Sendable {
    public let cacheCoverage: String
    public let key: String
    public let refcount: UInt32
    public let relayUrls: [String]
    public let state: String
    public let warmingUntilMs: UInt64?

    public var id: String { key }
}

// MARK: - WireSubscriptionStatus
// Source: nmp_core::kernel::types::WireSubscriptionStatus
public struct WireSubscriptionStatus: Decodable, Equatable, Identifiable, Sendable {
    public let closeReason: String?
    public let eoseAtMs: UInt64?
    public let eventsRx: UInt64
    public let filterSummary: String
    public let lastEventAtMs: UInt64?
    public let logicalConsumerCount: UInt32
    public let openedAtMs: UInt64
    public let relayUrl: String
    public let state: String
    public let wireId: String

    public var id: String { wireId }
}

// MARK: - AccountSummary
// Source: nmp_core::kernel::identity_state::AccountSummary
public struct AccountSummary: Decodable, Equatable, Identifiable, Sendable {
    public let displayName: String?
    public let id: String
    public let isActive: Bool
    public let npub: String
    public let pictureUrl: String?
    public let signerIsRemote: Bool
    public let signerKind: String
    public let status: String
}

// MARK: - AppRelay
// Source: nmp_core::kernel::identity_state::AppRelay
public struct AppRelay: Decodable, Equatable, Identifiable, Sendable {
    public let role: String
    public let url: String

    public var id: String { url }
}

// MARK: - RelayRoleOption
// Source: nmp_core::actor::relay_roles::RelayRoleOption
public struct RelayRoleOption: Decodable, Equatable, Identifiable, Sendable {
    public let isDefault: Bool
    public let tint: String
    public let value: String

    public var id: String { value }
}

// MARK: - SnapshotProjections
// Source: crates/nmp-codegen/src/swift_projections_registry.rs (Stage 2 registry)
//
// The kernel's host-extensible `projections` map. Each entry mirrors one
// registered snapshot-projection key. Every member is optional so a stale
// kernel build that predates a projection still decodes (D1 forward-compat).
//
// The `CodingKeys` enum below uses post-`.convertFromSnakeCase` raw values
// (the iOS shell's `KernelHandle.decode` sets that strategy). Cases whose
// raw value matches the Swift property name carry no explicit literal.
struct SnapshotProjections: Decodable, Equatable {
    let wallet: WalletStatusData?
    let bunkerHandshake: BunkerHandshake?
    let nip46Onboarding: Nip46Onboarding?
    let signerState: SignerState?
    let publishQueue: [PublishQueueEntry]?
    let publishOutbox: [PublishOutboxItem]?
    let outboxSummary: OutboxSummary?
    let configuredRelays: [AppRelay]?
    let relayRoleOptions: [RelayRoleOption]?
    let accounts: [AccountSummary]?
    let activeAccount: String?
    let actionResults: [LastActionResult]?
    let actionStages: [String: [ActionStageEntry]]?
    let actionLifecycle: ActionLifecycleSnapshot?
    let profile: ProfileCard?
    let homeFeed: OpFeedSnapshot?
    let groupEvents: GroupEventsSnapshot?
    let dmInbox: DmInboxSnapshot?
    let followList: FollowListSnapshot?
    let discoveredGroups: DiscoveredGroupsSnapshot?
    let groupDefaults: GroupDefaultsSnapshot?
    let dmRelayList: DmRelayListSnapshot?
    let relayDiagnostics: RelayDiagnosticsSnapshot?
    let refEventEnvelopes: [String: EmbeddedEventEnvelope]?
    let settingsHub: [String: Int]?
    let marmotSnapshot: MarmotSnapshot?
    let marmotMessages: [String: [MarmotMessage]]?

    enum CodingKeys: String, CodingKey {
        case wallet
        case bunkerHandshake
        case nip46Onboarding
        case signerState
        case publishQueue
        case publishOutbox
        case outboxSummary
        case configuredRelays
        case relayRoleOptions
        case accounts
        case activeAccount
        case actionResults
        case actionStages
        case actionLifecycle
        case profile
        case homeFeed = "nmp.feed.home"
        case groupEvents = "nmp.nip29.groupEvents"
        case dmInbox = "nmp.nip17.dmInbox"
        case followList = "nmp.followList"
        case discoveredGroups = "nmp.nip29.discoveredGroups"
        case groupDefaults = "nmp.nip29.groupDefaults"
        case dmRelayList = "nmp.nip17.dmRelayList"
        case relayDiagnostics
        case refEventEnvelopes = "refs.event.envelopes"
        case settingsHub
        case marmotSnapshot = "nmp.marmot.snapshot"
        case marmotMessages = "nmp.marmot.messages"
    }
}
