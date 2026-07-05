//! `DiscoveredGroupsProjection` — the read-side of the NIP-29 group-discovery
//! screen.
//!
//! Like [`super::group_events::GroupEventsProjection`], this is **pure
//! consumption**: a [`ObservedProjectionSink`] that accumulates the relay-signed
//! metadata events for a SET of host relays and serialises them as a flat list
//! of `DiscoveredGroup` rows, one row per `(host_relay_url, local_id)` pair. It
//! registers no actions, mints no FFI symbols, and never touches the actor
//! loop.
//!
//! ## Multi-relay aggregation (#93)
//!
//! NIP-29 group identity is the **pair** `(host_relay_url, local_id)`
//! (`group_id.rs`). Two relays publishing kind:39000 with `d=room` are TWO
//! different groups. A live discovery session tracks a SET of relays
//! (`add_relay` / `remove_relay`, reconciled by
//! `read_session::open_nip29_group_discovery_session` as the caller's desired
//! relay set changes) and dedups/merges by that pair — never by `local_id`
//! alone.
//!
//! Attribution is per-EVENT, not per-projection: `KernelEvent::relay_provenance`
//! (populated by the kernel from the local store for every delivered event,
//! regardless of which demand's `relay_pin` admitted it) names the relay(s)
//! that actually delivered this event. An event is folded into a row for every
//! relay in `relay_provenance` that is ALSO currently tracked by this
//! projection; an event whose provenance names no tracked relay is dropped
//! (fail-closed, D6 — never guessed). This is what lets one shared reducer
//! safely aggregate N relay-pinned demands: the routing pin gets the event to
//! the reducer, but the reducer itself re-derives which relay(s) it came from
//! rather than trusting a single construction-time value.
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

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::ObservedProjectionSink;
use serde::{Deserialize, Serialize};

use crate::group_id::RelayUrl;
use crate::kinds::tags::{child_tag_values, parent_tag_value};
use crate::kinds::{d_tag_value, KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS, KIND_GROUP_METADATA};

/// One discovered group, ready for a host shell to render.
///
/// A flat carrier — the projection rolls all three metadata kinds into one
/// row per `local_id`. `member_count` / `admin_count` are 0 until the
/// corresponding 39002 / 39001 has arrived; the booleans default to
/// "public" / "open" (Highlighter convention) when no metadata has arrived.
///
/// Raw protocol data only (ADR-0072): presentation-layer fields such as
/// display-name fallback, avatar initials, and formatted subtitle are
/// the shell's responsibility and must not be pre-computed here.
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
    /// NIP-29 subgroups (nips PR #2319): the `["parent", <id>]` tag value on
    /// the latest 39000, pointing at this group's parent's `d` identifier.
    /// `None` (absent or empty) means this is a root group. The tree is
    /// scoped to the single host relay this projection is built for.
    pub parent: Option<String>,
    /// NIP-29 subgroups: the ordered `["child", <id>]` tag values on the
    /// latest 39000 — the parent's children list, in tag order. Empty until a
    /// 39000 carrying `child` tags arrives. The relay maintains this list as
    /// children are adopted/detached; clients render the hierarchy from it.
    pub children: Vec<String>,
}

/// The serialised read-model a discovery screen consumes.
///
/// `groups` is ordered by `(host_relay_url, group_id)` so the list is total,
/// stable, and human-friendly across snapshot ticks even when several
/// relays share a `local_id` (a different group per relay, #93). The
/// currently-tracked relay set is surfaced at the top so a shell can render
/// a screen header (e.g. "browsing 3 relays") without holding onto the
/// original input separately; it is NOT the set of relays that produced
/// `groups` — a relay with zero groups so far is still tracked.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct DiscoveredGroupsSnapshot {
    /// The relays this discovery session is currently tracking, sorted.
    pub host_relay_urls: Vec<String>,
    pub groups: Vec<DiscoveredGroup>,
}

impl DiscoveredGroupsSnapshot {
    /// Empty snapshot — what a freshly-constructed projection (or one whose
    /// internal lock is poisoned, D6) reports.
    #[must_use]
    pub fn empty(host_relay_urls: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            host_relay_urls: host_relay_urls.into_iter().map(Into::into).collect(),
            groups: Vec::new(),
        }
    }
}

/// Per-(relay, kind, d) latest-event entry. The projection only keeps the
/// most recent event per `(relay, kind, d)`; this struct is the comparator
/// key.
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
        if incoming.created_at == self.created_at {
            incoming.id > self.id
        } else {
            incoming.created_at > self.created_at
        }
    }
}

/// Accumulates a SET of host relays' kind:39000/39001/39002 events into a
/// flat list of discovered groups, one row per `(host_relay_url, local_id)`
/// pair (#93 — multi-relay group discovery).
///
/// Construct with the initially-tracked relay set; grow/shrink it over the
/// projection's lifetime with [`Self::add_relay`] / [`Self::remove_relay`] as
/// the live discovery session's desired relay set changes. Register the same
/// `Arc` as a [`ObservedProjectionSink`] (ingest) and capture it in a
/// snapshot-projection closure (output). Only events whose kind is 39000 /
/// 39001 / 39002 **and** which carry a `["d", _]` tag, AND whose
/// `relay_provenance` names a currently-tracked relay, are retained.
pub struct DiscoveredGroupsProjection {
    /// The relays this projection currently tracks. An event is folded into
    /// a row for every relay in its `relay_provenance` that is also a member
    /// of this set (see module docs — attribution is per-event, not
    /// per-projection).
    relays: Mutex<BTreeSet<RelayUrl>>,
    /// Latest event per `(relay, kind, d)`. NIP-33 replaceable semantics: a
    /// newer event for the same key strictly supersedes the older one.
    ///
    /// Bounded by [`MAX_PROJECTION_MESSAGES`] — once full, the oldest entry
    /// by insertion order is evicted. The cap defends the outer map against
    /// adversarial relays that spam fake group ids: without it, every
    /// distinct `(relay, kind, d)` triple would persist for the lifetime of
    /// the session, growing the snapshot and the resident set unboundedly.
    /// Re-delivering an existing key updates in place and does not shift
    /// eviction order — replaceable-event semantics are preserved by the
    /// [`LatestEvent::supersedes`] check before the call to `insert`.
    latest: Mutex<BoundedMessageMap<(RelayUrl, u32, String), LatestEvent>>,
}

impl DiscoveredGroupsProjection {
    /// Construct a projection tracking `host_relay_urls`. The internal map
    /// starts empty; events arrive via [`ObservedProjectionSink::on_kernel_event`].
    #[must_use]
    pub fn new(host_relay_urls: impl IntoIterator<Item = impl Into<RelayUrl>>) -> Self {
        Self {
            relays: Mutex::new(host_relay_urls.into_iter().map(Into::into).collect()),
            latest: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    /// The relays this projection currently tracks, sorted.
    #[must_use]
    pub fn host_relay_urls(&self) -> Vec<String> {
        self.relays
            .lock()
            .map(|relays| relays.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Start tracking `relay`: future events whose `relay_provenance` names it
    /// are folded in. A no-op if already tracked.
    pub fn add_relay(&self, relay: impl Into<RelayUrl>) {
        if let Ok(mut relays) = self.relays.lock() {
            relays.insert(relay.into());
        }
    }

    /// Stop tracking `relay` and purge every row already attributed to it —
    /// a live discovery session's relay set shrinking must not leave stale
    /// groups from a relay the caller is no longer browsing.
    pub fn remove_relay(&self, relay: &str) {
        if let Ok(mut relays) = self.relays.lock() {
            relays.remove(relay);
        }
        let Ok(mut latest) = self.latest.lock() else {
            return;
        };
        let stale: Vec<(RelayUrl, u32, String)> = latest
            .iter()
            .filter(|((r, _, _), _)| r == relay)
            .map(|(k, _)| k.clone())
            .collect();
        for key in stale {
            latest.remove(&key);
        }
    }

    /// Whether `event` belongs in this projection: one of the three metadata
    /// kinds AND a `["d", _]` tag is present. Relay attribution is checked
    /// separately (see [`Self::on_kernel_event`]) since it depends on
    /// per-event provenance, not just the event's own fields.
    fn accepts(event: &KernelEvent) -> bool {
        let kind_ok = matches!(
            event.kind,
            KIND_GROUP_METADATA | KIND_GROUP_ADMINS | KIND_GROUP_MEMBERS
        );
        kind_ok && d_tag_value(&event.tags).is_some()
    }

    /// Snapshot the current discovered-group set, ordered by
    /// `(host_relay_url, group_id)`.
    ///
    /// D6: a poisoned mutex degrades to [`DiscoveredGroupsSnapshot::empty`]
    /// rather than panicking — this can run on the actor thread inside a
    /// snapshot tick, where a panic would unwind the kernel.
    #[must_use]
    pub fn snapshot(&self) -> DiscoveredGroupsSnapshot {
        let host_relay_urls = self.host_relay_urls();
        let Ok(latest) = self.latest.lock() else {
            return DiscoveredGroupsSnapshot::empty(host_relay_urls);
        };

        // Bucket the per-(relay, kind, d) latest events by (relay, d) so
        // each (host_relay_url, group_id) pair appears once with all three
        // kinds rolled in. A `BTreeMap` keyed on that pair gives a total,
        // relay-then-id order for free.
        let mut by_key: BTreeMap<(RelayUrl, String), DiscoveredGroup> = BTreeMap::new();
        for ((relay, kind, d), entry) in latest.iter() {
            let row = by_key
                .entry((relay.clone(), d.clone()))
                .or_insert_with(|| DiscoveredGroup {
                    group_id: d.clone(),
                    host_relay_url: relay.clone(),
                    public: true,
                    open: true,
                    ..Default::default()
                });
            apply_event_to_row(row, *kind, &entry.tags);
        }

        DiscoveredGroupsSnapshot {
            host_relay_urls,
            groups: by_key.into_values().collect(),
        }
    }

    /// Snapshot as a `serde_json::Value` — the exact shape a host
    /// `register_snapshot_projection` closure must return.
    ///
    /// D6: a serialisation failure (not expected for this plain struct)
    /// collapses to an empty payload rather than propagating.
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or_else(|_| {
            serde_json::json!({
                "host_relay_urls": self.host_relay_urls(),
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
            // NIP-29 subgroups (#2319): `parent` is a single tag (at-most-one
            // per the spec); an empty value normalises to None (root).
            row.parent = parent_tag_value(tags).map(str::to_string);
            // Children preserve tag order (the spec models the list as
            // ordered; relay appends new children, parent admin reorders).
            row.children = child_tag_values(tags)
                .map(|v| v.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default();
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
    let n = tags.iter().filter(|t| t.len() >= 2 && t[0] == "p").count();
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl ObservedProjectionSink for DiscoveredGroupsProjection {
    /// Ingest one accepted kernel event. Non-matching events (wrong kind,
    /// missing `d` tag, or provenance naming no currently-tracked relay) are
    /// ignored. A matching event is folded into the per-`(relay, kind, d)`
    /// latest-event slot per NIP-33 replaceable semantics — once for every
    /// tracked relay its `relay_provenance` names (almost always exactly
    /// one; see module docs for why attribution is per-event).
    ///
    /// Cheap and panic-free, per the `ObservedProjectionSink` contract: a
    /// couple of uncontended lock + map operations. A poisoned mutex is a
    /// silent no-op (D6).
    fn on_kernel_event(&self, event: &KernelEvent) {
        if !Self::accepts(event) {
            return;
        }
        // `accepts` confirmed `d_tag_value` is `Some`; unwrap is safe.
        let d = match d_tag_value(&event.tags) {
            Some(d) => d.to_string(),
            None => return,
        };
        if event.relay_provenance.is_empty() {
            // No provenance to attribute this event to any tracked relay —
            // fail closed rather than guess (D6).
            return;
        }
        let matched: Vec<RelayUrl> = {
            let Ok(relays) = self.relays.lock() else {
                return;
            };
            event
                .relay_provenance
                .iter()
                .filter(|r| relays.contains(r.as_str()))
                .cloned()
                .collect()
        };
        if matched.is_empty() {
            return;
        }
        let Ok(mut latest) = self.latest.lock() else {
            return;
        };
        let incoming = LatestEvent {
            created_at: event.created_at,
            id: event.id.clone(),
            tags: event.tags.clone(),
        };
        for relay in matched {
            let key = (relay, event.kind, d.clone());
            match latest.get(&key) {
                Some(existing) if !existing.supersedes(&incoming) => {
                    // Existing is newer or equal-and-higher-id — keep it.
                }
                _ => {
                    latest.insert(key, incoming.clone());
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "discovered/tests.rs"]
mod tests;
