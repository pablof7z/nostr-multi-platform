//! `GroupRosterProjection` — the read-side of a NIP-29 group's **member
//! roster**.
//!
//! The existing [`super::joined::JoinedGroupsProjection`] and
//! [`super::discovered::DiscoveredGroupsProjection`] fold the relay-signed
//! 39001 (admins) / 39002 (members) snapshots into `member_count` /
//! `admin_count` scalars and an active-account `is_member` / `is_admin` bool —
//! they deliberately DISCARD the member pubkeys, so no consumer can render a
//! roster. This projection is the missing read model: it RETAINS every member /
//! admin pubkey plus the per-pubkey role tokens and the group's role catalog
//! (39003), exposing a typed `(pubkey, roles, is_admin, is_member)` row per
//! group member.
//!
//! ## Per-group scope
//!
//! NIP-29 group identity is the **pair** `(host_relay_url, local_id)`
//! (`group_id.rs`); this projection is scoped to ONE group at construction time.
//! An event is retained iff:
//!
//! - its kind is one of 39001 / 39002 / 39003, AND
//! - it carries a `["d", local_id]` tag matching the scoped group id.
//!
//! ## How the roster is extracted (per docs/design/nip29/kinds.md §2.4)
//!
//! - **39002 (members)** — one `["p", <pubkey>]` per member (the spec's
//!   canonical 2-element form). Each pubkey is marked `is_member`. Any extra
//!   elements (`["p", <pubkey>, <role>...]`, a relay convention) are preserved
//!   as role tokens.
//! - **39001 (admins)** — one `["p", <pubkey>]`, `["p", <pubkey>, <role>]`, or
//!   `["p", <pubkey>, <role>, <description>]` per admin. Each pubkey is marked
//!   `is_admin`; the 3rd+ elements are preserved verbatim as role tokens.
//! - **39003 (roles)** — `["role", <name>, <description>]` declaring the role
//!   catalog the relay knows about for this group. Optional (many relays omit
//!   it); folded into the snapshot's `roles` list when present.
//!
//! ## Display separation (ADR-0032)
//!
//! Raw protocol data only: hex pubkeys and verbatim role tokens. No
//! display-name fallback, avatar initials, or role-label formatting runs here —
//! that is the shell's responsibility.
//!
//! ## Replaceable-event semantics
//!
//! All three kinds are NIP-33 parameterized-replaceable on `d`. The projection
//! keeps only the most recent event per `kind` for the scoped group — comparing
//! `created_at`, ties broken by `id` descending so the choice is total and
//! deterministic.

use std::collections::BTreeMap;
use std::sync::Mutex;

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use serde::{Deserialize, Serialize};

use crate::group_id::RelayUrl;
use crate::kinds::{d_tag_value, KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS, KIND_GROUP_ROLES};

/// One member of a NIP-29 group's roster.
///
/// Raw protocol data only (ADR-0032): `pubkey` is the 64-char hex author key,
/// `roles` are verbatim role tokens (the 3rd+ elements of the member's `p`
/// tag). `is_admin` / `is_member` reflect which relay-signed snapshot(s) carry
/// the pubkey (a pubkey can be both — admins are usually members too).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct GroupRosterMember {
    /// Raw hex pubkey (the `["p", <pubkey>, ...]` tag value).
    pub pubkey: String,
    /// Verbatim role tokens for this pubkey — the extra elements on its `p`
    /// tag in the 39001 / 39002 snapshot (`["p", pubkey, role, ...]`). Empty
    /// for a plain member with no role.
    pub roles: Vec<String>,
    /// `true` iff this pubkey appears on the latest 39001 (admins) snapshot.
    pub is_admin: bool,
    /// `true` iff this pubkey appears on the latest 39002 (members) snapshot.
    pub is_member: bool,
}

/// One entry from the group's 39003 role catalog.
///
/// Raw protocol data only: `name` is the role token, `description` the optional
/// 3rd element. The catalog declares the role names the relay knows about; the
/// per-member `roles` reference these by token.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct GroupRole {
    /// Role name token (`["role", <name>, ...]`).
    pub name: String,
    /// Optional human description (`["role", <name>, <description>]`).
    pub description: Option<String>,
}

/// The serialised roster read model a group-detail screen consumes.
///
/// `members` is ordered by `pubkey` so the list is total, stable, and
/// deterministic across snapshot ticks. `roles` preserves the 39003 tag order.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct GroupRosterSnapshot {
    /// The host relay this roster lives on (the projection's scope).
    pub host_relay_url: String,
    /// The group's in-relay id (`["d", _]` tag value).
    pub group_id: String,
    /// One row per distinct pubkey across the latest 39001 + 39002 snapshots.
    pub members: Vec<GroupRosterMember>,
    /// The group's role catalog from the latest 39003 (empty when the relay
    /// publishes no 39003 — many do not).
    pub roles: Vec<GroupRole>,
}

impl GroupRosterSnapshot {
    /// Empty snapshot — what a freshly-constructed projection (or one whose
    /// internal lock is poisoned, D6) reports.
    #[must_use]
    pub fn empty(host_relay_url: impl Into<String>, group_id: impl Into<String>) -> Self {
        Self {
            host_relay_url: host_relay_url.into(),
            group_id: group_id.into(),
            members: Vec::new(),
            roles: Vec::new(),
        }
    }
}

/// Per-kind latest-event entry. NIP-33 replaceable semantics: a newer event for
/// the same kind strictly supersedes the older one.
#[derive(Clone, Debug)]
struct LatestEvent {
    created_at: u64,
    id: String,
    tags: Vec<Vec<String>>,
}

impl LatestEvent {
    /// `true` iff `incoming` should supersede `self` per NIP-33 replaceable
    /// semantics — strictly newer `created_at`, ties broken by id descending.
    fn supersedes(&self, incoming: &Self) -> bool {
        if incoming.created_at == self.created_at {
            incoming.id > self.id
        } else {
            incoming.created_at > self.created_at
        }
    }
}

/// Accumulates one group's kind:39001/39002/39003 events into a typed roster.
///
/// Construct with the [`RelayUrl`] host and the group's `local_id`; register
/// the same `Arc` as a [`ObservedProjectionSink`] (ingest) and capture it in a
/// snapshot-projection closure (output). Only events whose kind is
/// 39001 / 39002 / 39003 **and** which carry a matching `["d", local_id]` tag
/// are retained.
pub struct GroupRosterProjection {
    host_relay_url: RelayUrl,
    group_id: String,
    /// Latest event per kind (at most 3 entries: 39001 / 39002 / 39003).
    latest: Mutex<BTreeMap<u32, LatestEvent>>,
}

impl GroupRosterProjection {
    /// Construct a projection scoped to `(host_relay_url, group_id)`.
    #[must_use]
    pub fn new(host_relay_url: impl Into<RelayUrl>, group_id: impl Into<String>) -> Self {
        Self {
            host_relay_url: host_relay_url.into(),
            group_id: group_id.into(),
            latest: Mutex::new(BTreeMap::new()),
        }
    }

    /// The host relay this projection is scoped to.
    #[must_use]
    pub fn host_relay_url(&self) -> &str {
        &self.host_relay_url
    }

    /// The group id (`d` tag value) this projection is scoped to.
    #[must_use]
    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    /// Whether `event` belongs in this roster: one of the three roster
    /// metadata kinds AND a `["d", local_id]` tag matching the scoped group.
    fn accepts(&self, event: &KernelEvent) -> bool {
        let kind_ok = matches!(
            event.kind,
            KIND_GROUP_ADMINS | KIND_GROUP_MEMBERS | KIND_GROUP_ROLES
        );
        kind_ok && d_tag_value(&event.tags) == Some(self.group_id.as_str())
    }

    /// Snapshot the current roster.
    ///
    /// D6: a poisoned mutex degrades to [`GroupRosterSnapshot::empty`] rather
    /// than panicking — this can run on the actor thread inside a snapshot
    /// tick, where a panic would unwind the kernel.
    #[must_use]
    pub fn snapshot(&self) -> GroupRosterSnapshot {
        let Ok(latest) = self.latest.lock() else {
            return GroupRosterSnapshot::empty(self.host_relay_url.clone(), self.group_id.clone());
        };

        let mut by_pubkey: BTreeMap<String, GroupRosterMember> = BTreeMap::new();
        // Members first, then admins — both fold into the same per-pubkey row.
        if let Some(entry) = latest.get(&KIND_GROUP_MEMBERS) {
            fold_p_tags(&mut by_pubkey, &entry.tags, RosterRole::Member);
        }
        if let Some(entry) = latest.get(&KIND_GROUP_ADMINS) {
            fold_p_tags(&mut by_pubkey, &entry.tags, RosterRole::Admin);
        }

        let roles = latest
            .get(&KIND_GROUP_ROLES)
            .map(|entry| role_catalog(&entry.tags))
            .unwrap_or_default();

        GroupRosterSnapshot {
            host_relay_url: self.host_relay_url.clone(),
            group_id: self.group_id.clone(),
            members: by_pubkey.into_values().collect(),
            roles,
        }
    }

    /// Snapshot as a `serde_json::Value` — the exact shape a host
    /// `register_snapshot_projection` closure must return.
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or_else(|_| {
            serde_json::json!({
                "host_relay_url": self.host_relay_url,
                "group_id": self.group_id,
                "members": [],
                "roles": [],
            })
        })
    }
}

/// Which relay-signed snapshot a `p` tag came from.
#[derive(Clone, Copy)]
enum RosterRole {
    Admin,
    Member,
}

/// Fold every `["p", pubkey, role...]` tag into the per-pubkey roster row,
/// marking the source membership flag and merging role tokens.
fn fold_p_tags(
    by_pubkey: &mut BTreeMap<String, GroupRosterMember>,
    tags: &[Vec<String>],
    source: RosterRole,
) {
    for tag in tags.iter().filter(|t| t.len() >= 2 && t[0] == "p") {
        let pubkey = tag[1].clone();
        if pubkey.is_empty() {
            continue;
        }
        let row = by_pubkey
            .entry(pubkey.clone())
            .or_insert_with(|| GroupRosterMember {
                pubkey,
                ..Default::default()
            });
        match source {
            RosterRole::Admin => row.is_admin = true,
            RosterRole::Member => row.is_member = true,
        }
        // Preserve the verbatim role tokens (3rd+ elements), de-duplicated and
        // order-preserving so a pubkey present on both 39001 and 39002 does not
        // accumulate duplicates.
        for role in &tag[2..] {
            if !role.is_empty() && !row.roles.contains(role) {
                row.roles.push(role.clone());
            }
        }
    }
}

/// Extract the 39003 role catalog: `["role", <name>, <description>]` rows.
fn role_catalog(tags: &[Vec<String>]) -> Vec<GroupRole> {
    tags.iter()
        .filter(|t| t.len() >= 2 && t[0] == "role" && !t[1].is_empty())
        .map(|t| GroupRole {
            name: t[1].clone(),
            description: t.get(2).filter(|d| !d.is_empty()).cloned(),
        })
        .collect()
}

impl ObservedProjectionSink for GroupRosterProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if !self.accepts(event) {
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
        match latest.get(&event.kind) {
            Some(existing) if !existing.supersedes(&incoming) => {
                // Existing is newer or equal-and-higher-id — keep it.
            }
            _ => {
                latest.insert(event.kind, incoming);
            }
        }
    }
}

#[cfg(test)]
#[path = "roster/tests.rs"]
mod tests;
