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

use nmp_core::display::{avatar_color_hex, avatar_initials, format_ago_secs, short_npub, to_npub};
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
#[path = "group_chat/tests.rs"]
mod tests;
