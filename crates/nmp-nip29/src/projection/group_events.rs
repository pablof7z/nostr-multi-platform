//! `GroupEventsProjection` — the read-side of a NIP-29 group screen.
//!
//! This is **pure consumption**: a [`ObservedProjectionSink`] that accumulates
//! the h-tagged events of a single group (constrained to a consumer-declared
//! kind set) and serialises them as a flat, newest-first event list for a native
//! shell to render. It registers no actions, mints no FFI symbols, and never
//! touches the actor loop.
//!
//! NIP-29 owns ONLY the `["h", local_id]` routing concern (issue #2187). The
//! consumer declares **which group AND which kinds** it wants via a
//! [`GroupEventsQuery`]; the projection is kind-agnostic infrastructure that
//! honours that declaration. A "chat view" is simply a consumer that asks for
//! kinds `{9, 11}`; a discussion-plus-comments view asks for `{9, 11, 1111}`.
//!
//! ## How it plugs into the snapshot seam
//!
//! NMP has two complementary extension seams (see
//! `nmp_core::kernel::snapshot_registry` and
//! `nmp_core::actor::commands::event_observer`):
//!
//! - **`ObservedProjectionSink`** — the *ingest* side. `on_kernel_event` fires
//!   once per accepted event on the actor thread; a projection accumulates the
//!   facts it cares about into its own interior-mutable state.
//! - **`register_typed_snapshot_projection`** — the *output* side (ADR-0037).
//!   A host registers a no-argument closure that runs on every snapshot tick
//!   and returns a typed FlatBuffers sidecar (`TypedProjectionData`) under a
//!   host-chosen key, or `None` when there is no changed row to emit.
//!
//! `GroupEventsProjection` is built to sit on *both*: it implements
//! `ObservedProjectionSink` for ingest, and exposes
//! [`GroupEventsProjection::snapshot`] — a cheap, non-blocking, no-argument read
//! — so the host can encode it into a typed sidecar.
//!
//! Wiring that closure is the host app composition crate's job through the
//! NIP-29 group-events typed read session; this crate only provides the
//! projection type and a snapshot read that is safe to call from inside a tick.
//!
//! ## D8 — non-blocking
//!
//! [`GroupEventsProjection::snapshot_json`] runs on the actor thread inside the
//! snapshot tick. It takes one uncontended `Mutex` lock and clones a small
//! `Vec` — no I/O, no relay round-trips, no event-store scan. The accumulation
//! work (`on_kernel_event`) is likewise a single lock + map insert.
//!
//! ## What this projection can and cannot filter
//!
//! NIP-29 group identity is the **pair** `(host_relay_url, local_id)`
//! (`group_id.rs`). A [`KernelEvent`] carries only `id, author, kind,
//! created_at, tags, content` — it has **no relay-provenance metadata**. So the
//! projection can only filter on what is *in the event*: the kind (against the
//! query's [`GroupEventKinds`](crate::group_query::GroupEventKinds)) and the
//! `["h", local_id]` tag. Restricting ingest to the group's host relay is an
//! upstream routing concern — the `relay_pin` lane on `LogicalInterest` /
//! `ViewDependencies` — not something this observer can or should re-check.
//! A correctly-pinned subscription only ever delivers events from the host
//! relay; this projection trusts that pin and matches on `local_id` + kind
//! alone. The kind gate stays even when the relay interest already constrains
//! kinds, because cache replay / store hydration / test injection can deliver
//! other same-`h` kinds.

use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::ObservedProjectionSink;
use serde::{Deserialize, Serialize};

use crate::group_query::GroupEventsQuery;
use crate::kinds::h_tag_value;
use crate::reply::parse_reply_edges;

/// One rendered group event in a [`GroupEventsSnapshot`].
///
/// A flat carrier. Fields are the minimum a shell needs to draw a row — plus
/// the NIP-10 reply / thread edges (`reply_to` / `root`) so a consumer can
/// render reply chips (scroll-to-parent) and a thread view. All values are raw
/// (aim.md §2 — presentation layer formats pubkeys and timestamps; backend
/// ships hex + Unix seconds).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct GroupEvent {
    /// Event id (hex). Also the dedupe key inside the projection.
    pub id: String,
    /// Author pubkey (hex, 64 chars) — `KernelEvent::author`.
    pub pubkey: String,
    /// Event content, verbatim.
    pub content: String,
    /// Event `created_at` (Unix seconds).
    pub created_at: u64,
    /// Event kind — whatever the consumer's query admitted.
    pub kind: u32,
    /// Raw hex id of the immediate parent this event replies to (the NIP-10
    /// `reply` marker, or the deprecated positional parent). `None` for a
    /// thread root / standalone post. Equals [`Self::root`] for a direct reply
    /// to the thread root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    /// Raw hex id of the thread root this event belongs to (the NIP-10 `root`
    /// marker, or the deprecated positional root). `None` for a thread root /
    /// standalone post.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

impl GroupEvent {
    /// Build an event row from a kernel event. The caller is responsible for
    /// having already checked kind + `h`-tag membership.
    ///
    /// The NIP-10 reply / thread edges are parsed kind-agnostically from the
    /// event's `e` tags (see [`crate::reply`]).
    fn from_event(event: &KernelEvent) -> Self {
        let edges = parse_reply_edges(&event.tags);
        Self {
            id: event.id.clone(),
            pubkey: event.author.clone(),
            content: event.content.clone(),
            created_at: event.created_at,
            kind: event.kind,
            reply_to: edges.reply_to,
            root: edges.root,
        }
    }
}

/// The serialised group read model.
///
/// `events` is ordered **newest-first** (`created_at` descending). Ties on
/// `created_at` are broken by event id descending so the order is total and
/// deterministic across snapshot ticks. The snapshot is deliberately minimal:
/// it does NOT echo the group id or the kind set — that opener-owned state is
/// not part of the read model.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct GroupEventsSnapshot {
    pub events: Vec<GroupEvent>,
}

impl GroupEventsSnapshot {
    /// An empty snapshot — what a freshly-constructed projection (or a
    /// poisoned internal mutex, D6) reports.
    #[must_use]
    pub fn empty() -> Self {
        Self { events: Vec::new() }
    }
}

/// Accumulates a single NIP-29 group's events (for a consumer-declared kind
/// set) into a newest-first event list.
///
/// Construct with the target [`GroupEventsQuery`]; register the same `Arc` as a
/// [`ObservedProjectionSink`] (ingest) and capture it in a snapshot-projection
/// closure (output). Only events whose `["h", …]` tag value equals the query's
/// group `local_id` **and** whose kind is admitted by the query's
/// [`GroupEventKinds`](crate::group_query::GroupEventKinds) are retained.
pub struct GroupEventsProjection {
    /// The consumer's query — group identity (`local_id` matched against event
    /// `h` tags; `host_relay_url` is not an event-level filter) plus the kind
    /// selection. The SAME query the composer used to build the relay-interest
    /// `filter_json`, so the projection's accept predicate and the wire filter
    /// can never diverge.
    query: GroupEventsQuery,
    /// Accepted events keyed by event id. Idempotent: re-delivering an event
    /// replaces the prior value rather than duplicating it. Bounded by
    /// [`MAX_PROJECTION_MESSAGES`] — once full, the oldest-by-insertion entry
    /// is evicted, keeping per-projection memory and per-tick snapshot
    /// serialisation O(cap) rather than O(session). Ordering for the snapshot
    /// is applied on read, not here.
    events: Mutex<BoundedMessageMap<String, GroupEvent>>,
}

impl GroupEventsProjection {
    /// Construct a projection scoped to `query`. The event store starts empty;
    /// events arrive via [`ObservedProjectionSink::on_kernel_event`].
    #[must_use]
    pub fn new(query: GroupEventsQuery) -> Self {
        Self {
            query,
            events: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    /// The query this projection is scoped to.
    #[must_use]
    pub fn query(&self) -> &GroupEventsQuery {
        &self.query
    }

    /// Whether `event` belongs in this projection: an `["h", local_id]` tag
    /// matching this group's `local_id` **and** a kind admitted by the query's
    /// kind selection.
    ///
    /// The kind gate is retained even though the relay interest already filters
    /// on kinds: cache replay / store hydration / test injection can deliver
    /// other same-`h` kinds, and this projection must not leak them into a
    /// kind-scoped read.
    fn accepts(&self, event: &KernelEvent) -> bool {
        // Strictest reading: an event with no `h` tag, or an `h` tag for a
        // different group, is not part of this group. The host-relay pin
        // guarantees provenance; the `h` tag value is the in-group key.
        if h_tag_value(&event.tags) != Some(self.query.group().local_id.as_str()) {
            return false;
        }
        self.query.accepts_kind(event.kind)
    }

    /// Snapshot the current event set as a typed [`GroupEventsSnapshot`],
    /// ordered newest-first.
    ///
    /// D6: a poisoned mutex degrades to [`GroupEventsSnapshot::empty`] rather
    /// than panicking — this can run on the actor thread inside a snapshot
    /// tick, where a panic would unwind the kernel.
    #[must_use]
    pub fn snapshot(&self) -> GroupEventsSnapshot {
        let Ok(events) = self.events.lock() else {
            return GroupEventsSnapshot::empty();
        };
        let mut ordered: Vec<GroupEvent> = events.values().cloned().collect();
        // Newest-first. Tie-break on id (descending) so the order is total and
        // stable across ticks even when two events share a `created_at`.
        ordered.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        GroupEventsSnapshot { events: ordered }
    }

    /// Snapshot as a `serde_json::Value` — the exact shape a host
    /// `register_snapshot_projection` closure must return.
    ///
    /// D6: a serialisation failure (not expected for this plain struct)
    /// collapses to `json!({"events": []})` rather than propagating.
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot())
            .unwrap_or_else(|_| serde_json::json!({ "events": [] }))
    }
}

impl ObservedProjectionSink for GroupEventsProjection {
    /// Ingest one accepted kernel event. Non-matching events (wrong kind,
    /// missing/foreign `h` tag) are ignored. Matching events are inserted by
    /// id, so a re-delivery replaces rather than duplicates.
    ///
    /// Cheap and panic-free, per the `ObservedProjectionSink` contract: a single
    /// uncontended lock + map insert. A poisoned mutex is a silent no-op (D6).
    fn on_kernel_event(&self, event: &KernelEvent) {
        if !self.accepts(event) {
            return;
        }
        let Ok(mut events) = self.events.lock() else {
            return;
        };
        events.insert(event.id.clone(), GroupEvent::from_event(event));
    }
}

#[cfg(test)]
#[path = "group_events/tests.rs"]
mod tests;
