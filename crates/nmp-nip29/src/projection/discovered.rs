//! `DiscoveredGroupsProjection` — the read-side of the NIP-29 group-discovery
//! screen.
//!
//! Like [`super::group_chat::GroupChatProjection`], this is **pure
//! consumption**: a [`KernelEventObserver`] that accumulates the relay-signed
//! metadata events for a single host relay and serialises them as a flat list
//! of `DiscoveredGroup` rows. It registers no actions, mints no FFI symbols,
//! and never touches the actor loop.
//!
//! ## Per-relay scope
//!
//! NIP-29 group identity is the **pair** `(host_relay_url, local_id)`
//! (`group_id.rs`). Two relays publishing kind:39000 with `d=room` are TWO
//! different groups. This projection is therefore scoped to one host relay
//! at construction time; an event is retained iff:
//!
//! - its kind is one of 39000 / 39001 / 39002, AND
//! - it carries a `["d", local_id]` tag (the parameterized-replaceable key).
//!
//! Restricting to the host relay's own events is an *upstream* routing
//! concern: the companion `interest::relay_discovery_interest` pins the
//! subscription to the relay, so a correctly-pinned interest only ever
//! delivers events from that host. This projection trusts the pin and does
//! NOT re-check provenance from event tags (a `KernelEvent` has no
//! relay-of-origin field).
//!
//! ## How metadata is extracted (per docs/design/nip29/kinds.md §2.4)
//!
//! Kind:39000 — `["name", text]`, `["picture", url]`, `["about", text]`,
//! `["public"]`/`["private"]`. Absence of `["private"]` defaults to public
//! (Highlighter convention, adopted here).
//!
//! Kind:39002 — one `["p", pubkey]` tag per member. `member_count` is the
//! cardinality of those tags on the latest 39002 for this group.
//!
//! Kind:39001 (admins) is retained to derive `admin_count` and to detect
//! when the latest metadata snapshot is admin-touched, but its pubkeys are
//! not surfaced in the wire row — discovery shows rooms, not their admin
//! lists (a future "group detail" screen can layer that on top).
//!
//! ## Replaceable-event semantics
//!
//! All three kinds are NIP-33 parameterized-replaceable on `d`. The projection
//! keeps only the most recent event per `(kind, d)` — comparing `created_at`,
//! ties broken by `id` descending so the choice is total and deterministic.

use std::collections::BTreeMap;
use std::sync::Mutex;

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use serde::{Deserialize, Serialize};

use crate::group_id::RelayUrl;
use crate::kinds::{d_tag_value, KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS, KIND_GROUP_METADATA};

/// One discovered group, ready for a host shell to render.
///
/// A flat carrier — the projection rolls all three metadata kinds into one
/// row per `local_id`. `member_count` / `admin_count` are 0 until the
/// corresponding 39002 / 39001 has arrived; the booleans default to
/// "public" / "open" (Highlighter convention) when no metadata has arrived.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct DiscoveredGroup {
    /// The NIP-29 group's in-relay id (the `["d", _]` tag value). Stable
    /// list identity inside this projection.
    pub group_id: String,
    /// The host relay this group lives on. Mirrors the projection's
    /// construction-time scope; surfaced so a Swift call site can build a
    /// typed `GroupId { host_relay_url, local_id }` without re-supplying
    /// the URL from elsewhere.
    pub host_relay_url: String,
    /// `["name", _]` tag value, if the latest 39000 carried one.
    pub name: Option<String>,
    /// `["picture", _]` tag value, if any.
    pub picture: Option<String>,
    /// `["about", _]` tag value, if any.
    pub about: Option<String>,
    /// Cardinality of `["p", _]` tags on the latest 39002. `0` until 39002
    /// arrives.
    pub member_count: u32,
    /// Cardinality of `["p", _]` tags on the latest 39001. `0` until 39001
    /// arrives.
    pub admin_count: u32,
    /// `true` iff the latest 39000 lacks a `["private"]` tag. Defaults to
    /// `true` (public) when no 39000 has arrived — Highlighter convention.
    pub public: bool,
    /// `true` iff the latest 39000 lacks a `["closed"]` tag. Defaults to
    /// `true` (open) when no 39000 has arrived.
    pub open: bool,
}

/// The serialised read-model a discovery screen consumes.
///
/// `groups` is ordered alphabetically by `group_id` so the list is total,
/// stable, and human-friendly across snapshot ticks. The relay URL is
/// surfaced at the top so Swift can render a screen header without holding
/// onto the original input separately.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct DiscoveredGroupsSnapshot {
    /// The host relay this snapshot describes — every row's `host_relay_url`
    /// equals this value (the projection is single-relay scoped).
    pub host_relay_url: String,
    pub groups: Vec<DiscoveredGroup>,
}

impl DiscoveredGroupsSnapshot {
    /// Empty snapshot — what a freshly-constructed projection (or one whose
    /// internal lock is poisoned, D6) reports.
    pub fn empty(host_relay_url: impl Into<String>) -> Self {
        Self {
            host_relay_url: host_relay_url.into(),
            groups: Vec::new(),
        }
    }
}

/// Per-(kind, d) latest-event entry. The projection only keeps the most
/// recent event per `(kind, d)`; this struct is the comparator key.
#[derive(Clone, Debug)]
struct LatestEvent {
    created_at: u64,
    id: String,
    tags: Vec<Vec<String>>,
}

impl LatestEvent {
    /// `true` iff `incoming` should supersede `self` per NIP-33 replaceable
    /// semantics — strictly newer `created_at`, ties broken by id descending
    /// (so the choice is total and deterministic).
    fn supersedes(&self, incoming: &Self) -> bool {
        if incoming.created_at != self.created_at {
            incoming.created_at > self.created_at
        } else {
            incoming.id > self.id
        }
    }
}

/// Accumulates one host relay's kind:39000/39001/39002 events into a flat
/// list of discovered groups.
///
/// Construct with the [`RelayUrl`] the relay-pinned interest is targeting;
/// register the same `Arc` as a [`KernelEventObserver`] (ingest) and capture
/// it in a snapshot-projection closure (output). Only events whose kind is
/// 39000 / 39001 / 39002 **and** which carry a `["d", _]` tag are retained.
pub struct DiscoveredGroupsProjection {
    /// The host relay this projection is scoped to. Mirrors the
    /// `relay_pin` value the companion `relay_discovery_interest` pushes.
    host_relay_url: RelayUrl,
    /// Latest event per `(kind, d)`. NIP-33 replaceable semantics: a newer
    /// event for the same `(kind, d)` strictly supersedes the older one.
    /// `BTreeMap` keys are `(kind, d_tag)`; values are the comparator
    /// snapshot of the winning event.
    latest: Mutex<BTreeMap<(u32, String), LatestEvent>>,
}

impl DiscoveredGroupsProjection {
    /// Construct a projection scoped to `host_relay_url`. The internal map
    /// starts empty; events arrive via [`KernelEventObserver::on_kernel_event`].
    pub fn new(host_relay_url: impl Into<RelayUrl>) -> Self {
        Self {
            host_relay_url: host_relay_url.into(),
            latest: Mutex::new(BTreeMap::new()),
        }
    }

    /// The host relay this projection is scoped to.
    pub fn host_relay_url(&self) -> &str {
        &self.host_relay_url
    }

    /// Whether `event` belongs in this projection: one of the three metadata
    /// kinds AND a `["d", _]` tag is present.
    fn accepts(&self, event: &KernelEvent) -> bool {
        let kind_ok = matches!(
            event.kind,
            KIND_GROUP_METADATA | KIND_GROUP_ADMINS | KIND_GROUP_MEMBERS
        );
        kind_ok && d_tag_value(&event.tags).is_some()
    }

    /// Snapshot the current discovered-group set, alphabetised by group id.
    ///
    /// D6: a poisoned mutex degrades to [`DiscoveredGroupsSnapshot::empty`]
    /// rather than panicking — this can run on the actor thread inside a
    /// snapshot tick, where a panic would unwind the kernel.
    pub fn snapshot(&self) -> DiscoveredGroupsSnapshot {
        let Ok(latest) = self.latest.lock() else {
            return DiscoveredGroupsSnapshot::empty(self.host_relay_url.clone());
        };

        // Bucket the per-(kind, d) latest events by `d` so each group_id
        // appears once with all three kinds rolled in. A `BTreeMap` keyed
        // on `d` gives alphabetical ordering for free.
        let mut by_d: BTreeMap<String, DiscoveredGroup> = BTreeMap::new();
        for ((kind, d), entry) in latest.iter() {
            let row = by_d
                .entry(d.clone())
                .or_insert_with(|| DiscoveredGroup {
                    group_id: d.clone(),
                    host_relay_url: self.host_relay_url.clone(),
                    public: true,
                    open: true,
                    ..Default::default()
                });
            apply_event_to_row(row, *kind, &entry.tags);
        }

        DiscoveredGroupsSnapshot {
            host_relay_url: self.host_relay_url.clone(),
            groups: by_d.into_values().collect(),
        }
    }

    /// Snapshot as a `serde_json::Value` — the exact shape a host
    /// `register_snapshot_projection` closure must return.
    ///
    /// D6: a serialisation failure (not expected for this plain struct)
    /// collapses to an empty payload rather than propagating.
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or_else(|_| {
            serde_json::json!({
                "host_relay_url": self.host_relay_url,
                "groups": [],
            })
        })
    }
}

/// Fold one accepted metadata event into the row being built for its `d`.
///
/// Split out for unit-testability — the three kinds extract different
/// fields, and keeping the per-kind logic in one place makes it cheap to
/// add a new metadata field (e.g. NIP-29 `restricted`, `hidden`) without
/// touching the projection state machine.
fn apply_event_to_row(row: &mut DiscoveredGroup, kind: u32, tags: &[Vec<String>]) {
    match kind {
        KIND_GROUP_METADATA => {
            row.name = single_tag_value(tags, "name");
            row.picture = single_tag_value(tags, "picture");
            row.about = single_tag_value(tags, "about");
            // Highlighter convention: absence of `private` defaults to public.
            row.public = !has_marker_tag(tags, "private");
            row.open = !has_marker_tag(tags, "closed");
        }
        KIND_GROUP_ADMINS => {
            row.admin_count = count_p_tags(tags);
        }
        KIND_GROUP_MEMBERS => {
            row.member_count = count_p_tags(tags);
        }
        _ => {}
    }
}

/// First `["<key>", <value>]` tag value, if any.
fn single_tag_value(tags: &[Vec<String>], key: &str) -> Option<String> {
    tags.iter()
        .find(|t| t.len() >= 2 && t[0] == key)
        .map(|t| t[1].clone())
}

/// Whether `["<key>"]` (a marker tag — no value) is present.
fn has_marker_tag(tags: &[Vec<String>], key: &str) -> bool {
    tags.iter().any(|t| !t.is_empty() && t[0] == key)
}

/// Count of `["p", _]` tags in `tags`.
fn count_p_tags(tags: &[Vec<String>]) -> u32 {
    tags.iter()
        .filter(|t| t.len() >= 2 && t[0] == "p")
        .count() as u32
}

impl KernelEventObserver for DiscoveredGroupsProjection {
    /// Ingest one accepted kernel event. Non-matching events (wrong kind,
    /// missing `d` tag) are ignored. Matching events are folded into the
    /// per-`(kind, d)` latest-event slot per NIP-33 replaceable semantics.
    ///
    /// Cheap and panic-free, per the `KernelEventObserver` contract: a single
    /// uncontended lock + map insert. A poisoned mutex is a silent no-op (D6).
    fn on_kernel_event(&self, event: &KernelEvent) {
        if !self.accepts(event) {
            return;
        }
        // `accepts` confirmed `d_tag_value` is `Some`; unwrap is safe.
        let d = match d_tag_value(&event.tags) {
            Some(d) => d.to_string(),
            None => return,
        };
        let Ok(mut latest) = self.latest.lock() else {
            return;
        };
        let key = (event.kind, d);
        let incoming = LatestEvent {
            created_at: event.created_at,
            id: event.id.clone(),
            tags: event.tags.clone(),
        };
        match latest.get(&key) {
            Some(existing) if !existing.supersedes(&incoming) => {
                // Existing is newer or equal-and-higher-id — keep it.
            }
            _ => {
                latest.insert(key, incoming);
            }
        }
    }
}

#[cfg(test)]
#[path = "discovered/tests.rs"]
mod tests;
