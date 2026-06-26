//! `GroupTimelineProjection` — the read-side of a NIP-29 group-chat screen.
//!
//! This is **pure consumption**: a [`ObservedProjectionSink`] that accumulates the
//! h-tagged user-content events of a single group and serialises them as a
//! flat, newest-first event list for a native shell to render. It registers
//! no actions, mints no FFI symbols, and never touches the actor loop.
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
//! `GroupTimelineProjection` is built to sit on *both*: it implements
//! `ObservedProjectionSink` for ingest, and exposes [`GroupTimelineProjection::snapshot`]
//! — a cheap, non-blocking, no-argument read — so the host can encode it into a
//! typed sidecar and register it as
//!
//! ```ignore
//! let projection = Arc::new(GroupTimelineProjection::new(group_id));
//! let observer_id = app.open_observed_projection(ObservedProjection::from_shape(
//!     Arc::clone(&projection) as Arc<dyn ObservedProjectionSink>,
//!     "nmp.nip29.group_timeline",
//!     1,
//!     group_timeline_shape,
//!     128,
//! ));
//! let snap = Arc::clone(&projection);
//! app.register_typed_snapshot_projection("nmp.nip29.group_timeline", move || {
//!     let snapshot = snap.snapshot();
//!     Some(nmp_core::TypedProjectionData {
//!         key: "nmp.nip29.group_timeline".to_string(),
//!         payload: encode_group_timeline_snapshot(&snapshot),
//!         ..Default::default()
//!     })
//! });
//! ```
//!
//! Wiring that closure is the host app composition crate's job (a separate
//! PR); this crate only provides the projection type and a snapshot read
//! that is safe to call from inside a tick.
//!
//! ## D8 — non-blocking
//!
//! [`GroupTimelineProjection::snapshot_json`] runs on the actor thread inside the
//! snapshot tick. It takes one uncontended `Mutex` lock and clones a small
//! `Vec` — no I/O, no relay round-trips, no event-store scan. The accumulation
//! work (`on_kernel_event`) is likewise a single lock + map insert.
//!
//! ## What this projection can and cannot filter
//!
//! NIP-29 group identity is the **pair** `(host_relay_url, local_id)`
//! (`group_id.rs`). A [`KernelEvent`] carries only `id, author, kind,
//! created_at, tags, content` — it has **no relay-provenance metadata**. So the
//! projection can only filter on what is *in the event*: the kind and the
//! `["h", local_id]` tag. Restricting ingest to the group's host relay is an
//! upstream routing concern — the `relay_pin` lane on `LogicalInterest` /
//! `ViewDependencies` — not something this observer can or should re-check.
//! A correctly-pinned subscription only ever delivers events from the host
//! relay; this projection trusts that pin and matches on `local_id` alone.

use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::ObservedProjectionSink;
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{h_tag_value, KIND_CHAT_MESSAGE, KIND_DISCUSSION_OR_ARTIFACT};

/// One rendered group-chat event in a [`GroupTimelineSnapshot`].
///
/// A flat carrier — threading / reply nesting is deliberately *not* modelled
/// here. The read screen this feeds is a linear chat log; a threaded view is a
/// separate projection. Fields are the minimum a shell needs to draw a row,
/// in raw form (aim.md §2 — presentation layer formats pubkeys and
/// timestamps; backend ships hex + Unix seconds).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct GroupTimelineEvent {
    /// Event id (hex). Also the dedupe key inside the projection.
    pub id: String,
    /// Author pubkey (hex, 64 chars) — `KernelEvent::author`.
    pub pubkey: String,
    /// Event content, verbatim.
    pub content: String,
    /// Event `created_at` (Unix seconds).
    pub created_at: u64,
    /// Event kind — one of 9 (chat), 11 (thread).
    pub kind: u32,
}

impl GroupTimelineEvent {
    /// Build a event row from a kernel event. The caller is responsible for
    /// having already checked kind + `h`-tag membership.
    fn from_event(event: &KernelEvent) -> Self {
        Self {
            id: event.id.clone(),
            pubkey: event.author.clone(),
            content: event.content.clone(),
            created_at: event.created_at,
            kind: event.kind,
        }
    }
}

/// The serialised read-model a group-chat screen consumes.
///
/// `events` is ordered **newest-first** (`created_at` descending). Ties on
/// `created_at` are broken by event id descending so the order is total and
/// deterministic across snapshot ticks.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct GroupTimelineSnapshot {
    pub events: Vec<GroupTimelineEvent>,
}

impl GroupTimelineSnapshot {
    /// An empty snapshot — what a freshly-constructed projection (or a
    /// poisoned internal mutex, D6) reports.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            events: Vec::new(),
        }
    }
}

/// Accumulates a single NIP-29 group's chat-content events into a newest-first
/// event list.
///
/// Construct with the target [`GroupId`]; register the same `Arc` as a
/// [`ObservedProjectionSink`] (ingest) and capture it in a snapshot-projection
/// closure (output). Only events whose kind is 9 / 11 **and** whose
/// `["h", …]` tag value equals the group's `local_id` are retained.
pub struct GroupTimelineProjection {
    /// The group this projection reads. Only `local_id` is matched against
    /// event `h` tags; `host_relay_url` is retained for callers that want to
    /// echo the group identity but is *not* an event-level filter (a
    /// `KernelEvent` carries no relay provenance — see the module docs).
    group_id: GroupId,
    /// Accepted events keyed by event id. Idempotent: re-delivering an event
    /// replaces the prior value rather than duplicating it. Bounded by
    /// [`MAX_PROJECTION_MESSAGES`] — once full, the oldest-by-insertion entry
    /// is evicted, keeping per-projection memory and per-tick snapshot
    /// serialisation O(cap) rather than O(session). Ordering for the snapshot
    /// is applied on read, not here.
    events: Mutex<BoundedMessageMap<String, GroupTimelineEvent>>,
}

impl GroupTimelineProjection {
    /// Construct a projection scoped to `group_id`. The event store starts
    /// empty; events arrive via [`ObservedProjectionSink::on_kernel_event`].
    #[must_use]
    pub fn new(group_id: GroupId) -> Self {
        Self {
            group_id,
            events: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    /// The group this projection is scoped to.
    pub fn group_id(&self) -> &GroupId {
        &self.group_id
    }

    /// Whether `event` belongs in this projection: a chat-content kind
    /// (9 / 11) carrying an `["h", local_id]` tag matching this group.
    ///
    /// Moderation kinds (9000–9009), user-management (9021/9022), and
    /// relay-signed metadata (39000–39003) are deliberately excluded — this is
    /// a chat *read* model, not a moderation log.
    fn accepts(&self, event: &KernelEvent) -> bool {
        let kind_ok = matches!(event.kind, KIND_CHAT_MESSAGE | KIND_DISCUSSION_OR_ARTIFACT);
        if !kind_ok {
            return false;
        }
        // Strictest reading: an event with no `h` tag, or an `h` tag for a
        // different group, is not part of this group's chat. The host-relay
        // pin guarantees provenance; the `h` tag value is the in-group key.
        h_tag_value(&event.tags) == Some(self.group_id.local_id.as_str())
    }

    /// Snapshot the current event set as a typed [`GroupTimelineSnapshot`],
    /// ordered newest-first.
    ///
    /// D6: a poisoned mutex degrades to [`GroupTimelineSnapshot::empty`] rather
    /// than panicking — this can run on the actor thread inside a snapshot
    /// tick, where a panic would unwind the kernel.
    #[must_use]
    pub fn snapshot(&self) -> GroupTimelineSnapshot {
        let Ok(events) = self.events.lock() else {
            return GroupTimelineSnapshot::empty();
        };
        let mut ordered: Vec<GroupTimelineEvent> = events.values().cloned().collect();
        // Newest-first. Tie-break on id (descending) so the order is total and
        // stable across ticks even when two events share a `created_at`.
        ordered.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        GroupTimelineSnapshot { events: ordered }
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

impl ObservedProjectionSink for GroupTimelineProjection {
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
        events.insert(event.id.clone(), GroupTimelineEvent::from_event(event));
    }
}

#[cfg(test)]
#[path = "group_timeline/tests.rs"]
mod tests;
