//! Pure data types shared across kernel sub-modules.
//!
//! Holds all struct/enum definitions with no behaviour of their own: `StoredEvent`,
//! Profile, `TimelineItem`, `ProfileCard`, view payloads, relay health/status, wire
//! subscription state, counters, and the `AuthorRelayList` cache entry.

use super::{Serialize, RelayRole, CanonicalRelayUrl, Instant, BTreeSet, HashSet, HashMap};

// ── Seed accounts (test fixtures only) ──────────────────────────────────────

#[cfg(test)]
#[allow(dead_code)]
#[derive(Clone)]
pub(super) struct SeedAccount {
    pub(super) name: &'static str,
    pub(super) pubkey: &'static str,
}

#[cfg(test)]
#[allow(dead_code)]
pub(super) fn seed_accounts() -> Vec<SeedAccount> {
    use crate::relay::{FIATJAF_PUBKEY, JB55_PUBKEY, TEST_PUBKEY};
    vec![
        SeedAccount {
            name: "pablof7z",
            pubkey: TEST_PUBKEY,
        },
        SeedAccount {
            name: "fiatjaf",
            pubkey: FIATJAF_PUBKEY,
        },
        SeedAccount {
            name: "jb55",
            pubkey: JB55_PUBKEY,
        },
    ]
}

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
pub(super) struct Profile {
    pub(super) event_id: String,
    pub(super) created_at: u64,
    pub(super) display: String,
    /// Raw picture URL from kind:0. `None` while no kind:0 has arrived.
    /// At the `TimelineItem` / `ProfileCard` boundary this becomes a non-Option
    /// field backed by [`crate::substrate::placeholder::picture_placeholder`]
    /// (D1: display fields are always renderable).
    pub(super) picture_url: Option<String>,
    pub(super) nip05: String,
    pub(super) about: String,
    pub(super) avatar_initials: String,
    pub(super) avatar_color: String,
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
/// All display fields are non-`Option` (D1: best-effort rendering — placeholders
/// are part of the type contract).  `author_picture_url` carries either the
/// kind:0 picture URL or a deterministic `identicon:<pubkey-prefix>` URI when
/// no kind:0 has arrived.  The `author_avatar_source` field (`"kind0"` |
/// `"placeholder"`) lets the UI decide how to render without branching on
/// `Option`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct TimelineItem {
    pub(super) id: String,
    pub(super) author_pubkey: String,
    pub(super) author_display: String,
    /// Always non-empty (D1).  Either the kind:0 picture URL or an
    /// `identicon:<pubkey-prefix>` placeholder URI.
    pub(super) author_picture_url: String,
    pub(super) author_avatar_initials: String,
    pub(super) author_avatar_color: String,
    pub(super) author_avatar_source: String,
    /// NIP-57 lightning address (`lud16`) or LNURL (`lud06`) from the
    /// author's kind:0 metadata. `None` when the author has no lightning
    /// address or their kind:0 hasn't arrived yet. Pre-extracted so the
    /// shell zap button doesn't need to cross-reference a separate profile
    /// lookup — thin-shell rule, Rust decides zapability.
    pub(super) author_lnurl: Option<String>,
    /// Nostr event kind (e.g. 1 = note, 6 = repost, 7 = reaction). Carried so
    /// the shell can render kind-conditional UI (badges, navigation targets)
    /// without re-parsing the raw event JSON in `content`. D1 / thin-shell:
    /// the kind is the authoritative protocol signal — never inferred from
    /// content shape in native code.
    pub(super) kind: u32,
    pub(super) content: String,
    pub(super) content_preview: String,
    pub(super) created_at_display: String,
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
/// All display fields are non-`Option` (D1).  `picture_url` carries either the
/// kind:0 picture URL or an `identicon:<pubkey-prefix>` placeholder URI.
#[derive(Clone, Debug, Serialize)]
pub(super) struct ProfileCard {
    pub(super) pubkey: String,
    pub(super) npub: String,
    /// Pre-formatted, copy-button-ready short form of `npub` (or hex when
    /// `npub` was not encoded). Rust owns the truncation policy
    /// (`<first10>…<last8>`) so a Swift `truncatedNpub` helper would be
    /// pure render duplication (aim.md §6.9 — no business logic in native).
    /// Always non-empty when `npub` is non-empty.
    pub(super) npub_short: String,
    pub(super) display: String,
    /// Always non-empty (D1).  Either the kind:0 picture URL or an
    /// `identicon:<pubkey-prefix>` placeholder URI.
    pub(super) picture_url: String,
    pub(super) nip05: String,
    pub(super) about: String,
    pub(super) avatar_initials: String,
    pub(super) avatar_color: String,
    /// Avatar image provenance for ADR-0017.
    pub(super) source: String,
    /// True when a kind:0 metadata event has been received for this pubkey.
    /// False means the profile card is a placeholder pending relay response.
    pub(super) has_profile: bool,
    /// Pre-extracted lightning address (`lud16`) / LNURL (`lud06`) from this
    /// pubkey's kind:0 metadata. `None` when no kind:0 has arrived or the
    /// user has no lightning address. The zap button in the shell is
    /// enabled/disabled based on this field — Rust decides zapability,
    /// Swift renders it (thin-shell rule, aim.md §6.9).
    pub(super) lnurl: Option<String>,
}

/// Dispatch spec for a `ProfileAction` that fires a real write.
///
/// When this is `Some`, the shell wires the button to
/// `nmp_app_dispatch_action(namespace, body_json)` with no further logic. The
/// kernel/host registry decides what each namespace means; the shell never
/// switches on a `kind` field to pick an FFI symbol (aim.md §4.4: writes go
/// through the registered `ActionModule` family, not through hand-rolled
/// per-verb plumbing).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct ProfileDispatchSpec {
    /// Action namespace — registered via `ActionRegistry::register::<M>()`. Today:
    /// `nmp.follow` or `nmp.unfollow`.
    pub(super) namespace: &'static str,
    /// Serialised JSON body the action module expects. Rust formats this so
    /// the shell sends bytes the executor already validates.
    pub(super) body_json: String,
}

/// Primary action the shell may render for an open profile view.
///
/// `dispatch` carries the namespace+body the shell must send through
/// `nmp_app_dispatch_action` for write verbs (follow / unfollow). When
/// `dispatch` is `None` the action is a pure local-UI intent (open the edit
/// sheet) — there is no registered `ActionModule` for it. The shell branches on
/// `dispatch.is_some()`, not on the `kind` string, killing the §4.4 switch.
///
/// `icon_name` is the SF Symbol (or equivalent) the shell renders next to
/// `label`. Owning the icon name here keeps both `Label` text and icon
/// authored by Rust — the shell binds blindly.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct ProfileAction {
    /// Stable discriminator preserved for tests/diagnostics. The SHELL MUST
    /// NOT switch on this — use `dispatch` (Some → write) or its absence
    /// (None → local intent) instead.
    pub(super) kind: &'static str,
    pub(super) label: &'static str,
    pub(super) target_pubkey: String,
    /// SF Symbol name (iOS) the shell renders without further mapping.
    pub(super) icon_name: &'static str,
    /// Present when the action is a write — the shell wires straight through
    /// `nmp_app_dispatch_action`. Absent for local-UI intents (edit sheet).
    pub(super) dispatch: Option<ProfileDispatchSpec>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AuthorViewPayload {
    pub(super) pubkey: String,
    pub(super) state: String,
    pub(super) profile: ProfileCard,
    pub(super) items: Vec<TimelineItem>,
    pub(super) note_count: usize,
    /// Pre-formatted post-count string the shell binds verbatim
    /// (e.g. `"5"`). Trivial today but keeps the shell from interpolating
    /// any count derived from the items array (aim.md §6.9). Future
    /// localisation work plugs in here, not in Swift.
    pub(super) note_count_display: String,
    pub(super) primary_action: Option<ProfileAction>,
}

/// Per-author payload bundled into the `mention_profiles` projection.
///
/// This is the same shape Chirp's `MentionProfile` consumes; emitting it from
/// Rust kills the `Dictionary(uniqueKeysWithValues:)` derivation Swift was
/// doing at `ProfileView.swift:28-40` (aim.md §4.2: derived views are pure
/// functions of the event store and are emitted from the kernel, not
/// reconstructed by the shell).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct MentionProfilePayload {
    pub(super) display: String,
    pub(super) picture_url: String,
    pub(super) avatar_initials: String,
    pub(super) avatar_color: String,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ThreadViewPayload {
    pub(super) focused_event_id: String,
    pub(super) root_event_id: String,
    pub(super) state: String,
    pub(super) items: Vec<TimelineItem>,
    pub(super) previous_count: usize,
    pub(super) next_count: usize,
    /// Pre-formatted, ready-to-render label for the "Show N earlier" affordance.
    /// Empty string when `previous_count == 0`. Kernel owns pluralization
    /// (aim.md §6 anti-pattern #1: duplicated formatting belongs in Rust).
    pub(super) previous_count_label: String,
    /// Pre-formatted, ready-to-render label for the "N more replies" affordance.
    /// Empty string when `next_count == 0`. Same rationale as
    /// `previous_count_label`.
    pub(super) next_count_label: String,
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
    pub(super) nip77_negentropy: String,
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
    pub(super) created_at_display: String,
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
    /// Pre-formatted "N relays · <`created_at`>" header line (or "1 relay · …"
    /// — pluralization is server-side). The shell renders this verbatim
    /// instead of reconstructing the plural with a ternary on `target_relays`.
    pub(super) target_summary: String,
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

/// Active wire (WebSocket) subscription state.
///
/// T105: `relay_url` is the resolved wire target this sub was opened on. The
/// CLOSE frame for this sub-id must be routed back to the same `relay_url`
/// (the transport pool is URL-keyed, so closing on the wrong socket would
/// leave the original subscription open). `role` is the transport lane label.
pub(super) struct WireSub {
    pub(super) id: String,
    pub(super) role: RelayRole,
    /// Resolved relay URL this subscription was opened on (T105). The CLOSE
    /// frame for `id` must target this URL — the transport pool is URL-keyed
    /// and would otherwise leak the open subscription on the original relay.
    /// Canonical by construction: this field mirrors the `wire_subs` key half.
    pub(super) relay_url: CanonicalRelayUrl,
    pub(super) filter_summary: String,
    pub(super) state: String,
    pub(super) events_rx: u64,
    pub(super) opened_at: Instant,
    pub(super) last_event_at: Option<Instant>,
    pub(super) eose_at: Option<Instant>,
    pub(super) close_reason: Option<String>,
}

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
    /// T112 — NIP-77 negentropy probe state for this relay, as a diagnostic
    /// string key (`"unknown"` | `"probing"` | `"supported"` | `"unsupported"`).
    /// Stored as a plain string so `nmp-core` does not depend on any
    /// shell-side probe-state type (D0 — no cycle). Updated by the
    /// actor/observer layer via `Kernel::set_nip77_probe_state` whenever the
    /// NIP-77 capability probe transitions; see `status.rs` for the
    /// projection into `RelayStatus::nip77_negentropy`.
    pub(super) nip77_probe_state: String,
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
            nip77_probe_state: "unknown".to_string(),
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

// ── View interest (refcounted) ────────────────────────────────────────────────
/// Tracks an open view (author, thread, firehose) with a refcount.
///
/// Refcounting allows multiple view instances to share the same relay
/// subscription.  The subscription is closed only when the last claimant calls
/// `close_*`.
#[derive(Clone, Debug)]
pub(super) struct ViewInterest {
    pub(super) key: String,
    pub(super) refcount: u32,
}

// ── View-tracking sub-structs (D0 app-domain state) ───────────────────────────
//
// These group the kernel's view-tracking fields — app-domain state living in a
// protocol-neutral kernel — into named locatable units, making the D0 boundary
// explicit. Pure mechanical grouping: no behaviour of their own.

/// Author-view tracking: selected author, request-pending flag, sequence count.
#[derive(Default)]
pub(super) struct AuthorViewState {
    pub(super) selected_author: Option<ViewInterest>,
    pub(super) request_pending: bool,
    pub(super) seq: u64,
}

/// Thread-view tracking: selected thread, hydration queues, and inflight flags.
#[derive(Default)]
pub(super) struct ThreadViewState {
    pub(super) selected_thread: Option<ViewInterest>,
    pub(super) request_pending: bool,
    pub(super) seq: u64,
    pub(super) pending_ids: BTreeSet<String>,
    pub(super) requested_ids: HashSet<String>,
    pub(super) ids_inflight: bool,
    pub(super) pending_reply_targets: BTreeSet<String>,
    pub(super) requested_reply_targets: HashSet<String>,
    pub(super) replies_inflight: bool,
}

/// Diagnostic hashtag-firehose tracking: interest, sequence, and event counter.
#[derive(Default)]
pub(super) struct DiagnosticFirehoseState {
    pub(super) interest: Option<ViewInterest>,
    pub(super) seq: u64,
    pub(super) events: u64,
}

// ── Kernel sub-state groupings (phase 2 god-struct decomposition) ─────────────
//
// These continue the mechanical grouping started by `AuthorViewState` /
// `ThreadViewState` / `DiagnosticFirehoseState`: cohesive Kernel field clusters
// collapsed into named locatable units. Pure data — no behaviour of their own.

/// Profile-fetch request tracking: the in-flight / queued sets plus the
/// monotonic REQ-id sequence. Grouped because the three fields are always
/// mutated together by the `requests/profile.rs` claim/note-author request paths
/// (`claim_profile`, `pending_profile_claim_requests`, `profile_claim_request`,
/// `request_profile_for_rendered_note`, `author_requests`) and read together
/// by the `status.rs` profile diagnostics.
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
    /// Most recent ingested event (drives `last_event_to_emit_ms`).
    pub(super) last_event_at: Option<Instant>,
    /// First ingested event ever.
    pub(super) first_event_at: Option<Instant>,
    /// When the target profile's kind:0 first loaded.
    pub(super) target_profile_loaded_at: Option<Instant>,
    /// When the timeline view was first opened.
    pub(super) timeline_opened_at: Option<Instant>,
    /// When the first timeline item was rendered.
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
}

// ── Update envelope ───────────────────────────────────────────────────────────
/// Full JSON snapshot of kernel state emitted to the host on each tick.
/// Named `KernelSnapshot` (not `KernelUpdate`) to avoid ambiguity with the
/// public `crate::app::KernelUpdate` lifecycle-event enum.
#[derive(Clone, Debug, Serialize)]
pub(super) struct KernelSnapshot {
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
    // `publish_outbox`, `relay_edit_rows`, `relay_role_options`) is app-shaped
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
    /// `"relay_edit_rows"`, and `"relay_role_options"` — the publish /
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
