//! `GroupChatProjection` — the read-side of a NIP-29 group-chat screen.
//!
//! This is **pure consumption**: a [`KernelEventObserver`] that accumulates the
//! h-tagged user-content events of a single group and serialises them as a
//! flat, newest-first message list for a native shell to render. It registers
//! no actions, mints no FFI symbols, and never touches the actor loop.
//!
//! ## How it plugs into the snapshot seam
//!
//! NMP has two complementary extension seams (see
//! `nmp_core::kernel::snapshot_registry` and
//! `nmp_core::actor::commands::event_observer`):
//!
//! - **`KernelEventObserver`** — the *ingest* side. `on_kernel_event` fires
//!   once per accepted event on the actor thread; a projection accumulates the
//!   facts it cares about into its own interior-mutable state.
//! - **`register_snapshot_projection`** — the *output* side. A host registers
//!   a no-argument closure that runs on every snapshot tick and returns a
//!   `serde_json::Value` appended under a host-chosen key.
//!
//! `GroupChatProjection` is built to sit on *both*: it implements
//! `KernelEventObserver` for ingest, and exposes [`GroupChatProjection::snapshot_json`]
//! — a cheap, non-blocking, no-argument read — so the host can register it as
//!
//! ```ignore
//! let projection = Arc::new(GroupChatProjection::new(group_id));
//! let observer_id = app.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);
//! let snap = Arc::clone(&projection);
//! app.register_snapshot_projection("nmp.nip29.group_chat", move || snap.snapshot_json());
//! ```
//!
//! Wiring that closure is the host app composition crate's job (a separate
//! PR); this crate only provides the projection type and a snapshot read
//! that is safe to call from inside a tick.
//!
//! ## D8 — non-blocking
//!
//! [`GroupChatProjection::snapshot_json`] runs on the actor thread inside the
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
use nmp_core::KernelEventObserver;
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{h_tag_value, KIND_CHAT_MESSAGE, KIND_COMMENT, KIND_DISCUSSION_OR_ARTIFACT};

/// One rendered group-chat message in a [`GroupChatSnapshot`].
///
/// A flat carrier — threading / reply nesting is deliberately *not* modelled
/// here. The read screen this feeds is a linear chat log; a threaded view is a
/// separate projection. Fields are the minimum a shell needs to draw a row.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct GroupChatMessage {
    /// Event id (hex). Also the dedupe key inside the projection.
    pub id: String,
    /// Author pubkey (hex) — `KernelEvent::author`.
    pub pubkey: String,
    /// Event content, verbatim.
    pub content: String,
    /// Event `created_at` (unix seconds).
    pub created_at: u64,
    /// Event kind — one of 9 (chat), 11 (thread), 1111 (comment).
    pub kind: u32,
}

impl GroupChatMessage {
    /// Build a message row from a kernel event. The caller is responsible for
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
/// `messages` is ordered **newest-first** (`created_at` descending). Ties on
/// `created_at` are broken by event id descending so the order is total and
/// deterministic across snapshot ticks.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct GroupChatSnapshot {
    pub messages: Vec<GroupChatMessage>,
}

impl GroupChatSnapshot {
    /// An empty snapshot — what a freshly-constructed projection (or a poisoned
    /// internal mutex, D6) reports.
    #[must_use] 
    pub fn empty() -> Self {
        Self {
            messages: Vec::new(),
        }
    }
}

/// Accumulates a single NIP-29 group's chat-content events into a newest-first
/// message list.
///
/// Construct with the target [`GroupId`]; register the same `Arc` as a
/// [`KernelEventObserver`] (ingest) and capture it in a snapshot-projection
/// closure (output). Only events whose kind is 9 / 11 / 1111 **and** whose
/// `["h", …]` tag value equals the group's `local_id` are retained.
pub struct GroupChatProjection {
    /// The group this projection reads. Only `local_id` is matched against
    /// event `h` tags; `host_relay_url` is retained for callers that want to
    /// echo the group identity but is *not* an event-level filter (a
    /// `KernelEvent` carries no relay provenance — see the module docs).
    group_id: GroupId,
    /// Accepted messages keyed by event id. Idempotent: re-delivering an event
    /// replaces the prior value rather than duplicating it. Bounded by
    /// [`MAX_PROJECTION_MESSAGES`] — once full, the oldest-by-insertion entry
    /// is evicted, keeping per-projection memory and per-tick snapshot
    /// serialisation O(cap) rather than O(session). Ordering for the snapshot
    /// is applied on read, not here.
    messages: Mutex<BoundedMessageMap<String, GroupChatMessage>>,
}

impl GroupChatProjection {
    /// Construct a projection scoped to `group_id`. The message store starts
    /// empty; events arrive via [`KernelEventObserver::on_kernel_event`].
    #[must_use] 
    pub fn new(group_id: GroupId) -> Self {
        Self {
            group_id,
            messages: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    /// The group this projection is scoped to.
    pub fn group_id(&self) -> &GroupId {
        &self.group_id
    }

    /// Whether `event` belongs in this projection: a chat-content kind
    /// (9 / 11 / 1111) carrying an `["h", local_id]` tag matching this group.
    ///
    /// Moderation kinds (9000–9009), user-management (9021/9022), and
    /// relay-signed metadata (39000–39003) are deliberately excluded — this is
    /// a chat *read* model, not a moderation log.
    fn accepts(&self, event: &KernelEvent) -> bool {
        let kind_ok = matches!(
            event.kind,
            KIND_CHAT_MESSAGE | KIND_DISCUSSION_OR_ARTIFACT | KIND_COMMENT
        );
        if !kind_ok {
            return false;
        }
        // Strictest reading: an event with no `h` tag, or an `h` tag for a
        // different group, is not part of this group's chat. The host-relay
        // pin guarantees provenance; the `h` tag value is the in-group key.
        h_tag_value(&event.tags) == Some(self.group_id.local_id.as_str())
    }

    /// Snapshot the current message set as a typed [`GroupChatSnapshot`],
    /// ordered newest-first.
    ///
    /// D6: a poisoned mutex degrades to [`GroupChatSnapshot::empty`] rather
    /// than panicking — this can run on the actor thread inside a snapshot
    /// tick, where a panic would unwind the kernel.
    #[must_use]
    pub fn snapshot(&self) -> GroupChatSnapshot {
        let Ok(messages) = self.messages.lock() else {
            return GroupChatSnapshot::empty();
        };
        let mut ordered: Vec<GroupChatMessage> = messages.values().cloned().collect();
        // Newest-first. Tie-break on id (descending) so the order is total and
        // stable across ticks even when two events share a `created_at`.
        ordered.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        GroupChatSnapshot { messages: ordered }
    }

    /// Snapshot as a `serde_json::Value` — the exact shape a host
    /// `register_snapshot_projection` closure must return.
    ///
    /// D6: a serialisation failure (not expected for this plain struct)
    /// collapses to `json!({"messages": []})` rather than propagating.
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot())
            .unwrap_or_else(|_| serde_json::json!({ "messages": [] }))
    }
}

impl KernelEventObserver for GroupChatProjection {
    /// Ingest one accepted kernel event. Non-matching events (wrong kind,
    /// missing/foreign `h` tag) are ignored. Matching events are inserted by
    /// id, so a re-delivery replaces rather than duplicates.
    ///
    /// Cheap and panic-free, per the `KernelEventObserver` contract: a single
    /// uncontended lock + map insert. A poisoned mutex is a silent no-op (D6).
    fn on_kernel_event(&self, event: &KernelEvent) {
        if !self.accepts(event) {
            return;
        }
        let Ok(mut messages) = self.messages.lock() else {
            return;
        };
        messages.insert(event.id.clone(), GroupChatMessage::from_event(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// The group every test event in this module targets.
    fn group() -> GroupId {
        GroupId::new("wss://groups.example.com", "rust-nostr")
    }

    /// Build a `KernelEvent` with an explicit kind and tag set.
    fn event(id: &str, kind: u32, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
        KernelEvent {
            id: id.into(),
            author: format!("author-of-{id}"),
            kind,
            created_at,
            tags,
            content: format!("content of {id}"),
        }
    }

    /// `["h", local_id]` for the test group.
    fn h_tag(local_id: &str) -> Vec<Vec<String>> {
        vec![vec!["h".into(), local_id.into()]]
    }

    #[test]
    fn fresh_projection_yields_empty_snapshot() {
        let proj = GroupChatProjection::new(group());
        assert_eq!(proj.snapshot(), GroupChatSnapshot::empty());
        let json = proj.snapshot_json();
        assert_eq!(json, serde_json::json!({ "messages": [] }));
    }

    #[test]
    fn matching_chat_message_is_retained() {
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

        let snap = proj.snapshot();
        assert_eq!(snap.messages.len(), 1);
        let msg = &snap.messages[0];
        assert_eq!(msg.id, "e1");
        assert_eq!(msg.pubkey, "author-of-e1");
        assert_eq!(msg.content, "content of e1");
        assert_eq!(msg.created_at, 100);
        assert_eq!(msg.kind, KIND_CHAT_MESSAGE);
    }

    #[test]
    fn thread_and_comment_kinds_are_retained() {
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event(
            "thread",
            KIND_DISCUSSION_OR_ARTIFACT,
            10,
            h_tag("rust-nostr"),
        ));
        proj.on_kernel_event(&event("comment", KIND_COMMENT, 20, h_tag("rust-nostr")));

        let snap = proj.snapshot();
        assert_eq!(snap.messages.len(), 2);
        let kinds: Vec<u32> = snap.messages.iter().map(|m| m.kind).collect();
        assert!(kinds.contains(&KIND_DISCUSSION_OR_ARTIFACT));
        assert!(kinds.contains(&KIND_COMMENT));
    }

    #[test]
    fn event_for_a_different_group_is_excluded() {
        let proj = GroupChatProjection::new(group());
        // Correct kind, but the `h` tag names a different group.
        proj.on_kernel_event(&event("other", KIND_CHAT_MESSAGE, 100, h_tag("some-other-room")));
        assert!(proj.snapshot().messages.is_empty());
    }

    #[test]
    fn event_with_no_h_tag_is_excluded() {
        let proj = GroupChatProjection::new(group());
        // Correct kind, but no `h` tag at all — not a group event.
        proj.on_kernel_event(&event("loose", KIND_CHAT_MESSAGE, 100, vec![]));
        assert!(proj.snapshot().messages.is_empty());
    }

    #[test]
    fn off_kind_event_with_matching_h_tag_is_excluded() {
        let proj = GroupChatProjection::new(group());
        // kind 1 (plain note) and kind 9000 (a moderation action) both carry a
        // matching `h` tag, but neither is a chat-content kind.
        proj.on_kernel_event(&event("note", 1, 100, h_tag("rust-nostr")));
        proj.on_kernel_event(&event("modaction", 9000, 100, h_tag("rust-nostr")));
        assert!(proj.snapshot().messages.is_empty());
    }

    #[test]
    fn messages_are_ordered_newest_first() {
        let proj = GroupChatProjection::new(group());
        // Deliver out of chronological order.
        proj.on_kernel_event(&event("mid", KIND_CHAT_MESSAGE, 200, h_tag("rust-nostr")));
        proj.on_kernel_event(&event("old", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
        proj.on_kernel_event(&event("new", KIND_CHAT_MESSAGE, 300, h_tag("rust-nostr")));

        let snap = proj.snapshot();
        let ids: Vec<&str> = snap.messages.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["new", "mid", "old"]);
    }

    #[test]
    fn created_at_ties_break_on_id_descending() {
        let proj = GroupChatProjection::new(group());
        // Same `created_at` — order must still be total and deterministic.
        proj.on_kernel_event(&event("aaa", KIND_CHAT_MESSAGE, 500, h_tag("rust-nostr")));
        proj.on_kernel_event(&event("zzz", KIND_CHAT_MESSAGE, 500, h_tag("rust-nostr")));

        let snap = proj.snapshot();
        let ids: Vec<&str> = snap.messages.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["zzz", "aaa"]);
    }

    #[test]
    fn duplicate_event_id_is_not_duplicated() {
        let proj = GroupChatProjection::new(group());
        let evt = event("dup", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr"));
        proj.on_kernel_event(&evt);
        proj.on_kernel_event(&evt);

        let snap = proj.snapshot();
        assert_eq!(snap.messages.len(), 1, "re-delivered id must not duplicate");
    }

    #[test]
    fn snapshot_json_contains_the_messages() {
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
        proj.on_kernel_event(&event("e2", KIND_COMMENT, 200, h_tag("rust-nostr")));

        let json = proj.snapshot_json();
        let messages = json
            .get("messages")
            .and_then(|m| m.as_array())
            .expect("snapshot json has a `messages` array");
        assert_eq!(messages.len(), 2);
        // Newest-first: e2 (created_at 200) precedes e1 (created_at 100).
        assert_eq!(messages[0].get("id").and_then(|v| v.as_str()), Some("e2"));
        assert_eq!(messages[1].get("id").and_then(|v| v.as_str()), Some("e1"));
        // Field shape: `pubkey` carries `KernelEvent::author`.
        assert_eq!(
            messages[0].get("pubkey").and_then(|v| v.as_str()),
            Some("author-of-e2"),
        );
    }

    #[test]
    fn round_trips_through_serde() {
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
        let snap = proj.snapshot();
        let encoded = serde_json::to_string(&snap).expect("snapshot serialises");
        let decoded: GroupChatSnapshot =
            serde_json::from_str(&encoded).expect("snapshot deserialises");
        assert_eq!(snap, decoded);
    }

    #[test]
    fn drives_through_observer_trait_object() {
        // The projection must be usable as `Arc<dyn KernelEventObserver>` —
        // that is exactly how a host registers it with `register_event_observer`.
        let proj = Arc::new(GroupChatProjection::new(group()));
        let observer: Arc<dyn KernelEventObserver> = Arc::clone(&proj) as _;
        observer.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
        assert_eq!(proj.snapshot().messages.len(), 1);
    }

    #[test]
    fn group_id_accessor_returns_construction_value() {
        let proj = GroupChatProjection::new(group());
        assert_eq!(proj.group_id(), &group());
    }
}
