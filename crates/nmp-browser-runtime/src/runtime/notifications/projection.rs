//! Cross-protocol notifications for events that p-tag the active account.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent};
use nmp_core::ObservedProjectionSink;
use nmp_kinds::{
    KIND_GENERIC_REPOST, KIND_NIP22_COMMENT, KIND_REACTION, KIND_REPOST, KIND_SHORT_TEXT_NOTE,
    KIND_ZAP_RECEIPT,
};
use nmp_planner::InterestShape;
use serde::{Deserialize, Serialize};

pub const NOTIFICATIONS_KEY: &str = "nmp.notifications";
pub const NOTIFICATIONS_SCHEMA_ID: &str = "nmp.notifications";
pub const NOTIFICATIONS_SCHEMA_VERSION: u32 = 1;
pub const NOTIFICATIONS_FILE_IDENTIFIER: &[u8; 4] = b"NNTF";
pub const NOTIFICATIONS_LIMIT: u32 = 200;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NotificationRow {
    pub event_id: String,
    pub actor_pubkey: String,
    pub event_kind: u32,
    pub notification_kind: NotificationKind,
    pub created_at: u64,
    pub content: String,
    pub target_event_id: Option<String>,
    pub source_relays: Vec<String>,
    pub read: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    Mention,
    Reply,
    Reaction,
    Repost,
    Zap,
    Comment,
}

impl NotificationKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mention => "mention",
            Self::Reply => "reply",
            Self::Reaction => "reaction",
            Self::Repost => "repost",
            Self::Zap => "zap",
            Self::Comment => "comment",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct NotificationsSnapshot {
    pub viewer_pubkey: String,
    pub rows: Vec<NotificationRow>,
    pub unread_count: u32,
}

pub struct NotificationsProjection {
    viewer_pubkey: String,
    rows: Mutex<BoundedMessageMap<String, NotificationRow>>,
    read_event_ids: Mutex<BTreeSet<String>>,
}

impl NotificationsProjection {
    #[must_use]
    pub fn new(viewer_pubkey: String) -> Self {
        Self {
            viewer_pubkey,
            rows: Mutex::new(BoundedMessageMap::new(NOTIFICATIONS_LIMIT as usize)),
            read_event_ids: Mutex::new(BTreeSet::new()),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> NotificationsSnapshot {
        let Ok(rows) = self.rows.lock() else {
            return NotificationsSnapshot {
                viewer_pubkey: self.viewer_pubkey.clone(),
                rows: Vec::new(),
                unread_count: 0,
            };
        };
        let mut ordered: Vec<_> = rows.values().cloned().collect();
        let read_event_ids = self.read_event_ids.lock().ok();
        for row in &mut ordered {
            row.read = read_event_ids
                .as_ref()
                .is_some_and(|ids| ids.contains(&row.event_id));
        }
        ordered.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.event_id.cmp(&a.event_id))
        });
        let unread_count = ordered.iter().filter(|row| !row.read).count();
        NotificationsSnapshot {
            viewer_pubkey: self.viewer_pubkey.clone(),
            rows: ordered,
            unread_count: unread_count.min(u32::MAX as usize) as u32,
        }
    }

    pub fn mark_read<I, S>(&self, event_ids: I) -> usize
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let Ok(rows) = self.rows.lock() else {
            return 0;
        };
        let Ok(mut read_event_ids) = self.read_event_ids.lock() else {
            return 0;
        };
        let mut changed = 0;
        for id in event_ids {
            let id = id.as_ref().trim();
            if id.is_empty() || !rows.contains_key(id) {
                continue;
            }
            if read_event_ids.insert(id.to_string()) {
                changed += 1;
            }
        }
        changed
    }

    pub fn mark_all_read(&self) -> usize {
        let Ok(rows) = self.rows.lock() else {
            return 0;
        };
        let Ok(mut read_event_ids) = self.read_event_ids.lock() else {
            return 0;
        };
        let mut changed = 0;
        for (id, _) in rows.iter() {
            if read_event_ids.insert(id.clone()) {
                changed += 1;
            }
        }
        changed
    }

    fn classify(&self, event: &KernelEvent) -> Option<NotificationRow> {
        if event.author == self.viewer_pubkey || !has_p_tag(&event.tags, &self.viewer_pubkey) {
            return None;
        }

        let notification_kind = match event.kind {
            KIND_SHORT_TEXT_NOTE => {
                if first_event_tag(&event.tags).is_some() {
                    NotificationKind::Reply
                } else {
                    NotificationKind::Mention
                }
            }
            KIND_REACTION => NotificationKind::Reaction,
            KIND_ZAP_RECEIPT => NotificationKind::Zap,
            KIND_NIP22_COMMENT => NotificationKind::Comment,
            kind if is_repost_kind(kind) => NotificationKind::Repost,
            _ => return None,
        };

        let target_event_id = first_event_tag(&event.tags);

        Some(NotificationRow {
            event_id: event.id.clone(),
            actor_pubkey: event.author.clone(),
            event_kind: event.kind,
            notification_kind,
            created_at: event.created_at,
            content: event.content.clone(),
            target_event_id,
            source_relays: event.relay_provenance.clone(),
            read: false,
        })
    }
}

impl ObservedProjectionSink for NotificationsProjection {
    fn on_kernel_event(&self, event: &KernelEvent) {
        let Some(row) = self.classify(event) else {
            return;
        };
        let Ok(mut rows) = self.rows.lock() else {
            return;
        };
        rows.insert(row.event_id.clone(), row);
    }
}

#[must_use]
pub fn notifications_interest_shape(viewer_pubkey: &str) -> InterestShape {
    InterestShape {
        kinds: [
            KIND_SHORT_TEXT_NOTE,
            KIND_REPOST,
            KIND_GENERIC_REPOST,
            KIND_REACTION,
            KIND_ZAP_RECEIPT,
            KIND_NIP22_COMMENT,
        ]
        .into_iter()
        .collect(),
        tags: BTreeMap::from([("p".to_string(), BTreeSet::from([viewer_pubkey.to_string()]))]),
        limit: Some(NOTIFICATIONS_LIMIT),
        ..Default::default()
    }
}

const fn is_repost_kind(kind: u32) -> bool {
    kind == KIND_REPOST || kind == KIND_GENERIC_REPOST
}

fn has_p_tag(tags: &[Vec<String>], pubkey: &str) -> bool {
    tags.iter().any(|tag| {
        tag.first().is_some_and(|name| name == "p")
            && tag.get(1).is_some_and(|value| value == pubkey)
    })
}

fn first_event_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == "e") {
            tag.get(1).filter(|id| !id.is_empty()).cloned()
        } else {
            None
        }
    })
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;
