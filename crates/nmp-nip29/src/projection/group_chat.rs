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
use std::time::{SystemTime, UNIX_EPOCH};

use nmp_core::display::{avatar_color_hex, avatar_initials, format_ago_secs, short_hex, short_npub, to_npub};
use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::KernelEventObserver;
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{h_tag_value, KIND_CHAT_MESSAGE, KIND_DISCUSSION_OR_ARTIFACT};

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
    /// Pre-formatted abbreviated relative-time label for `created_at`
    /// (e.g. `"3s ago"`, `"12m ago"`, `"5h ago"`, `"2d ago"`). Computed in
    /// Rust at snapshot time via [`nmp_core::display::format_ago_secs`] so
    /// the host shell never reaches for `RelativeDateTimeFormatter` or any
    /// other date-formatting API (V-22 thin-shell fix — aim.md §2: display
    /// formatting is Rust-owned). Mirrors the V-20 fix on `DmMessage` in
    /// `nmp-nip17`.
    ///
    /// Computed against the snapshot's wall-clock "now" (read once per
    /// `snapshot()` call); stored as the empty string in the ingest-time
    /// `GroupChatMessage` and overwritten on every snapshot so the label is
    /// always fresh on render — never stale across ticks.
    #[serde(default)]
    pub created_at_display: String,
    /// Abbreviated bech32 pubkey for the chat row header (`"npub1<first10>…<last6>"`).
    /// Computed at ingest time from [`KernelEvent::author`] via
    /// [`nmp_core::display::short_npub`] — the V-33 canonical cross-surface
    /// helper — so the host shell never slices or encodes pubkeys itself
    /// (V-25 thin-shell fix — aim.md §2: display formatting is Rust-owned).
    #[serde(default)]
    pub author_display: String,
    /// Two-char uppercase initials for the avatar tile — first two chars of
    /// the bech32 body of the author's `npub1…` form, uppercased. Computed
    /// at ingest time via `avatar_initials(to_npub(pubkey_hex))` so the same
    /// author renders the same initials across timeline, DMs, and group chat.
    #[serde(default)]
    pub author_initials: String,
    /// Deterministic 6-hex avatar background colour from the author pubkey,
    /// uppercase, no `#` prefix. Computed at ingest time via
    /// [`nmp_core::display::avatar_color_hex`] — the canonical cross-surface
    /// djb2 helper. The same author renders with the same tint across every
    /// surface (DMs, NIP-29 group chat, the modular timeline, the Accounts
    /// toolbar, Marmot rows) — V-25 thin-shell fix, V-33 consolidation.
    #[serde(default)]
    pub author_color_hex: String,
    /// Event kind — one of 9 (chat), 11 (thread).
    pub kind: u32,
}

impl GroupChatMessage {
    /// Build a message row from a kernel event. The caller is responsible for
    /// having already checked kind + `h`-tag membership.
    ///
    /// `created_at_display` is left empty; the snapshot path (re)computes it
    /// against the snapshot's wall-clock "now" so the label is always fresh
    /// (V-22 — mirrors V-20's pattern on `DmMessage`).
    ///
    /// `author_display` / `author_initials` / `author_color_hex` are computed
    /// once at ingest from the event author (V-25) — they're pure functions
    /// of the pubkey hex and do not change across snapshot ticks, so unlike
    /// `created_at_display` they need no per-tick refresh.
    fn from_event(event: &KernelEvent) -> Self {
        Self {
            id: event.id.clone(),
            pubkey: event.author.clone(),
            content: event.content.clone(),
            created_at: event.created_at,
            created_at_display: String::new(),
            author_display: short_npub(&event.author),
            author_initials: avatar_initials(&to_npub(&event.author)),
            author_color_hex: avatar_color_hex(&event.author),
            kind: event.kind,
        }
    }
}

/// Wall-clock "now" in Unix seconds — the time source the production
/// [`GroupChatProjection::snapshot`] path uses to fill `created_at_display`.
///
/// D6: a clock that pre-dates the Unix epoch (impossible on any sane device)
/// degrades to `0`, which [`nmp_core::display::format_ago_secs`] renders as
/// `"now"`. Tests pin the clock via [`GroupChatProjection::snapshot_at`]
/// instead of reaching for this helper.
fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// First two characters of a NIP-29 `local_id`, uppercased — the
/// deterministic avatar-tile label for `PublicGroupRow` (V-29 thin-shell
/// fix). Mirrors the iOS `PublicGroupRow.initials` helper deleted in V-29.
///
/// For the empty string the label is `"?"` — the same fallback the deleted
/// Swift `guard !id.isEmpty else { return "?" }` enforced — so the host
/// always has a non-empty label to render. For 1-char inputs the available
/// prefix is returned (uppercased). Panic-free for any input — D6.
#[must_use]
fn group_initials(local_id: &str) -> String {
    if local_id.is_empty() {
        return "?".to_string();
    }
    local_id.chars().take(2).collect::<String>().to_uppercase()
}

/// The serialised read-model a group-chat screen consumes.
///
/// `messages` is ordered **newest-first** (`created_at` descending). Ties on
/// `created_at` are broken by event id descending so the order is total and
/// deterministic across snapshot ticks.
///
/// `group_initials` is the two-char uppercase avatar-tile label for the
/// PublicGroupRow header — the first two chars of `GroupId::local_id`,
/// uppercased (V-29 thin-shell fix). The iOS `PublicGroupRow` used to derive
/// it from `groupId.localId` in Swift; that derivation now lives in Rust so
/// the host shell never slices the local-id string itself (aim.md §2:
/// display formatting is Rust-owned). Mirrors `GroupChatMessage::author_initials`
/// (V-25) — same algorithm class, applied to the group identifier instead
/// of an author pubkey.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct GroupChatSnapshot {
    pub messages: Vec<GroupChatMessage>,
    /// Pre-formatted two-char uppercase initials for the group avatar tile,
    /// computed by [`group_initials`] from `GroupId::local_id` (V-29).
    /// `"?"` when the `local_id` is empty so the host always has a label.
    #[serde(default)]
    pub group_initials: String,
}

impl GroupChatSnapshot {
    /// An empty snapshot — what a freshly-constructed projection (or a poisoned
    /// internal mutex, D6) reports. `group_initials` is `"?"` here because no
    /// `local_id` is reachable on this path; mirrors the V-29 contract that
    /// the avatar tile always has a label.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            messages: Vec::new(),
            group_initials: "?".to_string(),
        }
    }
}

/// Accumulates a single NIP-29 group's chat-content events into a newest-first
/// message list.
///
/// Construct with the target [`GroupId`]; register the same `Arc` as a
/// [`KernelEventObserver`] (ingest) and capture it in a snapshot-projection
/// closure (output). Only events whose kind is 9 / 11 **and** whose
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
    /// (9 / 11) carrying an `["h", local_id]` tag matching this group.
    ///
    /// Moderation kinds (9000–9009), user-management (9021/9022), and
    /// relay-signed metadata (39000–39003) are deliberately excluded — this is
    /// a chat *read* model, not a moderation log.
    fn accepts(&self, event: &KernelEvent) -> bool {
        let kind_ok = matches!(
            event.kind,
            KIND_CHAT_MESSAGE | KIND_DISCUSSION_OR_ARTIFACT
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
        self.snapshot_at(now_unix_secs())
    }

    /// Snapshot the message set against a caller-supplied wall-clock "now"
    /// (Unix seconds). Exposed so tests can pin the clock and assert on the
    /// `created_at_display` relative-time labels deterministically. Production
    /// callers should use [`Self::snapshot`], which reads `SystemTime::now()`.
    #[must_use]
    pub fn snapshot_at(&self, now_secs: u64) -> GroupChatSnapshot {
        let Ok(messages) = self.messages.lock() else {
            return GroupChatSnapshot::empty();
        };
        // Each message is cloned out of the bounded store; the
        // `created_at_display` field is (re)computed here against `now_secs`
        // so it never goes stale across ticks (V-22 thin-shell fix — host
        // renders the label verbatim).
        let mut ordered: Vec<GroupChatMessage> = messages
            .values()
            .cloned()
            .map(|mut msg| {
                msg.created_at_display = format_ago_secs(now_secs, msg.created_at);
                msg
            })
            .collect();
        // Newest-first. Tie-break on id (descending) so the order is total and
        // stable across ticks even when two events share a `created_at`.
        ordered.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        GroupChatSnapshot {
            messages: ordered,
            // V-29: avatar-tile label for `PublicGroupRow` is derived in Rust
            // so the iOS shell never slices the local-id string itself. Cheap
            // pure-string compute, included on every tick alongside the
            // per-message label refresh.
            group_initials: group_initials(&self.group_id.local_id),
        }
    }

    /// Snapshot as a `serde_json::Value` — the exact shape a host
    /// `register_snapshot_projection` closure must return.
    ///
    /// D6: a serialisation failure (not expected for this plain struct)
    /// collapses to `json!({"messages": [], "group_initials": "?"})` rather
    /// than propagating — `group_initials` (V-29) carries `"?"` here for the
    /// same reason `GroupChatSnapshot::empty()` does (no usable `local_id`
    /// reachable on the failure path).
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or_else(|_| {
            serde_json::json!({ "messages": [], "group_initials": "?" })
        })
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
    fn fresh_projection_yields_empty_messages_with_group_initials() {
        // V-29: a fresh projection has no messages but DOES carry the
        // group-initials label derived from `local_id` ("rust-nostr" → "RU").
        // `GroupChatSnapshot::empty()` is the D6 fallback for a poisoned lock
        // (no `local_id` reachable there) and reports `"?"` instead — the
        // two paths now diverge on `group_initials` by design, so we assert
        // structure rather than full equality.
        let proj = GroupChatProjection::new(group());
        let snap = proj.snapshot();
        assert!(snap.messages.is_empty());
        assert_eq!(snap.group_initials, "RU");
        let json = proj.snapshot_json();
        assert_eq!(
            json,
            serde_json::json!({ "messages": [], "group_initials": "RU" })
        );
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
    fn thread_kind_is_retained() {
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event(
            "thread",
            KIND_DISCUSSION_OR_ARTIFACT,
            10,
            h_tag("rust-nostr"),
        ));

        let snap = proj.snapshot();
        assert_eq!(snap.messages.len(), 1);
        let kinds: Vec<u32> = snap.messages.iter().map(|m| m.kind).collect();
        assert!(kinds.contains(&KIND_DISCUSSION_OR_ARTIFACT));
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
        proj.on_kernel_event(&event(
            "e2",
            KIND_DISCUSSION_OR_ARTIFACT,
            200,
            h_tag("rust-nostr"),
        ));

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

    // ── V-22: created_at_display relative-time labels ────────────────────
    //
    // The exhaustive bucket tests live in `nmp_core::display::tests`. The
    // projection-level tests below verify the snapshot path threads the
    // pinned wall-clock through `nmp_core::display::format_ago_secs` correctly
    // — that is the behaviour the host shell binds to.

    #[test]
    fn snapshot_at_populates_created_at_display() {
        // Pin the clock so the bucket assertions are deterministic.
        let proj = GroupChatProjection::new(group());
        // Three events at known offsets from `now = 10_000`.
        proj.on_kernel_event(&event("seconds", KIND_CHAT_MESSAGE, 10_000 - 5, h_tag("rust-nostr")));
        proj.on_kernel_event(&event("minutes", KIND_CHAT_MESSAGE, 10_000 - 120, h_tag("rust-nostr")));
        proj.on_kernel_event(&event("hours", KIND_CHAT_MESSAGE, 10_000 - 7_200, h_tag("rust-nostr")));

        let snap = proj.snapshot_at(10_000);
        // Newest-first: seconds (now-5), minutes (now-120), hours (now-7200).
        assert_eq!(snap.messages[0].id, "seconds");
        assert_eq!(snap.messages[0].created_at_display, "5s ago");
        assert_eq!(snap.messages[1].id, "minutes");
        assert_eq!(snap.messages[1].created_at_display, "2m ago");
        assert_eq!(snap.messages[2].id, "hours");
        assert_eq!(snap.messages[2].created_at_display, "2h ago");
    }

    #[test]
    fn snapshot_at_refreshes_created_at_display_across_ticks() {
        // The label must be (re)computed every tick — never stale.
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 1_000, h_tag("rust-nostr")));

        // Tick at `now = 1_005` → "5s ago".
        let snap_a = proj.snapshot_at(1_005);
        assert_eq!(snap_a.messages[0].created_at_display, "5s ago");

        // Tick at `now = 1_065` → "1m ago" (same underlying message).
        let snap_b = proj.snapshot_at(1_065);
        assert_eq!(snap_b.messages[0].created_at_display, "1m ago");
    }

    #[test]
    fn ingest_leaves_created_at_display_empty() {
        // The ingest path must NOT pre-compute the label — only the snapshot
        // path may, so the label cannot leak a stale value if a code path
        // ever reads the bounded store directly.
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

        // Drop into the internal store directly to assert the ingest-time
        // shape. (Tests live in-module so this private field is reachable.)
        let messages = proj.messages.lock().expect("lock");
        let stored = messages
            .values()
            .next()
            .expect("one message stored");
        assert_eq!(stored.created_at_display, "", "ingest must not populate the label");
    }

    #[test]
    fn snapshot_json_includes_created_at_display() {
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

        let json = proj.snapshot_json();
        let msg = &json["messages"][0];
        // Even with the production `snapshot()` clock the field is present
        // and non-empty — for any sane host clock it renders as `Xd ago`.
        let display = msg["created_at_display"]
            .as_str()
            .expect("created_at_display present");
        assert!(!display.is_empty(), "snapshot_json must populate the label");
    }

    // ── V-25: author display strings ──────────────────────────────────────
    //
    // Pubkey-derived display strings live in Rust now — the iOS view binds
    // the three fields directly and the `shortPubkey` / `initials` helpers
    // plus the `String(prefix(6))` slice are deleted. These tests pin the
    // exact byte values so an algorithm drift here cannot silently change
    // what every group-chat row renders.

    #[test]
    fn short_hex_long_input_is_first_eight_ellipsis_last_eight() {
        let hex = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        assert_eq!(short_hex(hex), "abcdef01…23456789");
    }

    #[test]
    fn short_hex_short_input_is_unchanged() {
        assert_eq!(short_hex("ab12"), "ab12");
        assert_eq!(short_hex(""), "");
    }

    #[test]
    fn short_hex_boundary_sixteen_chars_is_abbreviated() {
        assert_eq!(short_hex("0123456789abcdef"), "01234567…89abcdef");
    }

    // The pinned cross-surface djb2 vector lives in `nmp_core::display::tests`
    // (`avatar_color_hex_matches_pinned_djb2_vector`); the
    // `ingest_populates_author_display_strings` / `snapshot_preserves_author_display_strings`
    // tests below anchor the call-site value (`"author-of-e1" → "E8844A"`)
    // so a drift in the canonical helper is still caught at this layer.

    #[test]
    fn ingest_populates_author_display_strings() {
        // The ingest path SHOULD pre-compute the author display strings —
        // unlike `created_at_display`, these are pure functions of the
        // pubkey hex and do not change across snapshot ticks, so recomputing
        // them per tick would be wasted work. Reach into the internal store
        // to assert the ingest-time shape.
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

        let messages = proj.messages.lock().expect("lock");
        let stored = messages.values().next().expect("one message stored");
        // `event("e1", …)` synthesises author = "author-of-e1" (len 12 < 16),
        // so `author_display` is the raw author and `author_initials` is the
        // first two chars uppercased.
        assert_eq!(stored.pubkey, "author-of-e1");
        assert_eq!(stored.author_display, "author-of-e1");
        assert_eq!(stored.author_initials, "AU");
        assert_eq!(stored.author_color_hex, "E8844A");
    }

    #[test]
    fn snapshot_preserves_author_display_strings() {
        // The snapshot path must surface the ingest-time author fields
        // unchanged — only `created_at_display` is rewritten per tick.
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

        let snap = proj.snapshot_at(200);
        let msg = &snap.messages[0];
        assert_eq!(msg.author_display, "author-of-e1");
        assert_eq!(msg.author_initials, "AU");
        assert_eq!(msg.author_color_hex, "E8844A");
    }

    #[test]
    fn snapshot_json_includes_author_display_strings() {
        // The Swift `GroupChatMessage` decoder uses `.convertFromSnakeCase`,
        // so the snake_case keys here are exactly what the host binds to
        // `authorDisplay` / `authorInitials` / `authorColorHex`.
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

        let json = proj.snapshot_json();
        let msg = &json["messages"][0];
        assert_eq!(
            msg["author_display"].as_str(),
            Some("author-of-e1"),
            "snapshot_json must emit author_display"
        );
        assert_eq!(
            msg["author_initials"].as_str(),
            Some("AU"),
            "snapshot_json must emit author_initials"
        );
        assert_eq!(
            msg["author_color_hex"].as_str(),
            Some("E8844A"),
            "snapshot_json must emit author_color_hex"
        );
    }

    #[test]
    fn snapshot_with_realistic_pubkey_abbreviates_correctly() {
        // Drive `from_event` with a real 64-char hex author so the
        // production-shape `author_display` branch is exercised end to end.
        let realistic_author =
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        let evt = KernelEvent {
            id: "e-real".into(),
            author: realistic_author.into(),
            kind: KIND_CHAT_MESSAGE,
            created_at: 100,
            tags: h_tag("rust-nostr"),
            content: "hello".into(),
        };
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&evt);

        let snap = proj.snapshot_at(200);
        let msg = &snap.messages[0];
        // short_npub: first 10 chars of npub1… + "…" + last 6 chars (V-33).
        assert_eq!(msg.author_display, "npub140x77…tddknj");
        // avatar_initials(to_npub(hex)) — first 2 chars of the bech32 body.
        // The pinned value "40" comes from the canonical npub encoding of
        // the test fixture; any drift here means the bech32 helper changed.
        assert_eq!(msg.author_initials, "40");
        assert_eq!(msg.author_color_hex, "08E60C");
    }

    // ── V-29: group_initials avatar-tile label ────────────────────────────
    //
    // The iOS `PublicGroupRow` used to derive `initials` from `groupId.localId`
    // in Swift. That derivation lives in Rust now; the view binds the snapshot
    // field directly. These tests pin the algorithm — a drift here changes
    // what every NIP-29 group row renders for its avatar tile.

    #[test]
    fn group_initials_takes_first_two_uppercased() {
        assert_eq!(group_initials("rust-nostr"), "RU");
        assert_eq!(group_initials("ab"), "AB");
        // Mixed case input is normalised to uppercase.
        assert_eq!(group_initials("aBcDeF"), "AB");
    }

    #[test]
    fn group_initials_empty_input_is_question_mark() {
        // Matches the deleted Swift `guard !id.isEmpty else { return "?" }`
        // fallback — the host always has a non-empty label to render.
        assert_eq!(group_initials(""), "?");
    }

    #[test]
    fn group_initials_single_char_input_is_uppercased() {
        // D6: no panic on shorter-than-two inputs; the available prefix is
        // returned uppercased.
        assert_eq!(group_initials("a"), "A");
    }

    #[test]
    fn snapshot_populates_group_initials_from_local_id() {
        // The fixture group has `local_id = "rust-nostr"` → "RU".
        let proj = GroupChatProjection::new(group());
        assert_eq!(proj.snapshot().group_initials, "RU");
    }

    #[test]
    fn snapshot_group_initials_is_refreshed_per_tick_against_local_id() {
        // The group_initials field must reflect the projection's own group
        // identity on every snapshot — not the empty-fallback `"?"`. Even
        // after ingesting messages, the label stays bound to `local_id`.
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

        let snap = proj.snapshot_at(200);
        assert_eq!(snap.group_initials, "RU");
        assert_eq!(snap.messages.len(), 1);
    }

    #[test]
    fn snapshot_json_includes_group_initials() {
        // The Swift `GroupChatSnapshot` decoder uses `.convertFromSnakeCase`,
        // so the snake_case key here is exactly what the host binds to
        // `groupInitials`. This is the load-bearing assertion that V-29 lets
        // the iOS `PublicGroupRow` delete its Swift `initials` derivation.
        let proj = GroupChatProjection::new(group());
        let json = proj.snapshot_json();
        assert_eq!(
            json["group_initials"].as_str(),
            Some("RU"),
            "snapshot_json must emit group_initials for the public-group avatar tile"
        );
    }

    #[test]
    fn empty_snapshot_constructor_uses_question_mark_label() {
        // `GroupChatSnapshot::empty()` runs on the poisoned-lock D6 path where
        // no `local_id` is reachable — it must still have a non-empty
        // `group_initials` so the host avatar tile is never blank.
        let empty = GroupChatSnapshot::empty();
        assert!(empty.messages.is_empty());
        assert_eq!(empty.group_initials, "?");
    }

    #[test]
    fn snapshot_serde_round_trips_group_initials() {
        // The `group_initials` field survives a serde round trip end to end
        // (the iOS shell decodes the same JSON shape).
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
        let snap = proj.snapshot();
        let encoded = serde_json::to_string(&snap).expect("snapshot serialises");
        let decoded: GroupChatSnapshot =
            serde_json::from_str(&encoded).expect("snapshot deserialises");
        assert_eq!(decoded.group_initials, "RU");
        assert_eq!(decoded.messages.len(), 1);
    }
}
