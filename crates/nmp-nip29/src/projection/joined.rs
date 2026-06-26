//! Joined-groups projection for the active account.
//!
//! Canonical joined/admin state comes only from relay-signed kind:39001 and
//! kind:39002 snapshots. User-signed moderation actions such as kind:9000 are
//! audit/request events and deliberately do not mutate this read model.

use std::collections::BTreeMap;
use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::ObservedProjectionSink;
use serde::{Deserialize, Serialize};

use crate::group_id::RelayUrl;
use crate::kinds::tags::{child_tag_values, parent_tag_value};
use crate::kinds::{d_tag_value, KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS, KIND_GROUP_METADATA};

/// One group the active pubkey belongs to on a host relay.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct JoinedGroup {
    pub group_id: String,
    pub host_relay_url: String,
    pub name: Option<String>,
    pub picture: Option<String>,
    pub about: Option<String>,
    pub member_count: u32,
    pub admin_count: u32,
    pub public: bool,
    pub open: bool,
    pub is_member: bool,
    pub is_admin: bool,
    /// NIP-29 subgroups (#2319): the `["parent", <id>]` tag value on the
    /// latest 39000. `None` (absent/empty) = root group. NOTE:
    /// `joined_groups_for_host` subscribes to 39001/39002 only, so this
    /// populates only when a 39000 also arrives via relay provenance. A
    /// host wanting the full tree on joined groups layers a discovery
    /// interest on the same relay (out of scope for this field).
    pub parent: Option<String>,
    /// NIP-29 subgroups: ordered `["child", <id>]` tag values on the latest
    /// 39000. Empty until a 39000 carrying `child` tags arrives (same
    /// provenance caveat as `parent`).
    pub children: Vec<String>,
}

/// Snapshot for the `"nmp.nip29.joined_groups"` typed sidecar.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct JoinedGroupsSnapshot {
    pub active_pubkey: String,
    pub groups: Vec<JoinedGroup>,
}

impl JoinedGroupsSnapshot {
    #[must_use]
    pub fn empty(active_pubkey: impl Into<String>) -> Self {
        Self {
            active_pubkey: active_pubkey.into(),
            groups: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct LatestEvent {
    created_at: u64,
    id: String,
    tags: Vec<Vec<String>>,
}

impl LatestEvent {
    fn supersedes(&self, incoming: &Self) -> bool {
        if incoming.created_at == self.created_at {
            incoming.id > self.id
        } else {
            incoming.created_at > self.created_at
        }
    }
}

/// Accumulates relay-signed 39000/39001/39002 events into the active account's
/// joined-groups list. If constructed with a host relay, that host is used as
/// the identity component for all accepted events; otherwise the first
/// `KernelEvent.relay_provenance` URL is required.
pub struct JoinedGroupsProjection {
    active_pubkey: String,
    host_relay_url: Option<RelayUrl>,
    latest: Mutex<BoundedMessageMap<(RelayUrl, u32, String), LatestEvent>>,
}

impl JoinedGroupsProjection {
    #[must_use]
    pub fn new(active_pubkey: impl Into<String>) -> Self {
        Self {
            active_pubkey: active_pubkey.into(),
            host_relay_url: None,
            latest: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    #[must_use]
    pub fn new_for_host(
        active_pubkey: impl Into<String>,
        host_relay_url: impl Into<RelayUrl>,
    ) -> Self {
        Self {
            active_pubkey: active_pubkey.into(),
            host_relay_url: Some(host_relay_url.into()),
            latest: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    fn accepts(event: &KernelEvent) -> bool {
        matches!(
            event.kind,
            KIND_GROUP_METADATA | KIND_GROUP_ADMINS | KIND_GROUP_MEMBERS
        ) && d_tag_value(&event.tags).is_some()
    }

    fn event_host(&self, event: &KernelEvent) -> Option<RelayUrl> {
        self.host_relay_url
            .clone()
            .or_else(|| event.relay_provenance.first().cloned())
    }

    #[must_use]
    pub fn snapshot(&self) -> JoinedGroupsSnapshot {
        let Ok(latest) = self.latest.lock() else {
            return JoinedGroupsSnapshot::empty(self.active_pubkey.clone());
        };

        let mut by_group: BTreeMap<(RelayUrl, String), JoinedGroup> = BTreeMap::new();
        for ((host, kind, d), entry) in latest.iter() {
            let row = by_group
                .entry((host.clone(), d.clone()))
                .or_insert_with(|| JoinedGroup {
                    group_id: d.clone(),
                    host_relay_url: host.clone(),
                    public: true,
                    open: true,
                    ..Default::default()
                });
            apply_event_to_row(row, *kind, &entry.tags, &self.active_pubkey);
        }

        let groups = by_group
            .into_values()
            .filter(|g| g.is_member || g.is_admin)
            .collect();
        JoinedGroupsSnapshot {
            active_pubkey: self.active_pubkey.clone(),
            groups,
        }
    }

    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or_else(|_| {
            serde_json::json!({
                "active_pubkey": self.active_pubkey,
                "groups": [],
            })
        })
    }
}

fn apply_event_to_row(row: &mut JoinedGroup, kind: u32, tags: &[Vec<String>], active_pubkey: &str) {
    match kind {
        KIND_GROUP_METADATA => {
            row.name = single_tag_value(tags, "name");
            row.picture = single_tag_value(tags, "picture");
            row.about = single_tag_value(tags, "about");
            row.public = !has_marker_tag(tags, "private");
            row.open = !has_marker_tag(tags, "closed");
            row.parent = parent_tag_value(tags).map(str::to_string);
            row.children = child_tag_values(tags)
                .map(|v| v.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default();
        }
        KIND_GROUP_ADMINS => {
            row.admin_count = count_p_tags(tags);
            row.is_admin = has_p_tag(tags, active_pubkey);
        }
        KIND_GROUP_MEMBERS => {
            row.member_count = count_p_tags(tags);
            row.is_member = has_p_tag(tags, active_pubkey);
        }
        _ => {}
    }
}

fn single_tag_value(tags: &[Vec<String>], key: &str) -> Option<String> {
    tags.iter()
        .find(|t| t.len() >= 2 && t[0] == key)
        .map(|t| t[1].clone())
}

fn has_marker_tag(tags: &[Vec<String>], key: &str) -> bool {
    tags.iter().any(|t| !t.is_empty() && t[0] == key)
}

fn count_p_tags(tags: &[Vec<String>]) -> u32 {
    u32::try_from(tags.iter().filter(|t| t.len() >= 2 && t[0] == "p").count()).unwrap_or(u32::MAX)
}

fn has_p_tag(tags: &[Vec<String>], pubkey: &str) -> bool {
    !pubkey.is_empty()
        && tags
            .iter()
            .any(|t| t.len() >= 2 && t[0] == "p" && t[1] == pubkey)
}

impl ObservedProjectionSink for JoinedGroupsProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if !Self::accepts(event) {
            return;
        }
        let Some(host) = self.event_host(event) else {
            return;
        };
        let Some(d) = d_tag_value(&event.tags).map(str::to_string) else {
            return;
        };
        let Ok(mut latest) = self.latest.lock() else {
            return;
        };
        let key = (host, event.kind, d);
        let incoming = LatestEvent {
            created_at: event.created_at,
            id: event.id.clone(),
            tags: event.tags.clone(),
        };
        match latest.get(&key) {
            Some(existing) if !existing.supersedes(&incoming) => {}
            _ => {
                latest.insert(key, incoming);
            }
        }
    }
}

#[cfg(test)]
#[path = "joined/tests.rs"]
mod tests;
