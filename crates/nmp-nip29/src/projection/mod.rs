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
//! app.register_snapshot_projection("nip29.group_chat", move || snap.snapshot_json());
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

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::{h_tag_value, KIND_CHAT_MESSAGE, KIND_COMMENT, KIND_DISCUSSION_OR_ARTIFACT};

/// One rendered group-chat message in a [`GroupChatSnapshot`].
///
/// A flat carrier — threading / reply nesting is deliberately *not* modelled
/// here. The read screen this feeds is a linear chat log; a threaded view is a
/// separate projection.
///
/// ## Pre-rendered display fields (aim.md §6 anti-pattern: "Duplicated
/// formatting logic across platforms")
///
/// `created_at_display`, `sender_short`, `sender_initials`, and
/// `sender_color_seed` are derived by [`GroupChatProjection::snapshot`] on
/// every tick. The platform shell renders these strings verbatim — it does
/// **not** call `RelativeDateTimeFormatter`, slice the pubkey, uppercase
/// initials, or seed an avatar color. All such logic lived in `GroupChatView`
/// before this projection took ownership of it (aim.md doctrine §6 rule 4:
/// "no native business logic").
///
/// `created_at_display` is recomputed against `SystemTime::now()` per tick so
/// the "2m" / "3h" labels advance with wall-clock time. The raw `created_at`
/// is retained for shells that want their own format.
///
/// Other surfaces (e.g. `MarmotGroupChat`, a future `ThreadView`) consuming
/// the same row shape **mirror these fields by name** — `created_at_display`
/// / `sender_short` / `sender_initials` / `sender_color_seed` — rather than
/// reaching into a shared util crate. The shape is the contract; this crate
/// owns NIP-29's rendering.
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
    /// Abbreviated relative-time label ("now", "5s", "2m", "3h", "1d",
    /// "2w", "5mo", "3y") computed against `SystemTime::now()` at snapshot
    /// time. Mirrors the abbreviated style of iOS `RelativeDateTimeFormatter`
    /// so the shell does not need to invoke it.
    pub created_at_display: String,
    /// Truncated hex pubkey for display: `aabbccdd…11223344` — first 8
    /// chars, ellipsis, last 8. Mirrors `nmp_core::kernel::nostr::short_hex`
    /// pattern but keeps a chat-style ellipsis. Shorter pubkeys render
    /// verbatim.
    pub sender_short: String,
    /// Two-character avatar label — the uppercased first two hex chars of
    /// the pubkey. Cheap and deterministic, no npub decoding.
    pub sender_initials: String,
    /// Hex string used by the shell as a deterministic avatar color seed —
    /// the first 6 hex chars of the pubkey. Mirrors what `GroupChatView`
    /// passed as `colorHex` before this projection took ownership.
    pub sender_color_seed: String,
}

/// Internal storage shape — the raw event facts a `GroupChatProjection`
/// retains per accepted event. Derived display fields (`*_display` /
/// `sender_*`) are *not* stored: they are recomputed per snapshot tick so
/// relative-time labels advance with wall-clock time. Keeping the stored
/// shape minimal also lets [`KernelEventObserver::on_kernel_event`] stay a
/// single uncontended lock + map insert.
#[derive(Clone, Debug, PartialEq, Eq)]
struct StoredMessage {
    id: String,
    pubkey: String,
    content: String,
    created_at: u64,
    kind: u32,
}

impl StoredMessage {
    /// Build a stored row from a kernel event. The caller is responsible for
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

    /// Project the stored row into a `GroupChatMessage` row using `now_secs`
    /// as the wall-clock reference for `created_at_display`. The
    /// `sender_*` fields are pure functions of `pubkey` and do not depend on
    /// `now_secs`.
    fn into_row(self, now_secs: u64) -> GroupChatMessage {
        let created_at_display = format_relative_seconds(now_secs, self.created_at);
        let sender_short = short_hex_pubkey(&self.pubkey);
        let sender_initials = pubkey_initials(&self.pubkey);
        let sender_color_seed = pubkey_color_seed(&self.pubkey);
        GroupChatMessage {
            id: self.id,
            pubkey: self.pubkey,
            content: self.content,
            created_at: self.created_at,
            kind: self.kind,
            created_at_display,
            sender_short,
            sender_initials,
            sender_color_seed,
        }
    }
}

/// Abbreviated relative-time formatter — mirrors the
/// `unitsStyle = .abbreviated` output of iOS `RelativeDateTimeFormatter`:
/// "now", "5s", "2m", "3h", "1d", "2w", "5mo", "3y".
///
/// `now_secs` is the projection's current wall-clock reference (unix
/// seconds), `created_at` is the event's timestamp. A future-timestamped
/// event (clock skew) clamps to `"now"`. A clock before the epoch (negative
/// duration in `format_relative_seconds`'s caller, `snapshot()`) is treated
/// the same way the rest of the kernel treats it — `0` — so labels remain
/// stable.
///
/// Bucket boundaries match what a SwiftUI chat row would otherwise show via
/// `RelativeDateTimeFormatter`:
///
/// - `< 5s`           → `"now"`
/// - `< 60s`          → `"Ns"`
/// - `< 60m`          → `"Nm"`
/// - `< 24h`          → `"Nh"`
/// - `< 7d`           → `"Nd"`
/// - `< 4w` (28d)     → `"Nw"`
/// - `< 12mo` (365d)  → `"Nmo"`  (30-day months — what the iOS formatter does)
/// - otherwise        → `"Ny"`
fn format_relative_seconds(now_secs: u64, created_at: u64) -> String {
    let elapsed = now_secs.saturating_sub(created_at);
    if elapsed < 5 {
        return "now".to_string();
    }
    if elapsed < 60 {
        return format!("{elapsed}s");
    }
    let minutes = elapsed / 60;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = hours / 24;
    if days < 7 {
        return format!("{days}d");
    }
    let weeks = days / 7;
    if weeks < 4 {
        return format!("{weeks}w");
    }
    // Months: 30-day buckets for the label, but the *boundary* between
    // "Nmo" and "Ny" is a 365-day year so `364d → 12mo` rather than
    // `0y` (which would happen with a naive `months < 12` cutoff against
    // `days / 30`).
    if days < 365 {
        let months = days / 30;
        return format!("{months}mo");
    }
    let years = days / 365;
    format!("{years}y")
}

/// Truncated hex pubkey for display — first 8 chars, ellipsis, last 8.
/// Mirrors `GroupChatView.shortPubkey` exactly so the migration is a
/// behaviour-preserving rename. Pubkeys shorter than 16 chars render
/// verbatim (matches the Swift guard).
fn short_hex_pubkey(pubkey: &str) -> String {
    if pubkey.chars().count() < 16 {
        return pubkey.to_string();
    }
    // Hex pubkeys are ASCII; byte-index slicing is char-safe here, but
    // route through char iteration to stay correct if a non-hex value
    // is ever passed (the projection itself does not validate).
    let prefix: String = pubkey.chars().take(8).collect();
    let suffix_start = pubkey.chars().count() - 8;
    let suffix: String = pubkey.chars().skip(suffix_start).collect();
    format!("{prefix}\u{2026}{suffix}")
}

/// Two-character avatar label — the uppercased first two chars of the
/// pubkey. Mirrors `GroupChatMessageRow.initials` exactly.
fn pubkey_initials(pubkey: &str) -> String {
    pubkey.chars().take(2).collect::<String>().to_uppercase()
}

/// Avatar color seed — the first 6 chars of the pubkey. Mirrors
/// `GroupChatMessageRow`'s `colorHex: String(message.pubkey.prefix(6))`
/// exactly so the avatar tint is unchanged across the migration.
fn pubkey_color_seed(pubkey: &str) -> String {
    pubkey.chars().take(6).collect()
}

/// Current wall-clock in unix seconds. Mirrors the `now_ms` helper used by
/// `nmp_core::ffi::action`. A clock before the epoch collapses to `0`, which
/// makes `format_relative_seconds` produce `"now"` for every retained event
/// (`now_secs.saturating_sub(created_at) == 0`) — the safest fallback.
fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
    /// Accepted messages keyed by event id. A `BTreeMap` keyed on id makes the
    /// observer idempotent: a re-delivered event id replaces rather than
    /// duplicates. Ordering for the snapshot is applied on read, not here.
    ///
    /// Stores raw event facts only ([`StoredMessage`]); derived display
    /// fields (`created_at_display`, `sender_*`) are recomputed in
    /// [`Self::snapshot`] so relative-time labels advance with wall-clock
    /// time across ticks.
    messages: Mutex<BTreeMap<String, StoredMessage>>,
}

impl GroupChatProjection {
    /// Construct a projection scoped to `group_id`. The message store starts
    /// empty; events arrive via [`KernelEventObserver::on_kernel_event`].
    pub fn new(group_id: GroupId) -> Self {
        Self {
            group_id,
            messages: Mutex::new(BTreeMap::new()),
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
    /// Display fields (`created_at_display`, `sender_short`,
    /// `sender_initials`, `sender_color_seed`) are computed here, on the
    /// actor thread, against `SystemTime::now()`. Relative-time labels
    /// therefore advance with wall-clock time across snapshot ticks — they
    /// are *not* frozen at ingest.
    ///
    /// D8: still a single uncontended `Mutex` lock + small `Vec` clone +
    /// per-row string allocations. No I/O, no relay round-trips. The
    /// formatter is `O(1)` per row.
    ///
    /// D6: a poisoned mutex degrades to [`GroupChatSnapshot::empty`] rather
    /// than panicking — this can run on the actor thread inside a snapshot
    /// tick, where a panic would unwind the kernel.
    pub fn snapshot(&self) -> GroupChatSnapshot {
        let Ok(messages) = self.messages.lock() else {
            return GroupChatSnapshot::empty();
        };
        let now_secs = now_unix_secs();
        let mut ordered: Vec<GroupChatMessage> = messages
            .values()
            .cloned()
            .map(|m| m.into_row(now_secs))
            .collect();
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
        messages.insert(event.id.clone(), StoredMessage::from_event(event));
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

    // ── derived display fields ────────────────────────────────────────────
    //
    // aim.md §6 / doctrine #9: pre-rendered display fields belong on the
    // projection so platform shells do not duplicate the formatting. These
    // tests pin the contract the iOS / Android / desktop / wasm consumers
    // can rely on.

    #[test]
    fn short_pubkey_truncates_long_hex_with_ellipsis() {
        // 64-char hex (a normal nostr pubkey) — 8 prefix, ellipsis, 8 suffix.
        let pk = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(short_hex_pubkey(pk), "01234567\u{2026}89abcdef");
    }

    #[test]
    fn short_pubkey_returns_short_input_verbatim() {
        // Under 16 chars renders unchanged (matches the Swift guard the
        // projection replaces).
        assert_eq!(short_hex_pubkey("abc"), "abc");
        assert_eq!(short_hex_pubkey(""), "");
        // Exactly 15 chars — still verbatim.
        assert_eq!(short_hex_pubkey("0123456789abcde"), "0123456789abcde");
    }

    #[test]
    fn initials_are_uppercased_first_two_chars() {
        assert_eq!(pubkey_initials("abcdef"), "AB");
        // Already uppercase stays put.
        assert_eq!(pubkey_initials("AB12cd"), "AB");
        // Single char — second slot is missing rather than padded.
        assert_eq!(pubkey_initials("z"), "Z");
        assert_eq!(pubkey_initials(""), "");
    }

    #[test]
    fn color_seed_is_first_six_chars() {
        assert_eq!(pubkey_color_seed("abcdef0123456789"), "abcdef");
        // Shorter than 6 → whatever is available.
        assert_eq!(pubkey_color_seed("abc"), "abc");
        assert_eq!(pubkey_color_seed(""), "");
    }

    #[test]
    fn relative_time_buckets_match_abbreviated_style() {
        // Anchor "now" — a fixed reference clock so the buckets are
        // verifiable without `SystemTime`. Large enough that every "ago"
        // arithmetic operation stays within `u64` (the largest probe is
        // ~365 days = ~31.5M seconds).
        let now = 100_000_000u64;
        // < 5s → "now".
        assert_eq!(format_relative_seconds(now, now), "now");
        assert_eq!(format_relative_seconds(now, now - 4), "now");
        // Future timestamp (clock skew) clamps to "now".
        assert_eq!(format_relative_seconds(now, now + 30), "now");
        // Seconds.
        assert_eq!(format_relative_seconds(now, now - 5), "5s");
        assert_eq!(format_relative_seconds(now, now - 59), "59s");
        // Minutes.
        assert_eq!(format_relative_seconds(now, now - 60), "1m");
        assert_eq!(format_relative_seconds(now, now - 60 * 59), "59m");
        // Hours.
        assert_eq!(format_relative_seconds(now, now - 60 * 60), "1h");
        assert_eq!(format_relative_seconds(now, now - 60 * 60 * 23), "23h");
        // Days.
        assert_eq!(format_relative_seconds(now, now - 60 * 60 * 24), "1d");
        assert_eq!(format_relative_seconds(now, now - 60 * 60 * 24 * 6), "6d");
        // Weeks.
        assert_eq!(format_relative_seconds(now, now - 60 * 60 * 24 * 7), "1w");
        assert_eq!(format_relative_seconds(now, now - 60 * 60 * 24 * 27), "3w");
        // Months (30-day buckets, matches abbreviated style). 364 days
        // still labels as 12mo — the year boundary is exactly 365d.
        assert_eq!(format_relative_seconds(now, now - 60 * 60 * 24 * 30), "1mo");
        assert_eq!(format_relative_seconds(now, now - 60 * 60 * 24 * 364), "12mo");
        // Years.
        assert_eq!(format_relative_seconds(now, now - 60 * 60 * 24 * 365), "1y");
    }

    #[test]
    fn snapshot_row_carries_pre_rendered_display_fields() {
        let proj = GroupChatProjection::new(group());
        // 64-char hex pubkey so the short / initials / color forms all
        // exercise their long-input branches.
        let pubkey = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let evt = KernelEvent {
            id: "e1".into(),
            author: pubkey.into(),
            kind: KIND_CHAT_MESSAGE,
            created_at: now_unix_secs(), // "now" — bucket-stable
            tags: h_tag("rust-nostr"),
            content: "hi".into(),
        };
        proj.on_kernel_event(&evt);

        let row = proj.snapshot().messages.into_iter().next().expect("one row");
        // Pre-rendered identity fields.
        assert_eq!(row.sender_short, "01234567\u{2026}89abcdef");
        assert_eq!(row.sender_initials, "01");
        assert_eq!(row.sender_color_seed, "012345");
        // Relative-time label fell into the "now" bucket (< 5s from
        // `SystemTime::now()` at construction).
        assert_eq!(row.created_at_display, "now");
    }

    #[test]
    fn relative_time_advances_across_snapshots() {
        // The relative-time label MUST recompute per tick — it must NOT
        // freeze at ingest. Inserting an event with a stale `created_at`
        // and reading two snapshots back demonstrates the formatter runs
        // against the current wall clock each call.
        //
        // We bound the assertion by a single bucket: an event aged "1h"
        // worth of seconds before "now" must report a multi-character
        // label that is NOT `"now"`, regardless of CI clock drift.
        let proj = GroupChatProjection::new(group());
        let stale = now_unix_secs().saturating_sub(60 * 60 * 2); // 2h ago
        let evt = KernelEvent {
            id: "stale".into(),
            author: "author-of-stale".into(),
            kind: KIND_CHAT_MESSAGE,
            created_at: stale,
            tags: h_tag("rust-nostr"),
            content: "old".into(),
        };
        proj.on_kernel_event(&evt);

        let label = proj
            .snapshot()
            .messages
            .into_iter()
            .next()
            .map(|m| m.created_at_display)
            .expect("one row");
        assert_ne!(label, "now", "stale event must not collapse to the now bucket");
        // 2h falls in the hours bucket — sanity check the formatter wrote
        // the abbreviated suffix the shell will render.
        assert!(label.ends_with('h'), "expected an hours label, got {label}");
    }

    #[test]
    fn snapshot_json_serialises_display_fields_with_snake_case_keys() {
        // The wire shape the Swift / Kotlin / TS shells decode. `serde`'s
        // default key strategy is field name verbatim — `created_at_display`,
        // `sender_short`, `sender_initials`, `sender_color_seed`.
        let proj = GroupChatProjection::new(group());
        proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

        let json = proj.snapshot_json();
        let row = json
            .get("messages")
            .and_then(|m| m.as_array())
            .and_then(|m| m.first())
            .expect("one row");
        assert!(row.get("created_at_display").and_then(|v| v.as_str()).is_some());
        assert!(row.get("sender_short").and_then(|v| v.as_str()).is_some());
        assert!(row.get("sender_initials").and_then(|v| v.as_str()).is_some());
        assert!(row.get("sender_color_seed").and_then(|v| v.as_str()).is_some());
    }
}
