import Foundation

// Action-lifecycle, publish-outbox, and perf-diagnostic DTOs for the
// KernelBridge FFI seam. Extracted from `KernelBridge.swift` so the bridge file
// holds only `KernelHandle` (file-size hard-cap separation). Pure DTOs;
// same-module Swift files see each other without import.

// ─── Perf-diagnostic types ────────────────────────────────────────────────
//
// `LogicalInterestStatus` and `WireSubscriptionStatus` moved to
// `Generated/KernelTypes.generated.swift` (V6 Stage 1, plan §6b). The Rust
// projection types in `nmp-core/src/kernel/types.rs` are now the single
// source of truth — Swift mirrors are emitted from `schemars` schemas.

// ─── Domain types shared across the UI ───────────────────────────────────

// V-112 (ADR-0042): `ThreadView` Decodable deleted — the `thread_view`
// projection (and its `threadView` field on the generated
// `SnapshotProjections`) was removed with the kernel author/thread view
// stack. Thread rendering reads the handle-opened per-app Flat feed under
// `nmp.feed.thread.<event_id>`.

// `AccountSummary` moved to `Generated/KernelTypes.generated.swift` (V6
// Stage 1, plan §6b). Rust source: `nmp-core/src/kernel/identity_state.rs`
// `AccountSummary`. Field docs live alongside the Rust definition.

struct PublishQueueEntry: Decodable, Identifiable, Equatable {
    let eventId: String
    let kind: UInt32
    let targetRelays: Int
    let status: String
    var id: String { eventId }
}

/// One action terminal result. Used in the per-tick `actionResults` array.
/// The deprecated `lastActionResult` sticky scalar was removed in #1610 —
/// use `actionResults` exclusively (drains every terminal that settled in a
/// tick, not just the last one; correct for spinner clearing per review #29).
///
/// `status` is one of `"published"`, `"failed"`, `"cancelled"`. `error` is
/// `nil` for `published` / `cancelled` and carries a human-readable reason for
/// `failed` (the publish engine joins per-relay reasons with `; `).
struct LastActionResult: Decodable, Equatable {
    let correlationId: String
    let status: String
    let error: String?
}

// ─── PR-G: action_stages projection wire type ────────────────────────────
//
// One entry in a correlation_id's stage history. The Rust side uses serde
// `#[serde(tag = "stage", rename_all = "snake_case")]` so the `stage`
// discriminant ships as a flat snake_case string ("requested",
// "publishing", "accepted", "failed"). `Failed` carries a sibling
// `reason` field; other variants do not. `at_ms` is the Unix epoch
// millisecond stamp at recording time (kernel clock, deterministic under
// replay). `detail` is opaque per-stage JSON the host renders verbatim
// — `nil` when the kernel emitted no detail.
//
// To preserve the JSON-decoded `detail` as opaque data, we use
// `AnyCodableValue` (an existing helper in this file) or a `JSONValue`
// wrapper. Since the host largely doesn't introspect `detail` today, a
// `Data?`-style passthrough is sufficient: decode as `String?` of the
// JSON serialization. For PR-G the renderer needs only `stage` and
// `reason`; carrying `detail` as `[String: AnyDecodable]` is future
// work.

/// One stage in an async action's lifecycle, decoded from one entry of
/// `projections["action_stages"][<correlation_id>][i]`.
///
/// Construction-time decoding is forgiving: any unrecognized `stage`
/// discriminant collapses to `.unknown(raw:)` so a future kernel stage
/// added without a Swift counterpart does not crash the bridge (D1 —
/// snapshot decoders must degrade gracefully on schema growth).
enum ActionStage: Equatable {
    case requested
    case awaitingCapability
    case publishing
    case accepted
    /// `reason` is the human-readable failure message the host renders
    /// verbatim. Mirrors the `error` field on `LastActionResult`.
    case failed(reason: String)
    /// User-initiated cancellation — a DISTINCT terminal from `.failed`
    /// (S7/#1754). The host renders it without an error treatment: the user
    /// asked to cancel, nothing went wrong.
    case cancelled
    /// Catchall for future kernel stages — preserves the raw tag so a
    /// diagnostic view can still display something meaningful.
    case unknown(raw: String)

    var isTerminal: Bool {
        switch self {
        case .accepted, .failed, .cancelled: return true
        default: return false
        }
    }
}

/// One row in a correlation_id's stage history. The PR-G snapshot mirror
/// projection emits a `[String: [ActionStageEntry]]` map; this struct
/// decodes one element of the inner array.
struct ActionStageEntry: Decodable, Equatable {
    let stage: ActionStage
    /// Unix epoch milliseconds — when the kernel reducer recorded the
    /// transition. Stable under `FixedClock` for deterministic replay.
    let atMs: UInt64

    enum CodingKeys: String, CodingKey {
        case stage
        case atMs
        case reason
        // `detail` is intentionally not decoded — the bridge passes the
        // stage forward verbatim without introspection. Future work can
        // add a typed `detail` field per-stage.
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let raw = try container.decode(String.self, forKey: .stage)
        atMs = try container.decode(UInt64.self, forKey: .atMs)
        switch raw {
        case "requested": stage = .requested
        case "awaiting_capability", "awaitingCapability": stage = .awaitingCapability
        case "publishing": stage = .publishing
        case "accepted": stage = .accepted
        case "failed":
            let reason = try container.decodeIfPresent(String.self, forKey: .reason) ?? ""
            stage = .failed(reason: reason)
        case "cancelled":
            stage = .cancelled
        default:
            stage = .unknown(raw: raw)
        }
    }

    /// Memberwise initializer. The custom `init(from:)` above suppresses
    /// Swift's synthesized memberwise init, so the Wave C typed-sidecar glue
    /// (`TypedProjectionGlue.actionStages`) needs this explicit one to build a
    /// row from the `flatc --swift` reader struct.
    init(stage: ActionStage, atMs: UInt64) {
        self.stage = stage
        self.atMs = atMs
    }
}

// ─── V5 thin-shell: action_lifecycle projection wire types ──────────────
//
// The kernel's `action_lifecycle` projection collapses the per-stage
// `action_stages` history into the host display shape:
// `{ in_flight: [...], recent_terminal: [...] }`. Each entry carries a
// `correlation_id` plus the latest stage (flattened verbatim from the
// Rust `LifecycleStage` enum — `Failed`'s `reason` lifts to a sibling
// of `stage`). Terminal entries drop on a 3-second TTL inside the
// kernel; the shell does not track them.

/// One stage in the V5 display projection. Mirrors the Rust
/// `LifecycleStage` enum; an unrecognized discriminant collapses to
/// `.unknown(raw:)` so a future kernel stage added without a Swift
/// counterpart does not crash the bridge (D1 — graceful schema growth).
enum ActionLifecycleStage: Equatable {
    case requested
    case awaitingCapability
    case publishing
    case accepted
    /// `reason` is the English prose fallback the host renders when no
    /// `reasonCode` is present or recognized. `reasonCode` (#1735) is the stable
    /// machine key the shell localizes via `UiLifecycleReasonProse`, present ONLY
    /// for the kernel's own curated copy; opaque upstream / diagnostic text is
    /// prose-only (`reasonCode == nil`). `reasonSubject` is an optional
    /// contextual value for interpolation. Read `localizedReason` to get the
    /// host-facing string (localized code, falling back to `reason`).
    case failed(reason: String, reasonCode: String?, reasonSubject: String?)
    /// User-initiated cancellation — a DISTINCT terminal from `.failed`
    /// (S7/#1754). The host renders it without an error/failure treatment.
    case cancelled
    /// Catchall for future kernel stages — preserves the raw tag so a
    /// diagnostic view can still display something meaningful.
    case unknown(raw: String)

    var isTerminal: Bool {
        switch self {
        case .accepted, .failed, .cancelled: return true
        default: return false
        }
    }

    /// The host-facing failure reason for a `.failed` stage: the localized
    /// `reasonCode` when present and recognized, else the English `reason`
    /// fallback the wire always carries (#1735). `nil` for non-`failed` stages.
    var localizedReason: String? {
        guard case let .failed(reason, reasonCode, reasonSubject) = self else { return nil }
        if let code = reasonCode,
            let localized = UiLifecycleReasonProse.localized(code: code, subject: reasonSubject) {
            return localized
        }
        return reason
    }
}

/// One row in either `inFlight` or `recentTerminal`. The Rust side
/// flattens `stage` and `correlation_id` (and `reason` on `failed`)
/// onto the same object, so the decoder reads them via an explicit
/// `init(from:)` that switches on the `stage` discriminant.
struct ActionLifecycleEntry: Decodable, Equatable, Identifiable {
    let correlationId: String
    let stage: ActionLifecycleStage

    var id: String { correlationId }

    enum CodingKeys: String, CodingKey {
        case correlationId
        case stage
        case reason
        case reasonCode = "reason_code"
        case reasonSubject = "reason_subject"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        correlationId = try container.decode(String.self, forKey: .correlationId)
        let raw = try container.decode(String.self, forKey: .stage)
        switch raw {
        case "requested": stage = .requested
        case "awaiting_capability", "awaitingCapability": stage = .awaitingCapability
        case "publishing": stage = .publishing
        case "accepted": stage = .accepted
        case "failed":
            let reason = try container.decodeIfPresent(String.self, forKey: .reason) ?? ""
            // #1735: the curated machine code (+ optional subject) is absent for
            // prose-only failures, so they degrade to the `reason` fallback.
            let reasonCode = try container.decodeIfPresent(String.self, forKey: .reasonCode)
            let reasonSubject = try container.decodeIfPresent(String.self, forKey: .reasonSubject)
            stage = .failed(reason: reason, reasonCode: reasonCode, reasonSubject: reasonSubject)
        case "cancelled":
            stage = .cancelled
        default:
            stage = .unknown(raw: raw)
        }
    }

    /// Memberwise initializer. The custom `init(from:)` above suppresses
    /// Swift's synthesized memberwise init, so the V6 Stage 4 typed-sidecar glue
    /// (`TypedProjectionGlue.actionLifecycle`) needs this explicit one to build a
    /// row from the `flatc --swift` reader struct (mirroring the
    /// `PublishOutboxRelay` precedent in PR #1053).
    init(correlationId: String, stage: ActionLifecycleStage) {
        self.correlationId = correlationId
        self.stage = stage
    }
}

/// V5 thin-shell display projection. The kernel handles all lifecycle
/// bookkeeping (latest-stage-wins collapse, TTL eviction on terminals).
/// The shell decodes this struct verbatim and renders directly — no
/// pendingActions set, no manual ackActionStage, no PR-G2 race cache.
struct ActionLifecycleSnapshot: Decodable, Equatable {
    /// Correlation_ids whose latest stage is non-terminal
    /// (`requested` / `awaitingCapability` / `publishing`). Render a
    /// spinner per entry. Stable order: first-record first.
    let inFlight: [ActionLifecycleEntry]
    /// Correlation_ids that settled (`accepted` / `failed`) within the
    /// last 3 seconds. Render a success/failure toast per entry; the
    /// kernel drops the entry on its next emit past the TTL. Stable
    /// order: first-record first.
    let recentTerminal: [ActionLifecycleEntry]
}

/// One publish-outbox item. ADR-0032 / aim.md §2 #4: presentation strings
/// (`title`, `preview`, `statusLabel`, `systemImage`) have been removed from
/// the wire. The shell computes display strings from the raw `kind`, `content`,
/// and `status` fields. See `NotificationsView+OutboxRow.swift` for helpers.
struct PublishOutboxItem: Decodable, Identifiable, Equatable {
    let handle: String
    let eventId: String
    let kind: UInt32
    /// Raw event content — shell derives its own kind-appropriate preview.
    let content: String
    // ADR-0032 / V-115: `createdAtDisplay` removed. Raw Unix-seconds timestamp;
    // shell formats with its own locale/TZ via `UInt64.relativeTimeFromUnixSeconds`.
    let createdAt: UInt64
    let status: String
    /// Pre-decided "is the Retry button enabled" flag. The kernel owns the
    /// retry-policy rule ("a row already sending cannot be retried"); the
    /// shell binds this directly to `.disabled(!canRetry)` (RMP bible
    /// commandment #4 — no native `if` deciding what the app should do).
    let canRetry: Bool
    let targetRelays: Int
    // ADR-0032 / V-115: `targetSummary` removed. Shell composes
    // "N relays · <time>" from `targetRelays` + `createdAt.relativeTimeFromUnixSeconds`.
    let relays: [PublishOutboxRelay]

    var id: String { handle }
}

/// One relay row within a publish-outbox item. ADR-0032 / aim.md §2 #4:
/// `statusLabel` and `attemptLabel` removed from the wire — the shell computes
/// them from the raw `status` token and `attempt` counter.
struct PublishOutboxRelay: Decodable, Identifiable, Equatable {
    let relayUrl: String
    let status: String
    let attempt: UInt32
    let message: String
    /// Raw machine token for why the relay was targeted, e.g. `"nip65_write"`,
    /// `"local_config"`, `"discovery_indexer:{kind}"`, `"recipient_inbox:{pubkey}"`,
    /// `"explicit"`. Empty string on old kernels or when no reason applies.
    /// Shell formats via `relayReasonDisplay` in `NotificationsView+OutboxRow.swift`.
    /// `skip_serializing_if = "String::is_empty"` on the Rust side means the
    /// key is absent when empty; `decodeIfPresent` handles that transparently.
    let relayReason: String

    var id: String { relayUrl }

    private enum CodingKeys: String, CodingKey {
        case relayUrl, status, attempt, message, relayReason
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        relayUrl = try c.decode(String.self, forKey: .relayUrl)
        status = try c.decode(String.self, forKey: .status)
        attempt = try c.decode(UInt32.self, forKey: .attempt)
        message = try c.decode(String.self, forKey: .message)
        relayReason = try c.decodeIfPresent(String.self, forKey: .relayReason) ?? ""
    }

    /// Memberwise initializer. The custom `init(from:)` above suppresses Swift's
    /// synthesized memberwise init, so the V6 Stage 4 typed-sidecar glue
    /// (`TypedProjectionGlue.publishOutbox`) needs this explicit one to build a
    /// row from the `flatc --swift` reader struct.
    init(
        relayUrl: String,
        status: String,
        attempt: UInt32,
        message: String,
        relayReason: String
    ) {
        self.relayUrl = relayUrl
        self.status = status
        self.attempt = attempt
        self.message = message
        self.relayReason = relayReason
    }
}

/// Per-status counters for the publish outbox. ADR-0032 / aim.md §2 #4:
/// `title` / `subtitle` pre-formatted strings removed from the wire — the
/// shell computes display strings from the raw counters. See computed helpers
/// in `NotificationsView.swift`.
struct OutboxSummary: Decodable, Equatable {
    let total: UInt32
    let sending: UInt32
    let retrying: UInt32
    let queued: UInt32
    let failed: UInt32

    /// Empty-state fallback used when the snapshot predates the projection
    /// (an older kernel build that ships no `outbox_summary` key).
    static let empty = OutboxSummary(
        total: 0,
        sending: 0,
        retrying: 0,
        queued: 0,
        failed: 0
    )
}

/// Profile summary card. Raw kind:0 metadata fields — `displayName` and
/// `pictureUrl` are `nil` until a kind:0 has arrived; the presentation
/// layer chooses its own fallback (typically the abbreviated hex pubkey).
/// ADR-0032.
struct ProfileCard: Decodable, Equatable {
    let pubkey: String
    // ADR-0032 / V-115: `npub` (bech32) removed from wire. Shells encode
    // bech32 via `nmp_app_encode_profile(app, pubkey)` or equivalent.
    /// Display name from kind:0 (`display_name` / `displayName` / `name`,
    /// first non-empty wins). `nil` when no kind:0 has arrived yet —
    /// presentation layer renders its own fallback.
    let displayName: String?
    /// Raw `name` field from kind:0, when present.
    let name: String?
    /// Raw snake-case `display_name` field from kind:0, when present.
    let rawDisplayName: String?
    /// Raw camel-case `displayName` field from kind:0, when present.
    let displayNameCamel: String?
    /// Picture URL from kind:0. `nil` when no kind:0 has arrived yet or
    /// the metadata carries no `picture` field — presentation layer
    /// chooses a placeholder strategy.
    let pictureUrl: String?
    /// Raw `banner` field from kind:0, when present.
    let banner: String?
    /// Raw `website` field from kind:0, when present.
    let website: String?
    let nip05: String
    let about: String
    /// Raw `lud16` lightning address from kind:0.
    let lud16: String?
    /// Raw `lud06` LNURL field from kind:0.
    let lud06: String?
    /// NIP-57 lightning address (`lud16`) / LNURL (`lud06`) pre-extracted
    /// from kind:0. `nil` when the user has no lightning address or their
    /// kind:0 hasn't arrived. The zap button is shown only when this is
    /// non-nil — Rust decides zapability, the shell renders (thin-shell
    /// rule).
    let lnurl: String?
}

extension ProfileCard {
    /// Display label for this profile — kind:0 display name when present,
    /// abbreviated hex pubkey otherwise. ADR-0032 fallback owned by the
    /// presentation layer.
    var displayLabel: String { displayName ?? pubkey.shortHex }
}

/// The Rust-routed write a profile button performs, or `nil` for local-only
/// chrome (currently the edit-profile sheet). The shell maps each case to the
/// matching typed model write (`model.follow` / `model.unfollow`), which builds
/// the generated `GeneratedActionBuilders` bytes — the shell never spells a
/// namespace or hand-assembles a body (M14-1 / #2145, ADR-0064 §3).
enum ProfileWrite: Equatable {
    case follow(pubkey: String)
    case unfollow(pubkey: String)
}

/// A primary profile button. The shell owns only presentation labels/icons.
/// A `nil` `write` is local chrome only (currently the edit-profile sheet).
struct ProfileAction: Equatable {
    let label: String
    /// SF Symbol name the shell renders without further mapping.
    let iconName: String
    /// Rust-routed write to perform, or nil for local UI.
    let write: ProfileWrite?
}

// V-112 (ADR-0042): `AuthorProfileSnapshot` Decodable deleted — the
// `author_view` projection (and its `authorView` field on the generated
// `SnapshotProjections`) was removed with the kernel author/thread view
// stack. Author rendering reads the handle-opened per-app Flat feed;
// `ProfileAction` above stays as
// presentation chrome for `ProfileView`.

// The synthetic-construction call site `ModularBlockView.syntheticItem`
// is updated to provide the new mandatory fields directly.

// `KernelMetrics` and `RelayStatus` moved to
// `Generated/KernelTypes.generated.swift` (V6 Stage 1, plan §6b). Rust
// source: `nmp-core/src/kernel/types.rs::Metrics` /
// `nmp-core/src/kernel/types.rs::RelayStatus`. Field docs live alongside
// the Rust definitions.
//
// The generated `KernelMetrics` adds transport/drop counters the hand-written
// shape was missing — `claimDropsTotal` and `updateFrameDegradationsTotal` —
// both non-optional `UInt64`. The Rust kernel always emits them
// (`update.rs::metrics_snapshot`), so the now-stricter Swift decode is
// safe against any live snapshot.
//
// The generated `RelayStatus` adds three fields the hand-written shape
// was missing — `errorCategory: String?`, `denied: Bool`, and
// `lastCloseReason: String?` — all currently-emitted by
// `kernel::status::relay_status()`. The `nip77Negentropy` field tightens
// from `String?` to `String` (Rust emits it unconditionally as
// `"unknown" | "probing" | "supported" | "unsupported"`), and
// `bytesRx` / `bytesTx` / `eventsRx` are tightened from optional to
// non-optional to match the Rust definitions.
