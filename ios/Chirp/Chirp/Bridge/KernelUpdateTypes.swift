import Foundation

// Update-frame, typed snapshot envelope, dispatch-result, and create-account
// DTOs for the KernelBridge FFI seam. Extracted from `KernelBridge.swift` so the
// bridge file holds only `KernelHandle` (file-size hard-cap separation). These
// are pure value types; same-module Swift files see each other without import.

enum KernelDecodedUpdateFrame {
    case snapshot(KernelUpdateResult)
    case panic(String)
}

// ─── Typed SnapshotFrame envelope (ADR-0044 Tier-3) ───────────────────────

/// The typed `SnapshotFrame` envelope fields, read DIRECTLY off the
/// `SnapshotFrame` table (ADR-0044) — distinct from the `typed_projections`
/// sidecar list every other `typed*` decode walks. PR #1034 added these
/// first-class fields (`rev`, `running`, `metrics`, the relay/interest/wire
/// vectors, `logs`) on the frame so the host reads them without walking a
/// generic payload tree.
///
/// All fields are written by the producer as a unit. The whole struct is gated
/// on the one field whose FlatBuffers accessor reports presence
/// (`SnapshotFrame.metrics != nil`); when it is absent, typed-only accessors
/// collapse to their empty/default values.
struct TypedSnapshotEnvelope: Equatable {
    let rev: UInt64
    let running: Bool
    let metrics: KernelMetrics
    let relayStatuses: [RelayStatus]
    let logicalInterests: [LogicalInterestStatus]
    let wireSubscriptions: [WireSubscriptionStatus]
    let logs: [String]
    /// Snapshot-driven error toast — read DIRECTLY off the `SnapshotFrame`
    /// table (`last_error_toast`), the same first-class envelope tier as the
    /// other fields. `nil` ⇒ no toast on this tick. This re-homes the last
    /// raw whole-payload read (`update.lastErrorToast`) onto the typed
    /// envelope; `KernelModel` copies it into its user-clearable
    /// `lastErrorToast` slot in `apply(result:)`.
    let lastErrorToast: String?
}

// ─── Swift-side timing wrapper ────────────────────────────────────────────

struct KernelUpdateResult {
    /// All optional typed slots below are `nil` when the sidecar is absent,
    /// cleared, idle, or failed decode-before-commit. `KernelModel+Projections`
    /// is typed-only; nil slots collapse to empty/default state or keep the
    /// prior cached value through `ProjectionMergeCache`.
    /// Typed home-feed decode result (ADR-0038 typed path).
    let typedHomeFeed: ChirpTimelineSnapshot?
    /// Typed `accounts` projection decode (V6 Stage 4 / Wave B `KACC` sidecar).
    let typedAccounts: [AccountSummary]?
    /// Typed `active_account` projection decode (V6 Stage 4 / Wave B `KACT`).
    /// `nil` also represents "no active account".
    let typedActiveAccount: String?
    /// Typed `configured_relays` projection decode (`KCRL`).
    let typedConfiguredRelays: [AppRelay]?
    /// Typed `relay_role_options` projection decode (`KRRO`).
    let typedRelayRoleOptions: [RelayRoleOption]?
    /// Typed `outbox_summary` projection decode (`KOXS`).
    let typedOutboxSummary: OutboxSummary?
    /// Typed `publish_outbox` projection decode (`KPBO`).
    let typedPublishOutbox: [PublishOutboxItem]?
    /// Typed `publish_queue` projection decode (`KPBQ`).
    let typedPublishQueue: [PublishQueueEntry]?
    /// Typed `relay_diagnostics` projection decode (`KRDG`).
    let typedRelayDiagnostics: RelayDiagnosticsSnapshot?
    /// Typed `action_lifecycle` projection decode (`KALC`).
    let typedActionLifecycle: ActionLifecycleSnapshot?
    /// Typed `nmp.follow_list` projection decode (`NF02`; envelope key
    /// `nmp.follow_list`, schema_id `nmp.nip02.follow_list`).
    let typedFollowList: FollowListSnapshot?
    /// Typed `nmp.nip57.zaps` projection decode (`NZAP`).
    let typedZaps: ZapsAggregateSnapshot?
    /// Typed `nmp.nip29.group_chat` projection decode (`NGCS`).
    let typedGroupChat: GroupChatSnapshot?
    /// Typed `nmp.nip29.discovered_groups` projection decode (`NDGS`).
    let typedDiscoveredGroups: DiscoveredGroupsSnapshot?
    /// Typed `nmp.nip29.group_defaults` projection decode (`NGDF`, #626).
    let typedGroupDefaults: GroupDefaultsSnapshot?
    /// Typed `profile` projection decode (`KPRF`).
    let typedProfile: ProfileCard?
    /// Typed `claimed_profiles` projection decode (`KCPR`).
    let typedClaimedProfiles: [String: ProfileCard]?
    /// Typed `resolved_profiles` projection decode (`KRPR`).
    let typedResolvedProfiles: [String: ProfileCard]?
    /// Typed `nmp.nip17.dm_inbox` projection decode (`NDMI`).
    let typedDmInbox: DmInboxSnapshot?
    /// Typed `nmp.nip17.dm_relay_list` projection decode (`NDRL`).
    let typedDmRelayList: DmRelayListSnapshot?
    /// Typed `claimed_events` projection decode (`KCEV`).
    let typedClaimedEvents: [String: ClaimedEventDto]?
    /// Typed `claimed_event_embeds` projection decode (`NEMB`, issue #1283
    /// Phase 1).
    let typedClaimedEventEmbeds: [String: EmbeddedEventEnvelope]?
    /// Typed `bunker_handshake` projection decode (`KBHS`).
    let typedBunkerHandshake: BunkerHandshake?
    /// Typed `nip46_onboarding` projection decode (`KN46`).
    let typedNip46Onboarding: Nip46Onboarding?
    /// Typed `signer_state` projection decode (`KSST`). ADR-0048 D6 —
    /// generalises the V-14 `bunker_connection_state` sidecar. `nil` while no
    /// remote-signer session is active — the steady state for local-key
    /// accounts. When non-nil, `isReady` drives the green dot,
    /// `isAwaitingApproval` the "Waiting for Amber…" affordance,
    /// `isReconnecting` the amber badge, and `isUnavailable`/`isFailed` the
    /// red re-auth prompt (ADR-0032 / relay_diagnostics pattern).
    let typedSignerState: SignerState?
    /// Typed `nmp.marmot.snapshot` projection decode (`NMMS`, V-107 / ADR-0039).
    /// Routed to `MarmotStore.apply` in `KernelModel.apply`.
    /// The producer emits no sidecar while signed-out, so nil is the steady state.
    let typedMarmotSnapshot: MarmotSnapshot?
    /// Typed `nmp.marmot.messages` projection decode (`NMMG`, V-107 / ADR-0039).
    /// The flattened-vector wire rebuilds the `group_id_hex -> [MarmotMessage]`
    /// map. Routed to `MarmotStore.apply` in `KernelModel.apply`.
    let typedMarmotMessages: [String: [MarmotMessage]]?
    /// Typed `wallet` projection decode (`NWST`). The producer emits no sidecar
    /// while the wallet is disconnected, so nil is the steady state.
    let typedWallet: WalletStatusData?
    /// Typed `settings_hub` projection decode (`KSHB`, kernel built-in).
    let typedSettingsHub: [String: Int]?
    /// Wave C: Typed `action_results` projection decode (`KARS`).
    let typedActionResults: [LastActionResult]?
    /// Wave C: Typed `action_stages` projection decode (`KAST`).
    let typedActionStages: [String: [ActionStageEntry]]?
    // V-112 (ADR-0042): typedAuthorView (AuthorProfileSnapshot) and
    // typedThreadView (ThreadView) deleted — author_view / thread_view typed
    // sidecars removed with AuthorViewState / ThreadViewState.
    /// ADR-0044 Tier-3: the typed `SnapshotFrame` envelope (`rev` / `running` /
    /// `metrics` / `relayStatuses` / `logicalInterests` / `wireSubscriptions` /
    /// `logs`), read directly off the `SnapshotFrame` table. Non-nil when the
    /// frame carried the typed envelope (gated on `metrics`).
    let typedEnvelope: TypedSnapshotEnvelope?
    /// Dynamic per-screen flat feeds keyed as `nmp.feed.author.<pubkey>` or
    /// `nmp.feed.thread.<event_id>`. These keys are opened per navigation
    /// target, so they cannot be codegen'd as fixed projection fields.
    let flatFeeds: [String: ChirpTimelineSnapshot]
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
}

// ─── dispatch_action return envelope (PR-A) ───────────────────────────────

/// Synchronous outcome of `nmp_app_dispatch_action`. The Rust kernel returns
/// `{"correlation_id":"<id>"}` on accept (the action was validated, minted a
/// correlation id, and routed to its executor), or `{"error":"<message>"}` on
/// reject (null app, unknown namespace, malformed JSON, module validator
/// rejection). PR-A: the Swift bridge parses this envelope so a caller can
/// drive a spinner keyed on the correlation_id and surface the error message
/// as a toast on the reject path.
///
/// The terminal verdict ("published" / "failed" / "cancelled") is a SEPARATE
/// async signal — match the `correlation_id` against
/// `projections["action_results"]` on subsequent snapshot ticks.
enum DispatchResult: Equatable {
    /// The action was accepted and enqueued. Carries the `correlation_id`
    /// minted by `ActionRegistry::start`. V5: the kernel's
    /// `action_lifecycle` projection automatically surfaces this id under
    /// `inFlight` until the action settles, then under `recentTerminal`
    /// for a 3-second window. The host does NOT maintain its own pending
    /// set — read `model.actionLifecycle` to drive the UI.
    case accepted(correlationId: String)
    /// The action was rejected synchronously. Carries the human-readable
    /// error from the Rust kernel — show it as a toast.
    case failure(_ message: String)

    var correlationId: String? {
        if case let .accepted(id) = self { return id }
        return nil
    }

    var errorMessage: String? {
        if case let .failure(msg) = self { return msg }
        return nil
    }

    /// Parse the JSON envelope returned by `nmp_app_dispatch_action`.
    ///
    /// The kernel's contract (`ffi/action.rs`): every non-null app returns
    /// either `{"correlation_id":"<32-hex or event-id>"}` or
    /// `{"error":"<reason>"}`. Anything else (malformed JSON, missing fields)
    /// degrades to `.failure` so the caller never silently loses an action.
    static func parse(envelope: String) -> DispatchResult {
        guard let data = envelope.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return .failure("dispatch envelope was not a JSON object (bytes=\(envelope.utf8.count))")
        }
        if let correlationId = object["correlation_id"] as? String, !correlationId.isEmpty {
            return .accepted(correlationId: correlationId)
        }
        if let message = object["error"] as? String {
            return .failure(message)
        }
        return .failure("dispatch envelope missing both correlation_id and error (bytes=\(envelope.utf8.count))")
    }
}

// ─── createAccount FFI payload (Codable, PR-L) ────────────────────────────

/// JSON payload for `nmp_app_create_new_account` — typed wrapper for the
/// profile metadata + onboarding relay seed list. The wire shape mirrors
/// what the Rust FFI expects exactly: a flat profile object
/// (`{"name":"…","about":"…"}`) and an array of two-element relay tuples
/// (`[["wss://…", "both"], …]`).
///
/// PR-L: replaces the `JSONSerialization.data(withJSONObject:)` + `try!`
/// path in `KernelBridge.createAccount` so a typed-but-impossible encode
/// failure surfaces as a toast instead of trapping the process.
struct CreateAccountFFIPayload: Encodable {
    let profile: [String: String]
    let relays: [[String]]

    init(profile: [String: String], relays: [(String, String)]) {
        self.profile = profile
        self.relays = relays.map { [$0.0, $0.1] }
    }
}

extension Duration {
    var microseconds: Int {
        let parts = components
        return Int(parts.seconds) * 1_000_000 + Int(parts.attoseconds / 1_000_000_000_000)
    }
}
