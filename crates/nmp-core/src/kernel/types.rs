//! Pure data types shared across kernel sub-modules.
//!
//! Holds all struct/enum definitions with no behaviour of their own: `StoredEvent`,
//! Profile, `TimelineItem`, `ProfileCard`, view payloads, relay health/status, wire
//! subscription state, counters, and the `AuthorRelayList` cache entry.

use super::{BTreeSet, CanonicalRelayUrl, HashMap, HashSet, Instant, RelayRole, Serialize};

// ── Event read-cache ──────────────────────────────────────────────────────────

/// Lightweight read-cache entry for timeline ordering and display.
///
/// The `EventStore` is the single authoritative writer (D4).  This struct is
/// populated **only** after `EventStore::insert` returns `Inserted | Replaced`.
#[derive(Clone, Debug)]
pub(super) struct StoredEvent {
    pub(super) id: String,
    pub(super) author: String,
    pub(super) kind: u32,
    pub(super) created_at: u64,
    pub(super) tags: Vec<Vec<String>>,
    pub(super) content: String,
    pub(super) relay_count: u32,
}

// ── Profile cache ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub(in crate::kernel) struct Profile {
    pub(super) event_id: String,
    pub(in crate::kernel) created_at: u64,
    /// The verbatim display-name value from kind:0
    /// (`display_name` / `displayName` / `name`, first non-empty wins).
    /// Empty string when the parsed metadata carries none of those fields
    /// — NO `short_npub` fallback is substituted (aim.md §2 — projection
    /// builders convert the empty string to `Option::None` at the
    /// projection boundary).
    pub(super) display: String,
    /// Raw picture URL from kind:0. `None` when no kind:0 has arrived OR
    /// the parsed metadata carries no `picture` field (or the value does
    /// not begin with `http`). Surfaced as `Option<String>` to projection
    /// builders verbatim — no identicon placeholder is substituted in the
    /// cache (aim.md §2 — presentation layer chooses the missing-picture
    /// strategy).
    pub(super) picture_url: Option<String>,
    pub(super) nip05: String,
    pub(super) about: String,
    /// NIP-57 lightning address (`lud16`) or LNURL (`lud06`) from this
    /// pubkey's kind:0 metadata. `None` when no kind:0 has arrived or the
    /// metadata had no lnurl. Pre-extracted at parse time (see
    /// `nostr::parse_profile`) so derived projections (`TimelineItem`,
    /// `ProfileCard`) don't re-traverse raw event JSON.
    pub(super) lnurl: Option<String>,
}

// ── Timeline and view payloads ────────────────────────────────────────────────

/// A single item in a timeline or thread view.
///
/// Carries raw protocol data only (aim.md §2 — NMP is a data framework;
/// projection and snapshot code sends raw pubkeys as hex, timestamps as
/// Unix seconds, and surfaces kind:0-derived fields as `Option<String>`
/// — `None` when no kind:0 has arrived). Presentation layers own all
/// formatting decisions (bech32 encoding, abbreviation, avatar
/// initials/tint, relative-time labels, placeholder/identicon strategy
/// for the missing-picture case).
// V6 Stage 3 — Swift `TimelineItem` Decodable codegen. Widened from
// `pub(super)` to `pub(crate)` so the feature-gated
// `pub(crate) use ... as TimelineItemForCodegen` re-export in
// `kernel/mod.rs` can lift the type out of `kernel`'s parent-module
// visibility ceiling and reach `crate::codegen_schema::dump_pilot_schemas`.
// Crate-private encapsulation is preserved — `TimelineItem` is still
// invisible outside `nmp-core` (no `pub use` further up). Mirrors the
// Stage 1 pattern used for `RelayStatus`, `Metrics`,
// `WireSubscriptionStatus`, and `LogicalInterestStatus`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
pub(crate) struct TimelineItem {
    pub(super) id: String,
    /// Author Nostr pubkey, hex (64 chars). Presentation layer formats
    /// for display.
    pub(super) author_pubkey: String,
    /// Author picture URL from kind:0. `None` when no kind:0 has arrived
    /// or the metadata carries no `picture` field — presentation layer
    /// chooses a placeholder/identicon strategy.
    pub(super) author_picture_url: Option<String>,
    /// NIP-57 lightning address (`lud16`) or LNURL (`lud06`) from the
    /// author's kind:0 metadata. `None` when the author has no lightning
    /// address or their kind:0 hasn't arrived yet. Pre-extracted so the
    /// shell zap button doesn't need to cross-reference a separate profile
    /// lookup — thin-shell rule, Rust decides zapability.
    pub(super) author_lnurl: Option<String>,
    /// Author display name from kind:0, if the kernel has it cached.
    /// `None` when no kind:0 has arrived yet — the presentation layer
    /// formats `author_pubkey` as a short hex abbreviation in that case.
    /// Baked directly into the timeline item so the renderer has the name
    /// without a separate profile-claim lifecycle.
    pub(super) author_display_name: Option<String>,
    /// Nostr event kind (e.g. 1 = note, 6 = repost, 7 = reaction). Carried so
    /// the shell can render kind-conditional UI (badges, navigation targets)
    /// without re-parsing the raw event JSON in `content`. D1 / thin-shell:
    /// the kind is the authoritative protocol signal — never inferred from
    /// content shape in native code.
    pub(super) kind: u32,
    pub(super) content: String,
    pub(super) content_preview: String,
    /// Event `created_at` (Unix seconds). Presentation layer formats for
    /// display (aim.md §2).
    pub(super) created_at: u64,
    pub(super) relay_count: u32,
    /// `true` when `kind == 6` (NIP-18 repost). Thin-shell: the view layer
    /// flips the "Repost" badge and re-routes thread navigation on this bool;
    /// it MUST NOT switch on `kind` itself (re-parsing protocol semantics in
    /// the UI is exactly the violation aim.md §6.9 forbids).
    pub(super) is_repost: bool,
    /// Event id the shell should route to when the row is tapped. For a
    /// kind:1 note this is `id`; for a kind:6 repost it is the inner kind:1's
    /// id when the embedded NIP-18 JSON is well-formed, falling back to `id`
    /// when it is missing or malformed (D1: best-effort). The shell binds
    /// this verbatim — no `?? id` fallback, no JSON parsing in Swift.
    pub(super) nav_target_id: String,
    /// Inner-note text the shell renders inside a kind:6 repost cell. For a
    /// kind:1 note this is `""` (the cell uses `content` directly); for a
    /// kind:6 it is the inner event's `content` field when the embedded JSON
    /// parses, falling back to `""` when it is missing or malformed (D1). The
    /// shell uses this string verbatim — no JSON parsing, no `?? ""` fallback.
    pub(super) repost_inner_content: String,
}

/// Profile summary card.
///
/// Carries the raw kind:0 fields with `Option<String>` semantics — `None`
/// signals "no kind:0 has arrived yet for this field" so presentation
/// layers can choose their own fallback (typically formatting the raw
/// pubkey). aim.md §2 — NMP is a data framework; backend ships raw
/// protocol data, presentation layers own formatting.
#[derive(Clone, Debug, Serialize)]
pub(super) struct ProfileCard {
    pub(super) pubkey: String,
    // D6 / ADR-0032: `npub` (bech32) field removed — projection sends raw hex
    // pubkey only; shells encode bech32 host-side via `nmp_app_encode_profile`
    // or their own implementation. Closes V-115.
    /// Display name from kind:0 (`display_name` / `displayName` / `name`,
    /// first non-empty wins). `None` when no kind:0 has arrived yet —
    /// presentation layer renders its own fallback.
    pub(super) display_name: Option<String>,
    /// Picture URL from kind:0. `None` when no kind:0 has arrived yet
    /// or the metadata carries no `picture` field — presentation layer
    /// chooses a placeholder/identicon strategy.
    pub(super) picture_url: Option<String>,
    pub(super) nip05: String,
    pub(super) about: String,
    /// Pre-extracted lightning address (`lud16`) / LNURL (`lud06`) from
    /// this pubkey's kind:0 metadata. `None` when no kind:0 has arrived
    /// or the user has no lightning address. The zap button in the shell
    /// is enabled/disabled based on this field — Rust decides
    /// zapability, the shell renders it.
    pub(super) lnurl: Option<String>,
}

impl ProfileCard {
    /// Build a card from a lightweight `mention_profiles` payload.
    /// `nip05`/`about` are empty, `lnurl` is None — the mention projection
    /// never carries them.
    pub(in crate::kernel) fn from_mention(pubkey: &str, m: &MentionProfilePayload) -> Self {
        Self {
            pubkey: pubkey.to_string(),
            display_name: m.display_name.clone(),
            picture_url: m.picture_url.clone(),
            nip05: String::new(),
            about: String::new(),
            lnurl: None,
        }
    }
}

// V-112 (ADR-0042): ProfileDispatchSpec, ProfileAction, AuthorViewPayload,
// ThreadViewPayload deleted — the author_view / thread_view kernel projections
// are removed. Profile display for the author screen now comes from the
// resolved_profiles (claimed_profiles) projection.

/// Per-author payload bundled into the `mention_profiles` projection.
///
/// Carries raw protocol identifiers + raw kind:0 fields only (aim.md §2 —
/// NMP is a data framework; presentation layers own bech32 encoding,
/// abbreviation, avatar initials/tint, and any "no kind:0 yet" fallback).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct MentionProfilePayload {
    /// Hex pubkey (64 chars) for the mentioned author. Carried in the
    /// struct body so shells consuming a flat array of payloads do not
    /// lose provenance (the map key alone is not enough when the payload
    /// flows through a JSON-serialised projection).
    pub(super) pubkey: String,
    /// Display name from kind:0 (`display_name` / `displayName` / `name`,
    /// first non-empty wins). `None` when no kind:0 has arrived yet for
    /// this author — presentation layer renders its own fallback.
    pub(super) display_name: Option<String>,
    /// Picture URL from kind:0. `None` when no kind:0 has arrived yet
    /// or the metadata carries no `picture` field.
    pub(super) picture_url: Option<String>,
}


// ── Relay health and wire subscription state ──────────────────────────────────
// V6 Stage 1 — visibility widened from `pub(super)` to `pub(crate)` so the
// feature-gated `crate::codegen_schema` re-export can name the type (Rust's
// `pub(super)` is parent-module-only and cannot be re-exported beyond it
// even with `pub(crate) use`). Crate-private encapsulation is preserved —
// nothing outside `nmp-core` can see the type. See `crate::codegen_schema`.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
pub(crate) struct RelayStatus {
    pub(super) role: String,
    pub(super) relay_url: String,
    pub(super) connection: String,
    pub(super) auth: String,
    pub(super) negentropy_probe: String,
    pub(super) active_wire_subscriptions: usize,
    pub(super) reconnect_count: u32,
    pub(super) last_connected_at_ms: Option<u128>,
    pub(super) last_event_at_ms: Option<u128>,
    pub(super) last_notice: Option<String>,
    pub(super) last_error: Option<String>,
    /// Machine-readable category for `last_error`. Closed key set:
    /// `auth_required | transient | permanent | malformed_event | policy_denied`.
    /// `None` when `last_error` is empty. Lets iOS branch on error *class*
    /// without substring-matching the English `last_error` prose.
    pub(super) error_category: Option<String>,
    pub(super) events_rx: u64,
    pub(super) bytes_rx: u64,
    pub(super) bytes_tx: u64,
    /// T120 (G8 / G11): relay has denied this client by policy
    /// (NIP-01 CLOSED reason `restricted:`, `blocked:`, or `shadowbanned:`).
    /// Set once a denial classification arrives; surfaces in diagnostics so
    /// UIs and reconnect workers can suppress retries against this relay.
    pub(super) denied: bool,
    /// T120 (G8 / G11): diagnostic key for the most recent NIP-01 CLOSED
    /// reason prefix (`auth-required`, `rate-limited`, `restricted`, …) —
    /// matches `CloseReason::as_key()`. `None` until the first classified
    /// CLOSED frame arrives.
    pub(super) last_close_reason: Option<String>,
    /// ADR-0051 — the relay's NIP-11 information document, once `nmp-nip11`
    /// has fetched it for this URL. `None` until the fetch resolves (or if
    /// the relay serves no document). The carried-through `RelayInfoDoc` is
    /// substrate-generic transport metadata (D0).
    ///
    /// Excluded from the V6 Stage-1 Swift `KernelTypes.generated.swift` mirror
    /// (`#[schemars(skip)]`): that emitter renders flat-record types only, and a
    /// nested `Option<RelayInfoDoc>` is Stage-2/3 scope (see
    /// `crates/nmp-codegen/src/swift.rs` + `docs/architecture-audit/
    /// v6-codegen-plan.md`). iOS reads `info` through the `relay_diagnostics`
    /// projection — both the authoritative serde-JSON subtree and the `KRDG`
    /// typed FlatBuffers sidecar (`InfoRow`) — not through this flat mirror, so
    /// skipping it from the schema costs the shell nothing. `serde` still
    /// serialises the field; only the JSON *schema* omits it.
    #[cfg_attr(feature = "codegen-schema", schemars(skip))]
    pub(super) info: Option<crate::substrate::RelayInfoDoc>,
}

// V6 Stage 1 — visibility widened from `pub(super)` to `pub(crate)` for
// `crate::codegen_schema` re-export. See `RelayStatus` above.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
pub(crate) struct WireSubscriptionStatus {
    pub(super) wire_id: String,
    pub(super) relay_url: String,
    pub(super) filter_summary: String,
    pub(super) state: String,
    pub(super) logical_consumer_count: u32,
    pub(super) events_rx: u64,
    pub(super) opened_at_ms: u128,
    pub(super) last_event_at_ms: Option<u128>,
    pub(super) eose_at_ms: Option<u128>,
    pub(super) close_reason: Option<String>,
}

// V6 Stage 1 — visibility widened from `pub(super)` to `pub(crate)` for
// `crate::codegen_schema` re-export. See `RelayStatus` above.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
pub(crate) struct LogicalInterestStatus {
    pub(super) key: String,
    pub(super) state: String,
    pub(super) refcount: u32,
    pub(super) relay_urls: Vec<String>,
    pub(super) cache_coverage: String,
    pub(super) warming_until_ms: Option<u128>,
}

/// User-facing projection of publish intents that have not finished.
///
/// This is derived from the publish engine's in-flight snapshot; the UI never
/// reconstructs retry policy or relay state from logs.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct PublishOutboxItem {
    pub(super) handle: String,
    pub(super) event_id: String,
    pub(super) kind: u32,
    pub(super) title: String,
    pub(super) preview: String,
    /// Raw Unix-seconds creation timestamp. ADR-0032: projection sends raw
    /// epoch seconds; shells format for display with their own locale/TZ.
    /// Replaces the deprecated `created_at_display` string (V-115).
    pub(super) created_at: u64,
    pub(super) status: String,
    /// Pre-formatted English label for `status` (e.g. `"Sending"`, `"Retrying"`).
    /// Doctrine §6 anti-pattern #1: the shell renders this directly — it never
    /// switches on `status` to choose a label string. Always non-empty.
    pub(super) status_label: String,
    /// SF Symbol name for the row icon, pre-classified from `kind`. The shell
    /// renders this verbatim via `Image(systemName:)` so it never branches on
    /// the Nostr kind number — `kind` is a protocol concept that belongs in
    /// Rust (aim.md §4.4 / §6 anti-pattern: "kind-number switches in views").
    /// Always non-empty (default `"doc.text"`).
    pub(super) system_image: String,
    /// Pre-decided "is the Retry button enabled" flag. The kernel knows the
    /// retry-policy rule ("a row already sending cannot be retried"); the
    /// shell never reconstructs it. RMP bible commandment #4 — no native `if`
    /// deciding what the app should *do*.
    pub(super) can_retry: bool,
    pub(super) target_relays: usize,
    // ADR-0032 / V-115: `target_summary` removed — shells compose "N relays ·
    // <formatted time>" themselves from `target_relays` + `created_at`.
    pub(super) relays: Vec<PublishOutboxRelay>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct PublishOutboxRelay {
    pub(super) relay_url: String,
    pub(super) status: String,
    /// Pre-formatted English label for `status` (e.g. `"Sending"`, `"Retrying"`).
    /// Always non-empty — the shell never `.capitalized`s `status` or switches
    /// on it to choose a label string.
    pub(super) status_label: String,
    pub(super) attempt: u32,
    /// Pre-formatted "try N" badge — empty string when `attempt` is zero so
    /// the shell renders unconditionally (D1: best-effort rendering — no
    /// `if attempt > 0` deciding whether to show the badge). When non-empty
    /// the shell renders it as-is.
    pub(super) attempt_label: String,
    pub(super) message: String,
    /// Pre-formatted "why was this relay targeted?" string, computed by the
    /// outbox resolver at publish time and carried verbatim through the
    /// snapshot. Examples: `"NIP-65 write relay"`, `"App relay (local config)"`,
    /// `"Inbox relay for <hex pubkey>"` (raw hex — D6 forbids backend
    /// projections from calling `display::*` abbreviation helpers; the shell
    /// applies its own `short_npub` / bech32 rendering). Empty when the publish predates this
    /// projection field (older persisted rows) — `skip_serializing_if` keeps
    /// the JSON payload shape unchanged in that case so apps that don't yet
    /// read the field stay forward-compatible.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(super) relay_reason: String,
}

/// Pre-formatted outbox summary header for `NotificationsView` (and similar
/// shells). The kernel owns the counters AND the user-facing English strings;
/// the shell only binds the strings.
///
/// Doctrine §6 anti-pattern #1 ("Duplicated formatting logic across platforms")
/// and RMP bible commandment #4 ("no native business logic"). The shell never
/// counts `publish_outbox` entries by status to derive a subtitle; it reads
/// `outbox_summary.subtitle` directly.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct OutboxSummarySnapshot {
    /// Pre-formatted headline — e.g. `"Nothing waiting"`, `"3 pending
    /// publishes"`, or `"1 pending publish"`. Always non-empty (D1).
    pub(super) title: String,
    /// Pre-formatted explanatory subtitle that decomposes per-status counts
    /// into a single sentence. Always non-empty (D1).
    pub(super) subtitle: String,
    pub(super) total: u32,
    pub(super) sending: u32,
    pub(super) retrying: u32,
    pub(super) queued: u32,
    pub(super) failed: u32,
}

/// Per-relay rolling counters for diagnostics.
#[derive(Clone, Debug, Default)]
pub(super) struct Counters {
    pub(super) frames_rx: u64,
    pub(super) events_rx: u64,
    pub(super) eose_rx: u64,
    pub(super) notices_rx: u64,
    pub(super) closed_rx: u64,
    pub(super) bytes_rx: u64,
    pub(super) bytes_tx: u64,
}

// `WireSub` moved to `kernel/wire_sub.rs` (LOC cap; K3 Stage D1 added a field).
// Re-exported so the established `super::WireSub` / `types::WireSub` paths and
// the `WireSubscriptionState::subs` map below resolve unchanged.
pub(super) use super::wire_sub::WireSub;

/// Per-relay health state: connection status, timestamps, and counters.
#[derive(Clone, Debug)]
pub(super) struct RelayHealth {
    pub(super) connection: String,
    pub(super) connected_at: Option<Instant>,
    pub(super) last_event_at: Option<Instant>,
    pub(super) last_notice: Option<String>,
    pub(super) last_error: Option<String>,
    /// Machine-readable category for `last_error`. Closed key set:
    /// `auth_required | transient | permanent | malformed_event | policy_denied`
    /// (see [`crate::kernel::closed_reason`] for the constants). Stamped
    /// alongside `last_error` and cleared with it. Projected into
    /// `RelayStatus::error_category` by `status.rs`.
    pub(super) error_category: Option<String>,
    pub(super) reconnect_count: u32,
    pub(super) counters: Counters,
    /// NIP-42 per-relay auth state — diagnostic key matching ADR-0007 wire
    /// keys (`not_required` | `challenge_received` | `authenticating` |
    /// `authenticated` | `failed`). Mutated by `handle_auth_challenge` /
    /// `handle_auth_ok` per D8 (without bumping `changed_since_emit`).
    pub(super) auth: String,
    /// T120 (G8 / G11): set when the relay has denied this client by policy
    /// (NIP-01 CLOSED `restricted:` / `blocked:` / `shadowbanned:`). The
    /// reconnect/REQ machinery should treat a denied relay as offline-for-
    /// this-client; recovery is a fresh socket only (relay edit, etc.).
    pub(super) denied: bool,
    /// T120 (G8 / G11): the diagnostic key of the most recently classified
    /// NIP-01 CLOSED reason. `None` until the first classified frame arrives.
    pub(super) last_close_reason: Option<String>,
    /// T112 — negentropy probe state for this relay, as a diagnostic
    /// string key (`"unknown"` | `"probing"` | `"supported"` | `"unsupported"`).
    /// Negentropy is a generic relay-side reconciliation capability; its
    /// concrete NIP binding lives in a downstream protocol crate, so this
    /// substrate field stays NIP-agnostic. Stored as a plain string so
    /// `nmp-core` does not depend on any shell-side probe-state type (D0 —
    /// no cycle). Updated by the actor/observer layer via
    /// `Kernel::set_negentropy_probe_state` whenever the capability probe
    /// transitions; see `status.rs` for the projection into
    /// `RelayStatus::negentropy_probe`.
    pub(super) negentropy_probe_state: String,
}

impl Default for RelayHealth {
    fn default() -> Self {
        Self {
            connection: "offline".to_string(),
            connected_at: None,
            last_event_at: None,
            last_notice: None,
            last_error: None,
            error_category: None,
            reconnect_count: 0,
            counters: Counters::default(),
            auth: "not_required".to_string(),
            denied: false,
            last_close_reason: None,
            negentropy_probe_state: "unknown".to_string(),
        }
    }
}

// ── NIP-65 relay list cache ───────────────────────────────────────────────────

/// Cached kind:10002 relay list for an author.
///
/// `event_id` is used as a tiebreak when two events share the same `created_at`:
/// lexicographically smaller event id wins, mirroring the store's supersession
/// logic.
#[derive(Clone, Debug, Default)]
pub(super) struct AuthorRelayList {
    /// Event id of the kind:10002 that produced this relay list.
    pub(super) event_id: String,
    pub(super) created_at: u64,
    pub(super) read_relays: Vec<String>,
    pub(super) write_relays: Vec<String>,
    pub(super) both_relays: Vec<String>,
}

// V-68 / V-112 (ADR-0042): ViewInterest + AuthorViewState / ThreadViewState
// deleted here. View refcounting now lives in the planner's InterestRegistry
// (multi-owner `(scope, key)` slots) behind the generic open_interest seam.
// Author and thread view state now lives inside the per-app FlatFeed registered
// by nmp_app_chirp_open_author_feed / nmp_app_chirp_open_thread_feed.

/// Diagnostic ingest event counter.
///
/// M2 (ADR-0042): the production `open_firehose_tag` hashtag-feed verb was
/// deleted in favour of the generic `open_interest` C-ABI. The `interest` /
/// `seq` subscription-tracking fields went with it. What remains is the
/// `events` counter, kept because the `diag-firehose-` **test ingest seam**
/// (`should_store_event` line ~244 + the timeline-insert clause) is still
/// load-bearing test infrastructure — ~15 kernel test files drive events
/// through that prefix to bypass the follow-set gate with timeline-injection
/// semantics the generic `open_interest` deliberately does NOT replicate
/// (open_interest stays out of the home timeline). The counter feeds the
/// `diagnostic_firehose_events` snapshot field; keeping it avoids unrelated
/// FFI/codegen-Swift regen churn. Retiring the test seam itself is a separate
/// test-support refactor (tracked in V-112).
#[derive(Default)]
pub(super) struct DiagnosticFirehoseState {
    pub(super) events: u64,
}

// ── Kernel sub-state groupings (phase 2 god-struct decomposition) ─────────────
//
// V-112 (ADR-0042): `AuthorViewState` / `ThreadViewState` deleted.
// These continue the mechanical grouping started by `DiagnosticFirehoseState`:
// cohesive Kernel field clusters collapsed into named locatable units.
// Pure data — no behaviour of their own.

/// Profile-fetch request tracking: the in-flight / queued sets plus the
/// monotonic REQ-id sequence. Grouped because the three fields are always
/// mutated together by the `requests/profile.rs` claim request paths
/// (`claim_profile`, `pending_profile_claim_requests`, `profile_claim_request`,
/// `author_requests`) and read together by the `status.rs` profile diagnostics.
/// F-CR-00: `request_profile_for_rendered_note` (proactive ingest-time fetch)
/// was removed; kind:0 is now fetched only on component `claim_profile`.
#[derive(Default)]
pub(super) struct ProfileRequestState {
    /// Pubkeys whose kind:0 has been REQ'd (inflight or completed). A pubkey in
    /// this set is never re-requested.
    pub(super) requested: HashSet<String>,
    /// Pubkeys queued for kind:0 fetch because a profile claim or rendered note
    /// arrived before an outbound profile request was emitted. Drained by
    /// `pending_profile_claim_requests`.
    pub(super) pending: BTreeSet<String>,
    /// Monotonic counter feeding unique `profile-*` REQ sub-ids.
    pub(super) req_seq: u64,
}

/// FFI diagnostic timing milestones — `Option<Instant>` markers stamped once at
/// the first occurrence of each lifecycle event. Read as a unit by the
/// `update.rs` metrics assembly (via `elapsed_ms`) and `status.rs`. `None` until
/// the corresponding event happens.
#[derive(Default)]
pub(super) struct TimingMilestones {
    /// When `Kernel::start` first ran.
    pub(super) started_at: Option<Instant>,
    /// Byte-stable Unix-ms wall anchor (see `relay_diagnostics::event_to_unix_ms`).
    pub(super) started_unix_ms: Option<u64>,
    /// Most recent / first ingested event (drives `last_event_to_emit_ms`).
    pub(super) last_event_at: Option<Instant>,
    pub(super) first_event_at: Option<Instant>,
    /// When the target profile's kind:0 first loaded.
    pub(super) target_profile_loaded_at: Option<Instant>,
    /// When the timeline view was first opened / first item rendered.
    pub(super) timeline_opened_at: Option<Instant>,
    pub(super) timeline_first_item_at: Option<Instant>,
}

/// Wire (WebSocket) subscription bookkeeping. `subs` is the per-`(relay_url,
/// sub_id)` registry; `persistent` is the set of `(relay_url, sub_id)` pairs
/// that must survive EOSE (NWC-style long-lived listeners). Grouped because the
/// EOSE/CLOSED handlers in `ingest/mod.rs` and the REQ paths in `requests/`
/// touch both in lockstep — see the `wire_subs` field doc on `Kernel` for the
/// #170 relay-scoped-keying rationale.
#[derive(Default)]
pub(super) struct WireSubscriptionState {
    /// Wire-sub bookkeeping keyed by `(relay_url, sub_id)`.
    pub(super) subs: HashMap<(CanonicalRelayUrl, String), WireSub>,
    /// `(relay_url, sub_id)` pairs pinned open across EOSE.
    pub(super) persistent: HashSet<(CanonicalRelayUrl, String)>,
}

// ── Metrics snapshot ──────────────────────────────────────────────────────────
// V6 Stage 1 — visibility widened from `pub(super)` to `pub(crate)` for
// `crate::codegen_schema` re-export. See `RelayStatus` above.
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
pub(crate) struct Metrics {
    pub(super) generated_events: u64,
    pub(super) note_events: u64,
    pub(super) profile_events: u64,
    pub(super) duplicate_events: u64,
    pub(super) delete_events: u64,
    pub(super) stored_events: usize,
    pub(super) tombstones: usize,
    pub(super) visible_items: usize,
    pub(super) visible_profiled_items: usize,
    pub(super) visible_placeholder_avatar_items: usize,
    pub(super) open_views: u32,
    pub(super) events_since_last_update: u64,
    pub(super) diagnostic_firehose_events: u64,
    pub(super) inserted_count: usize,
    pub(super) updated_count: usize,
    pub(super) removed_count: usize,
    pub(super) events_per_second_configured: u32,
    pub(super) emit_hz_configured: u32,
    pub(super) update_sequence: u64,
    pub(super) estimated_store_bytes: usize,
    pub(super) payload_bytes: usize,
    pub(super) store_to_payload_ratio: f64,
    pub(super) actor_queue_depth: u32,
    pub(super) frames_rx: u64,
    pub(super) events_rx: u64,
    pub(super) eose_rx: u64,
    pub(super) notices_rx: u64,
    pub(super) closed_rx: u64,
    pub(super) bytes_rx: u64,
    pub(super) bytes_tx: u64,
    pub(super) contacts_authors: usize,
    pub(super) timeline_authors: usize,
    pub(super) first_event_ms: Option<u128>,
    pub(super) target_profile_loaded_ms: Option<u128>,
    pub(super) timeline_opened_ms: Option<u128>,
    pub(super) timeline_first_item_ms: Option<u128>,
    pub(super) update_emitted_ms: Option<u128>,
    pub(super) last_event_to_emit_ms: Option<u128>,
    pub(super) max_event_to_emit_ms: u128,
    pub(super) max_events_per_update: u64,
    /// T114b — diagnostic drop counter; under the current dual-channel design
    /// this is always zero (unbounded command channel cannot drop). Retained
    /// for API compatibility; survives `ActorCommand::Reset` via shared Arc.
    pub(super) dispatch_drops_total: u64,
    /// T114b — `claim_profile` drops on per-pubkey `MAX_CLAIMS_PER_PUBKEY`
    /// overflow. Kernel-lifetime counter; resets on `ActorCommand::Reset`
    /// (the cap is a per-kernel D8 invariant, not a process metric).
    pub(super) claim_drops_total: u64,
    /// Microseconds spent in `make_update` on the PREVIOUS tick (one-tick lag,
    /// same as `payload_bytes`): full time from `emit_started` through the
    /// FlatBuffers encode call. Covers projection builds + encode.
    /// Zero on the first tick. Feed to per-session p50/p95/p99 diagnostics.
    pub(super) make_update_us: u128,
    /// Microseconds spent in FlatBuffers encoding alone on the PREVIOUS
    /// tick (one-tick lag). Combined with `make_update_us` this lets callers
    /// separate "building the snapshot tree" from "encoding it for transport".
    pub(super) serialize_us: u128,
    /// Count of update-frame encoding/decoding degradations observed by the
    /// Rust transport boundary. This is intentionally monotonic for the kernel
    /// lifetime so malformed or impossible value-shape drift becomes visible in
    /// diagnostics instead of collapsing to an empty/null snapshot.
    pub(super) update_frame_degradations_total: u64,
}

pub(crate) use super::negentropy_types::NegentropySyncStats;
pub(super) use super::negentropy_types::AVG_EVENT_BYTES;

// ── Update envelope ───────────────────────────────────────────────────────────
/// Full snapshot of kernel state encoded into the host update frame each tick.
/// Named `KernelSnapshot` (not `KernelUpdate`) to avoid ambiguity with the
/// public `crate::app::KernelUpdate` lifecycle-event enum.
// ADR-0044 — widened from `pub(super)` to `pub(crate)` so the transport layer
// (`crate::update_envelope`) can populate the typed Tier-3 `SnapshotFrame`
// fields directly from this struct instead of re-walking the generic JSON
// `payload`. Doctrinally fine: these are framework-owned envelope types, and
// ADR-0044 §2 explicitly endorses the transport schema coupling to them.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct KernelSnapshot {
    pub(super) rev: u64,
    /// Snapshot schema version (`KERNEL_SCHEMA_VERSION`). Lets a shell detect
    /// a kernel-vs-shell schema mismatch and degrade gracefully (D1) instead
    /// of mis-decoding a renamed/removed/retyped field.
    pub(super) schema_version: u32,
    /// Unix-epoch milliseconds at the moment this snapshot was emitted.
    /// A consuming shell can detect actor-thread death by observing this
    /// field stop advancing.
    ///
    /// `dispatch_command` panics are deliberately *not* wrapped in
    /// `catch_unwind` (a command panic is a genuine bug that must stay
    /// visible). From the shell's side that manifests as the update channel
    /// going permanently silent — no error, no toast, no crash report. A
    /// shell that watches this field can convert that silent freeze into an
    /// observable staleness signal.
    pub(super) last_tick_ms: u64,
    pub(super) update_kind: &'static str,
    pub(super) running: bool,
    // D0: the views cluster (`profile`, the visible timeline, `author_view`,
    // `thread_view`, and the `inserted` / `updated` / `removed` deltas) is
    // app-shaped social view state — NOT a protocol-neutral kernel primitive.
    // There are NO typed fields for them. All seven are surfaced through the
    // host-extensible `projections` map below under the built-in keys
    // `"profile"`, `"timeline"`, `"author_view"`, `"thread_view"`,
    // `"inserted"`, `"updated"`, and `"removed"`: a shell reads
    // `projections.timeline` etc. instead of a baked-in kernel field. The
    // generic typed-field name `items` is deliberately renamed to the more
    // descriptive `"timeline"` projection key. Like the publish cluster and
    // the identity pair, these are kernel-owned domain state, so `make_update`
    // inserts them into the map directly after running the host-registered
    // projection closures.
    pub(super) metrics: Metrics,
    pub(super) relay_status: RelayStatus,
    pub(super) relay_statuses: Vec<RelayStatus>,
    pub(super) logical_interests: Vec<LogicalInterestStatus>,
    pub(super) wire_subscriptions: Vec<WireSubscriptionStatus>,
    pub(super) logs: Vec<String>,
    // D0: identity output (`accounts`, `active_account`) is no longer a typed
    // `KernelSnapshot` field set. `AccountSummary` stays a substrate type in
    // `nmp-core`, but the *snapshot output* for the account list and the
    // active-account handle is surfaced through the host-extensible
    // `projections` map below under the built-in keys `"accounts"` and
    // `"active_account"` — a shell reads `projections.accounts` /
    // `projections.active_account` instead of a baked-in kernel field. This
    // mirrors the publish cluster and the `"wallet"` / `"bunker_handshake"`
    // projections: `make_update` inserts both keys directly after running the
    // host-registered projection closures.
    //
    // D0: the publish/relay-settings cluster (`publish_queue`,
    // `publish_outbox`, `configured_relays`, `relay_role_options`) is app-shaped
    // relay/publish state — NOT a protocol-neutral kernel primitive. There are
    // NO typed fields for them. They are surfaced through the host-extensible
    // `projections` map below under their built-in keys: a shell reads
    // `projections.publish_queue` etc.
    // instead of a baked-in kernel field. Unlike the host-registered `"wallet"`
    // / `"bunker_handshake"` projections, these three are kernel-owned domain
    // state, so `make_update` inserts them into the map directly after running
    // the host-registered projection closures.
    pub(super) last_error_toast: Option<String>,
    /// Machine-readable category for `last_error_toast`. Closed key set:
    /// `auth_required | transient | permanent | malformed_event | policy_denied`
    /// (see [`crate::kernel::closed_reason`]). `None` when `last_error_toast`
    /// is empty or was set via the legacy uncategorized path. Lets iOS branch
    /// on error class without parsing the English toast string.
    pub(super) last_error_category: Option<String>,
    /// #171 (D6) — last genuine structural planner error recorded by
    /// `SubscriptionLifecycle::last_planner_error()`, surfaced so the host
    /// observes it instead of silent empty frames. `null` in steady state.
    pub(super) last_planner_error: Option<String>,
    /// V-67 (D6) — set when the kernel was asked to open a durable store at a
    /// specific path but the open failed. The kernel fell back to an ephemeral
    /// in-memory store, so all locally-stored events are transient for this
    /// session. `null` in the healthy case AND when no storage path was
    /// configured (in-memory is the legitimate default for tests/CI).
    ///
    /// The host MUST surface this to the user (e.g. an alert or a persistent
    /// banner) so they are not surprised when events are missing on next launch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) store_open_failure: Option<String>,
    /// V-66 (D3) — set to `true` when the kernel has an active account but
    /// `configured_relays` is empty, meaning every outbound connection falls
    /// back to `FALLBACK_CONTENT_RELAY` / `FALLBACK_INDEXER_RELAY` without
    /// user consent. The fallback still operates so the app stays functional,
    /// but the host MUST surface this diagnostic (e.g. a banner: "No relays
    /// configured — using defaults") so the user knows their publish target.
    ///
    /// Absent from the wire (`skip_serializing_if`) when the condition is not
    /// active: a kernel with no active account, or one whose `configured_relays`
    /// is non-empty, emits no field — wire stays byte-for-byte identical to
    /// pre-V-66 snapshots in the healthy case.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) no_configured_relays: Option<bool>,
    /// GAP-5: NIP-agnostic negentropy session statistics. Accumulates across the
    /// most-recent reconciliation session; the NIP-77 runtime pushes raw counts
    /// via `Kernel::set_negentropy_sync_stats` on session completion. Zero-default
    /// until the first session completes. Omitted from JSON when all counts are zero
    /// and `last_reconcile_at_ms` is `None` (pre-first-session, wire-backwards-compat).
    pub(super) negentropy_sync_stats: NegentropySyncStats,
    // D0: NIP-47 NWC is an app noun — there is NO typed `wallet_status` field.
    // Wallet state is surfaced through the host-registered `"wallet"` snapshot
    // projection (see `projections` below): a shell reads `projections.wallet`
    // instead of a baked-in kernel field. This was the first internal consumer
    // of the snapshot-projection seam.
    //
    // D0: NIP-46 remote signing is an app noun — there is likewise NO typed
    // `bunker_handshake` field. Handshake state is surfaced through the
    // built-in `"bunker_handshake"` snapshot projection: a shell reads
    // `projections.bunker_handshake` instead of a baked-in kernel field.
    /// Host-registered and built-in projection data. Each host-registered
    /// projection closure runs on every tick and appends a namespaced JSON
    /// value under its key. Host keys are host-chosen (e.g. `"market.listings"`,
    /// `"todo.items"`).
    ///
    /// `make_update` also inserts the kernel-owned built-in projections after
    /// running the host closures: `"publish_queue"`, `"publish_outbox"`,
    /// `"configured_relays"`, and `"relay_role_options"` — the publish /
    /// relay-settings cluster (D0: relay/publish state is an app noun, not a
    /// typed `KernelSnapshot` field); `"accounts"` /
    /// `"active_account"` — the identity pair; and `"profile"`, `"timeline"`,
    /// `"author_view"`, `"thread_view"`, `"inserted"`, `"updated"`,
    /// `"removed"` — the views cluster (D0: social view state is an app noun).
    /// A host projection that registers one of those reserved keys is
    /// overwritten by the built-in value (built-in wins) so the kernel-owned
    /// projections are always authoritative.
    ///
    /// This is the output-side counterpart to the action-registry seam: a
    /// non-social app extends the snapshot with its own namespace WITHOUT
    /// editing `KernelSnapshot`'s typed social fields. Append-only and
    /// `skip_serializing_if` empty — a shell that predates this field simply
    /// never sees the key (backwards compatible, D1).
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub(super) projections: std::collections::HashMap<String, serde_json::Value>,
}

// ── Claimed-event projection payload ──────────────────────────────────────────

/// Per-event payload bundled into the `claimed_events` snapshot
/// projection. Surfaces the raw protocol fields a renderer needs to
/// resolve an embed without re-walking the store on the FFI side.
///
/// Keyed by `primary_id` in the projection map:
/// - hex-64 event id for nevent/note URIs (matches `StoredEvent.id`),
/// - `kind:pubkey:d_tag` coordinate string for naddr URIs (matches the
///   renderer-side `WireUri.primary_id`).
///
/// D0 — the name is intentionally generic ("event", not "embed"); the
/// kernel primitive that drives this projection is `claim_event` and
/// can carry any kind, not just embed-class events.
///
/// Mirrors the `MentionProfilePayload` projection pattern: `pub(crate)`
/// struct with `pub(super)` fields, serialised through
/// `serde_json::to_value` from `kernel/update.rs`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(crate) struct ClaimedEventDto {
    /// The projection key — either a hex-64 event id (nevent/note) or
    /// `kind:pubkey:d_tag` (naddr). Carried in the body too so a shell
    /// consuming a flat array of payloads keeps provenance.
    pub(super) primary_id: String,
    /// Canonical 64-hex event id of the resolved event (always the
    /// concrete event id, even when the URI was an addressable
    /// coordinate).
    pub(super) id: String,
    /// Author pubkey, hex (64 chars). Presentation layer formats for
    /// display.
    pub(super) author_pubkey: String,
    /// Author's display name from kind:0, if the kernel has it cached.
    /// `None` means the kind:0 hasn't been ingested yet — the renderer
    /// composes with `NostrProfileName` which falls back to the
    /// truncated npub. Mirrors `TimelineItem::author_display_name`.
    pub(super) author_display_name: Option<String>,
    /// Author's picture URL from kind:0, if the kernel has it cached.
    /// `None` means kind:0 absent — `NostrAvatar` falls back to an
    /// identicon. Mirrors `TimelineItem::author_picture_url`.
    pub(super) author_picture_url: Option<String>,
    /// Event kind.
    pub(super) kind: u32,
    /// Unix-seconds `created_at`. Presentation layer formats relative
    /// time.
    pub(super) created_at: u64,
    /// Raw event tags. Renderers walk these for embed-specific fields
    /// (NIP-23 title, summary, image).
    pub(super) tags: Vec<Vec<String>>,
    /// Raw event content. NIP-23 article body, kind:1 note text, etc.
    pub(super) content: String,
}

impl ClaimedEventDto {
    /// Build a `ClaimedEventDto` from a kernel-side `StoredEvent`,
    /// stamping the caller-provided `primary_id` (which may be either
    /// the event id verbatim or an addressable coordinate string).
    /// Author profile fields default to `None`; the projection builder
    /// in `kernel/update.rs` enriches them from
    /// `Kernel::profile_for_pubkey`.
    pub(super) fn from_stored(primary_id: String, e: &StoredEvent) -> Self {
        Self {
            primary_id,
            id: e.id.clone(),
            author_pubkey: e.author.clone(),
            author_display_name: None,
            author_picture_url: None,
            kind: e.kind,
            created_at: e.created_at,
            tags: e.tags.clone(),
            content: e.content.clone(),
        }
    }

    /// Stamp the author's display name + picture URL from the kernel's
    /// profile cache. `None` fields stay `None` when the kernel has no
    /// kind:0 for the author yet — the renderer composes with
    /// `NostrProfileName` / `NostrAvatar` which fall back to truncated
    /// npub + identicon.
    pub(super) fn with_author_profile(
        mut self,
        display_name: Option<String>,
        picture_url: Option<String>,
    ) -> Self {
        self.author_display_name = display_name;
        self.author_picture_url = picture_url;
        self
    }
}
