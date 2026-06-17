import Foundation

// ─── Diagnostics read model (relay_diagnostics projection) ────────────────
//
// Mirror of `nmp-core::kernel::relay_diagnostics::RelayDiagnosticsSnapshot` —
// the shape the `relay_diagnostics` built-in projection emits under the
// snapshot key `"relay_diagnostics"`. The Rust projection pre-rolls every
// aggregate (active / EOSE'd / total sub counts, total events_rx) and pre-
// formats every display string (relative-time labels, role / connection /
// auth labels + semantic tones).
//
// Thin-shell rule: these are pure DTOs. The shell renders fields directly —
// it does NOT filter / sort / reduce wireSubscriptions, does NOT compute
// `Date(timeIntervalSince1970:)` from `lastEventAtMs`, does NOT switch on
// `state == "open"` to pick a color. All of that is in the Rust projection
// (aim.md §4.5 / §6 anti-pattern #1 / §"Where do views live?" — line 241).
//
// Extracted from `KernelSnapshotTypes.swift` to satisfy the 500-LOC
// file-size hard-cap gate (AGENTS.md).

/// Per-wire-subscription enriched row.
struct RelayDiagnosticsWireSub: Decodable, Identifiable, Equatable {
    let wireId: String
    let shortWireId: String
    let relayUrl: String
    let filterSummary: String
    let stateLabel: String
    let stateTone: String
    let consumerCountLabel: String
    let eventsRxDisplay: String?
    let eoseObserved: Bool
    /// Unix epoch ms (0 = none); shell renders via `relativeTimeFromUnixSeconds`
    /// (ADR-0032). opened / last-event / EOSE timestamps.
    let openedMs: UInt64
    let lastEventMs: UInt64
    let eoseMs: UInt64
    let closeReason: String?
    var id: String { wireId }
}

/// ADR-0051 relay-information document (NIP-11), mirror of the Rust
/// `RelayDiagnosticsInfo` projection. Carried on `RelayDiagnosticsRow.info`;
/// `nil` until `nmp-nip11` has fetched it (or the relay serves no document).
///
/// Thin-shell rule: pure DTO. Every `Option<String>` decodes to `nil` when the
/// relay did not advertise the field (JSON `null` / typed `has_* == false`); the
/// three `limitation` booleans are tri-state (`nil` = not advertised). The shell
/// renders these directly — no HTTP, no JSON parsing, no NIP-11 awareness.
///
/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// (`KernelHandle.decode`) maps the Rust `#[derive(Serialize)]` snake_case keys
/// (`supported_nips` / `payment_required` / `auth_required` /
/// `restricted_writes`) onto these camelCase properties on the JSON-fallback
/// path. The typed path (`TypedProjectionGlue.relayDiagnosticsInfo`) builds this
/// via the memberwise initializer and never touches the decoder.
struct RelayDiagnosticsInfo: Decodable, Equatable {
    let name: String?
    let description: String?
    let icon: String?
    let pubkey: String?
    let contact: String?
    let software: String?
    let version: String?
    let supportedNips: [UInt32]
    let paymentRequired: Bool?
    let authRequired: Bool?
    let restrictedWrites: Bool?
}

/// One entry in the per-relay bounded NOTICE log (mirror of `RelayDiagnosticsNotice`).
/// `atMs` carries wall-clock Unix epoch milliseconds; shells format as "Xs ago"
/// via `relativeTimeFromUnixSeconds` (aim.md §62). `text` is pre-truncated to
/// 180 chars at the Rust capture site.
struct RelayDiagnosticsNotice: Decodable, Identifiable, Equatable {
    let atMs: UInt64
    let text: String
    /// Stable identity for `ForEach` — use timestamp as tie-breaker with text.
    var id: String { "\(atMs)-\(text)" }
}

/// One routing provenance reason explaining why a relay was placed in the plan.
/// Mirrors the Rust `RelayConnectionReason` struct.
///
/// `kind` is a stable machine tag for icon/tone lookups; `label` is the
/// pre-formatted human string the shell renders directly.
/// `tone` is the semantic hue key (`"ok"` / `"warn"` / `"accent"` / `"muted"`).
/// `authorPubkeys` carries the (capped) author pubkey list; `authorTotal` is
/// the exact total count. `kindsLabel` is the pre-formatted kinds string for
/// interest reasons. `sourceEventId` carries the hint origin event id when known.
struct RelayConnectionReason: Decodable, Equatable {
    let kind: String
    let label: String
    let tone: String
    let authorPubkeys: [String]
    let authorTotal: UInt32
    let kindsLabel: String
    let sourceEventId: String?
}

/// One rolled-up relay row.
struct RelayDiagnosticsRow: Decodable, Identifiable, Equatable {
    let relayUrl: String
    let shortUrl: String
    let roleLabel: String
    let roleTone: String
    let connectionLabel: String
    let connectionTone: String
    let authLabel: String
    let authTone: String
    let totalSubCount: UInt32
    let activeSubCount: UInt32
    let eosedSubCount: UInt32
    let totalEventsRx: UInt64
    let totalEventsDisplay: String
    let reconnectCount: UInt32
    let bytesRxDisplay: String?
    let bytesTxDisplay: String?
    /// Unix epoch ms (0 = none); shell renders via `relativeTimeFromUnixSeconds`.
    /// last-connect / last-event timestamps.
    let lastConnectedMs: UInt64
    let lastEventMs: UInt64
    let lastNotice: String?
    /// Total NOTICE frames received from this relay (session counter).
    let noticeCount: UInt64
    /// Bounded NOTICE log, newest first (up to 32 entries). Each entry carries
    /// a wall-clock Unix-ms timestamp; shells format via `relativeTimeFromUnixSeconds`.
    let notices: [RelayDiagnosticsNotice]
    let lastError: String?
    let wireSubs: [RelayDiagnosticsWireSub]
    /// ADR-0051 — the relay's NIP-11 information document; `nil` until
    /// `nmp-nip11` has fetched it (or the relay serves no document). On the JSON
    /// path this decodes from `info: null`; on the typed path the child-table
    /// presence is the discriminator (no `has_info` flag).
    let info: RelayDiagnosticsInfo?
    /// Routing provenance reasons (SPLIT A, pre-block). Empty before the first
    /// compile or when no attribution is available. The `"blocked"` entry is
    /// always first when the relay is in the user's kind:10006 block list.
    let reasons: [RelayConnectionReason]
    var id: String { relayUrl }
}

/// Logical interest with semantic tone pre-classified.
struct RelayDiagnosticsInterest: Decodable, Identifiable, Equatable {
    let key: String
    let state: String
    let stateTone: String
    let refcount: UInt32
    let cacheCoverage: String
    let relayUrls: [String]
    var id: String { key }
}

/// Top-level diagnostics snapshot.
struct RelayDiagnosticsSnapshot: Decodable, Equatable {
    let relays: [RelayDiagnosticsRow]
    let interests: [RelayDiagnosticsInterest]

    static let empty = RelayDiagnosticsSnapshot(relays: [], interests: [])
}
