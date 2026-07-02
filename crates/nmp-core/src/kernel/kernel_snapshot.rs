//! The per-tick host update envelope and its metrics/timing sub-state.
//!
//! Owns [`KernelSnapshot`] — the full snapshot of kernel state encoded into the
//! host update frame each tick — together with [`Metrics`] (the diagnostic
//! counters it carries) and the two `Kernel` sub-state clusters that feed the
//! metrics: [`TimingMilestones`] and [`DiagnosticFirehoseState`].

use super::negentropy_types::NegentropySyncStats;
use super::relay_health::{LogicalInterestStatus, RelayStatus, WireSubscriptionStatus};
use super::{Instant, Serialize};

/// Diagnostic ingest event counter.
///
/// M2 (ADR-0076): the production `open_firehose_tag` hashtag-feed verb was
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
// V-112 (ADR-0076): `AuthorViewState` / `ThreadViewState` deleted.
// These continue the mechanical grouping started by `DiagnosticFirehoseState`:
// cohesive Kernel field clusters collapsed into named locatable units.
// Pure data — no behaviour of their own.

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

// ── Metrics snapshot ──────────────────────────────────────────────────────────
#[derive(Clone, Debug, Serialize)]
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
    /// T114b — `resolve_ref` drops on per-pubkey `MAX_CLAIMS_PER_PUBKEY`
    /// overflow. Kernel-lifetime counter; resets on `ActorCommand::Lifecycle(LifecycleCommand::Reset)`
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
    /// #2767 — command sends shed because the bounded actor inbox was full.
    /// Process-lifetime raw count (no saturation), preserved across `Reset`
    /// via the kernel's `command_drops` handle so a command burst is
    /// host-visible rather than silent.
    pub(super) command_drops: u64,
    /// #2767 — relay events shed because the actor's local relay backlog hit
    /// `RELAY_BACKLOG_CAP`. Process-lifetime raw count (no saturation),
    /// preserved across `Reset` via the kernel's `relay_backlog_drops` handle
    /// so a relay flood is host-visible rather than silent.
    pub(super) relay_backlog_drops: u64,
}

// ── Update envelope ───────────────────────────────────────────────────────────
/// Full snapshot of kernel state encoded into the host update frame each tick.
/// Named `KernelSnapshot` (not `KernelUpdate`) to avoid ambiguity with the
/// public `crate::app::KernelUpdate` lifecycle-event enum.
// ADR-0072 — widened from `pub(super)` to `pub(crate)` so the transport layer
// (`crate::update_envelope`) can populate the typed Tier-3 `SnapshotFrame`
// fields directly from this struct instead of re-walking the generic JSON
// `payload`. Doctrinally fine: these are framework-owned envelope types, and
// ADR-0072 §2 explicitly endorses the transport schema coupling to them.
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
    // D0: the views cluster (`profile`) is kernel-owned domain state surfaced
    // through the host-extensible `projections` map under the `"profile"` key.
    // V-112 (ADR-0076): `author_view` and `thread_view` deleted.
    // #1610: the JSON-era `"timeline"`, `"inserted"`, `"updated"`, `"removed"`
    // projection slots were removed from the codegen registry and from the Swift
    // shell surface; typed feeds ship through app-owned session keys.
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
    // `bunker_handshake` field. Handshake state is surfaced as a typed
    // FlatBuffers sidecar through the typed snapshot projection path.
}
