//! Raw `h`-tagged group-event projection for one NIP-29 group.
//!
//! Unlike [`super::group_chat::GroupChatProjection`], this read model is not a
//! chat row. It preserves the accepted event's raw fields and full tag matrix so
//! app/domain crates can perform their own cross-NIP joins without teaching
//! `nmp-nip29` about articles, highlights, podcasts, or replies.

use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent, MAX_PROJECTION_MESSAGES};
use nmp_core::KernelEventObserver;
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;
use crate::kinds::h_tag_value;

/// One raw event carrying the target group's `["h", local_id]` tag.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct GroupEventRow {
    pub id: String,
    pub pubkey: String,
    pub content: String,
    pub created_at: u64,
    pub kind: u32,
    pub tags: Vec<Vec<String>>,
    #[serde(default)]
    pub relay_provenance: Vec<String>,
}

impl GroupEventRow {
    fn from_event(event: &KernelEvent) -> Self {
        Self {
            id: event.id.clone(),
            pubkey: event.author.clone(),
            content: event.content.clone(),
            created_at: event.created_at,
            kind: event.kind,
            tags: event.tags.clone(),
            relay_provenance: event.relay_provenance.clone(),
        }
    }
}

/// Snapshot for `"nmp.nip29.group_events"`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct GroupEventsSnapshot {
    pub group_id: String,
    pub host_relay_url: String,
    pub events: Vec<GroupEventRow>,
}

impl GroupEventsSnapshot {
    #[must_use]
    pub fn empty(group: &GroupId) -> Self {
        Self {
            group_id: group.local_id.clone(),
            host_relay_url: group.host_relay_url.clone(),
            events: Vec::new(),
        }
    }
}

/// Accumulates raw `h`-tagged events for one group, newest-first on snapshot.
pub struct GroupEventsProjection {
    group_id: GroupId,
    events: Mutex<BoundedMessageMap<String, GroupEventRow>>,
}

impl GroupEventsProjection {
    #[must_use]
    pub fn new(group_id: GroupId) -> Self {
        Self {
            group_id,
            events: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
        }
    }

    pub fn group_id(&self) -> &GroupId {
        &self.group_id
    }

    fn accepts(&self, event: &KernelEvent) -> bool {
        h_tag_value(&event.tags) == Some(self.group_id.local_id.as_str())
    }

    #[must_use]
    pub fn snapshot(&self) -> GroupEventsSnapshot {
        let Ok(events) = self.events.lock() else {
            return GroupEventsSnapshot::empty(&self.group_id);
        };
        let mut ordered: Vec<GroupEventRow> = events.values().cloned().collect();
        ordered.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        GroupEventsSnapshot {
            group_id: self.group_id.local_id.clone(),
            host_relay_url: self.group_id.host_relay_url.clone(),
            events: ordered,
        }
    }

    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or_else(|_| {
            serde_json::json!({
                "group_id": self.group_id.local_id,
                "host_relay_url": self.group_id.host_relay_url,
                "events": [],
            })
        })
    }
}

impl KernelEventObserver for GroupEventsProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if !self.accepts(event) {
            return;
        }
        let Ok(mut events) = self.events.lock() else {
            return;
        };
        events.insert(event.id.clone(), GroupEventRow::from_event(event));
    }
}

#[cfg(test)]
#[path = "group_events/tests.rs"]
mod tests;
