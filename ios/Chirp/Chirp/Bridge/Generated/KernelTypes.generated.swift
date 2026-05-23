// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-core --features codegen-schema \
//       --bin dump_projection_schemas \
//       | cargo run -p nmp-codegen -- gen swift --stdin --out <path>
//
// Source of truth: the Rust projection types listed in the per-struct
// provenance comments below. The CI gate (`.github/workflows/codegen-drift.yml`)
// fails any PR whose generated Swift differs from a fresh run.
//
// Stage 1 pilot — 7 flat-record types (V6, docs/architecture-audit/
// v6-codegen-plan.md §6b). Stage 2 expands to the dotted-projection-key
// registry; Stage 3 sweeps the remaining hand-written Decodables.
// ─────────────────────────────────────────────────────────────────────────────

import Foundation

// MARK: - KernelMetrics
// Source: nmp_core::kernel::types::Metrics
public struct KernelMetrics: Decodable, Equatable {
    public let actorQueueDepth: UInt32
    public let bytesRx: UInt64
    public let bytesTx: UInt64
    public let claimDropsTotal: UInt64
    public let closedRx: UInt64
    public let contactsAuthors: Int
    public let deleteEvents: UInt64
    public let diagnosticFirehoseEvents: UInt64
    public let dispatchDropsTotal: UInt64
    public let duplicateEvents: UInt64
    public let emitHzConfigured: UInt32
    public let eoseRx: UInt64
    public let estimatedStoreBytes: Int
    public let eventsPerSecondConfigured: UInt32
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
    public let updateSequence: UInt64
    public let updatedCount: Int
    public let visibleItems: Int
    public let visiblePlaceholderAvatarItems: Int
    public let visibleProfiledItems: Int

    private enum CodingKeys: String, CodingKey {
        case actorQueueDepth = "actor_queue_depth"
        case bytesRx = "bytes_rx"
        case bytesTx = "bytes_tx"
        case claimDropsTotal = "claim_drops_total"
        case closedRx = "closed_rx"
        case contactsAuthors = "contacts_authors"
        case deleteEvents = "delete_events"
        case diagnosticFirehoseEvents = "diagnostic_firehose_events"
        case dispatchDropsTotal = "dispatch_drops_total"
        case duplicateEvents = "duplicate_events"
        case emitHzConfigured = "emit_hz_configured"
        case eoseRx = "eose_rx"
        case estimatedStoreBytes = "estimated_store_bytes"
        case eventsPerSecondConfigured = "events_per_second_configured"
        case eventsRx = "events_rx"
        case eventsSinceLastUpdate = "events_since_last_update"
        case firstEventMs = "first_event_ms"
        case framesRx = "frames_rx"
        case generatedEvents = "generated_events"
        case insertedCount = "inserted_count"
        case lastEventToEmitMs = "last_event_to_emit_ms"
        case makeUpdateUs = "make_update_us"
        case maxEventToEmitMs = "max_event_to_emit_ms"
        case maxEventsPerUpdate = "max_events_per_update"
        case noteEvents = "note_events"
        case noticesRx = "notices_rx"
        case openViews = "open_views"
        case payloadBytes = "payload_bytes"
        case profileEvents = "profile_events"
        case removedCount = "removed_count"
        case serializeUs = "serialize_us"
        case storeToPayloadRatio = "store_to_payload_ratio"
        case storedEvents = "stored_events"
        case targetProfileLoadedMs = "target_profile_loaded_ms"
        case timelineAuthors = "timeline_authors"
        case timelineFirstItemMs = "timeline_first_item_ms"
        case timelineOpenedMs = "timeline_opened_ms"
        case tombstones
        case updateEmittedMs = "update_emitted_ms"
        case updateSequence = "update_sequence"
        case updatedCount = "updated_count"
        case visibleItems = "visible_items"
        case visiblePlaceholderAvatarItems = "visible_placeholder_avatar_items"
        case visibleProfiledItems = "visible_profiled_items"
    }
}

// MARK: - RelayStatus
// Source: nmp_core::kernel::types::RelayStatus
public struct RelayStatus: Decodable, Equatable, Identifiable {
    public let activeWireSubscriptions: Int
    public let auth: String
    public let bytesRx: UInt64
    public let bytesTx: UInt64
    public let connection: String
    public let denied: Bool
    public let errorCategory: String?
    public let lastCloseReason: String?
    public let lastConnectedAtMs: UInt64?
    public let lastError: String?
    public let lastEventAtMs: UInt64?
    public let lastNotice: String?
    public let nip77Negentropy: String
    public let reconnectCount: UInt32
    public let relayUrl: String
    public let role: String

    public var id: String { relayUrl }

    private enum CodingKeys: String, CodingKey {
        case activeWireSubscriptions = "active_wire_subscriptions"
        case auth
        case bytesRx = "bytes_rx"
        case bytesTx = "bytes_tx"
        case connection
        case denied
        case errorCategory = "error_category"
        case lastCloseReason = "last_close_reason"
        case lastConnectedAtMs = "last_connected_at_ms"
        case lastError = "last_error"
        case lastEventAtMs = "last_event_at_ms"
        case lastNotice = "last_notice"
        case nip77Negentropy = "nip77_negentropy"
        case reconnectCount = "reconnect_count"
        case relayUrl = "relay_url"
        case role
    }
}

// MARK: - LogicalInterestStatus
// Source: nmp_core::kernel::types::LogicalInterestStatus
public struct LogicalInterestStatus: Decodable, Equatable, Identifiable {
    public let cacheCoverage: String
    public let key: String
    public let refcount: UInt32
    public let relayUrls: [String]
    public let state: String
    public let warmingUntilMs: UInt64?

    public var id: String { key }

    private enum CodingKeys: String, CodingKey {
        case cacheCoverage = "cache_coverage"
        case key
        case refcount
        case relayUrls = "relay_urls"
        case state
        case warmingUntilMs = "warming_until_ms"
    }
}

// MARK: - WireSubscriptionStatus
// Source: nmp_core::kernel::types::WireSubscriptionStatus
public struct WireSubscriptionStatus: Decodable, Equatable, Identifiable {
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

    private enum CodingKeys: String, CodingKey {
        case closeReason = "close_reason"
        case eoseAtMs = "eose_at_ms"
        case eventsRx = "events_rx"
        case filterSummary = "filter_summary"
        case lastEventAtMs = "last_event_at_ms"
        case logicalConsumerCount = "logical_consumer_count"
        case openedAtMs = "opened_at_ms"
        case relayUrl = "relay_url"
        case state
        case wireId = "wire_id"
    }
}

// MARK: - AccountSummary
// Source: nmp_core::kernel::identity_state::AccountSummary
public struct AccountSummary: Decodable, Equatable, Identifiable {
    public let displayName: String
    public let id: String
    public let isActive: Bool
    public let npub: String
    public let pictureUrl: String?
    public let signerIsRemote: Bool
    public let signerKind: String
    public let signerLabel: String
    public let status: String

    private enum CodingKeys: String, CodingKey {
        case displayName = "display_name"
        case id
        case isActive = "is_active"
        case npub
        case pictureUrl = "picture_url"
        case signerIsRemote = "signer_is_remote"
        case signerKind = "signer_kind"
        case signerLabel = "signer_label"
        case status
    }
}

// MARK: - RelayEditRow
// Source: nmp_core::kernel::identity_state::RelayEditRow
public struct RelayEditRow: Decodable, Equatable, Identifiable {
    public let role: String
    public let roleLabel: String
    public let roleTint: String
    public let url: String

    public var id: String { url }

    private enum CodingKeys: String, CodingKey {
        case role
        case roleLabel = "role_label"
        case roleTint = "role_tint"
        case url
    }
}

// MARK: - RelayRoleOption
// Source: nmp_core::actor::relay_roles::RelayRoleOption
public struct RelayRoleOption: Decodable, Equatable, Identifiable {
    public let isDefault: Bool
    public let label: String
    public let tint: String
    public let value: String

    public var id: String { value }

    private enum CodingKeys: String, CodingKey {
        case isDefault = "is_default"
        case label
        case tint
        case value
    }
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
    let publishQueue: [PublishQueueEntry]?
    let publishOutbox: [PublishOutboxItem]?
    let outboxSummary: OutboxSummary?
    let relayEditRows: [RelayEditRow]?
    let relayRoleOptions: [RelayRoleOption]?
    let accounts: [AccountSummary]?
    let activeAccount: String?
    let actionResults: [LastActionResult]?
    let lastActionResult: LastActionResult?
    let actionStages: [String: [ActionStageEntry]]?
    let actionLifecycle: ActionLifecycleSnapshot?
    let profile: ProfileCard?
    let timeline: [TimelineItem]?
    let authorView: AuthorProfileSnapshot?
    let threadView: ThreadView?
    let inserted: [TimelineItem]?
    let updated: [TimelineItem]?
    let removed: [String]?
    let groupChat: GroupChatSnapshot?
    let dmInbox: DmInboxSnapshot?
    let followList: FollowListSnapshot?
    let discoveredGroups: DiscoveredGroupsSnapshot?
    let zaps: ZapsAggregateSnapshot?
    let dmRelayList: DmRelayListSnapshot?
    let relayDiagnostics: RelayDiagnosticsSnapshot?
    let mentionProfiles: [String: MentionProfileWire]?
    let settingsHub: SettingsHubSummary?

    enum CodingKeys: String, CodingKey {
        case wallet
        case bunkerHandshake
        case nip46Onboarding
        case publishQueue
        case publishOutbox
        case outboxSummary
        case relayEditRows
        case relayRoleOptions
        case accounts
        case activeAccount
        case actionResults
        case lastActionResult
        case actionStages
        case actionLifecycle
        case profile
        case timeline
        case authorView
        case threadView
        case inserted
        case updated
        case removed
        case groupChat = "nmp.nip29.groupChat"
        case dmInbox = "nmp.nip17.dmInbox"
        case followList = "nmp.followList"
        case discoveredGroups = "nmp.nip29.discoveredGroups"
        case zaps = "nmp.nip57.zaps"
        case dmRelayList = "nmp.nip17.dmRelayList"
        case relayDiagnostics
        case mentionProfiles
        case settingsHub
    }
}
