//! Cross-protocol notifications for events that p-tag the active account.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, KernelEvent};
use nmp_core::ObservedProjectionSink;
use nmp_kinds::{KIND_NIP22_COMMENT, KIND_REACTION, KIND_SHORT_TEXT_NOTE, KIND_ZAP_RECEIPT};
use nmp_nip18::{is_repost_kind, try_from_kernel_event};
use nmp_planner::InterestShape;
use serde::{Deserialize, Serialize};

pub const NOTIFICATIONS_KEY: &str = "nmp.relations.notifications";
pub const NOTIFICATIONS_SCHEMA_ID: &str = "nmp.relations.notifications";
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
}

pub struct NotificationsProjection {
    viewer_pubkey: String,
    rows: Mutex<BoundedMessageMap<String, NotificationRow>>,
}

impl NotificationsProjection {
    #[must_use]
    pub fn new(viewer_pubkey: String) -> Self {
        Self {
            viewer_pubkey,
            rows: Mutex::new(BoundedMessageMap::new(NOTIFICATIONS_LIMIT as usize)),
        }
    }

    #[must_use]
    pub fn viewer_pubkey(&self) -> &str {
        &self.viewer_pubkey
    }

    #[must_use]
    pub fn snapshot(&self) -> NotificationsSnapshot {
        let Ok(rows) = self.rows.lock() else {
            return NotificationsSnapshot {
                viewer_pubkey: self.viewer_pubkey.clone(),
                rows: Vec::new(),
            };
        };
        let mut ordered: Vec<_> = rows.values().cloned().collect();
        ordered.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.event_id.cmp(&a.event_id))
        });
        NotificationsSnapshot {
            viewer_pubkey: self.viewer_pubkey.clone(),
            rows: ordered,
        }
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

        let target_event_id = if is_repost_kind(event.kind) {
            try_from_kernel_event(event).and_then(|repost| repost.target_event_id)
        } else if event.kind == KIND_ZAP_RECEIPT {
            nmp_nip57::try_from_kernel_event(event).and_then(|zap| zap.zapped_event_id)
        } else {
            first_event_tag(&event.tags)
        };

        Some(NotificationRow {
            event_id: event.id.clone(),
            actor_pubkey: event.author.clone(),
            event_kind: event.kind,
            notification_kind,
            created_at: event.created_at,
            content: event.content.clone(),
            target_event_id,
            source_relays: event.relay_provenance.clone(),
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
            nmp_nip18::KIND_REPOST,
            nmp_nip18::KIND_GENERIC_REPOST,
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
mod tests {
    use super::*;

    const VIEWER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const ACTOR: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const TARGET: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn event(id: &str, kind: u32, tags: Vec<Vec<&str>>, content: &str) -> KernelEvent {
        KernelEvent {
            id: id.to_string(),
            author: ACTOR.to_string(),
            kind,
            created_at: 42,
            tags: tags
                .into_iter()
                .map(|tag| tag.into_iter().map(str::to_string).collect())
                .collect(),
            content: content.to_string(),
            relay_provenance: vec!["wss://relay.example".to_string()],
        }
    }

    #[test]
    fn captures_reply_to_viewer_with_source_relay() {
        let projection = NotificationsProjection::new(VIEWER.to_string());
        projection.on_kernel_event(&event(
            "reply",
            KIND_SHORT_TEXT_NOTE,
            vec![vec!["e", TARGET], vec!["p", VIEWER]],
            "reply body",
        ));

        let snapshot = projection.snapshot();
        assert_eq!(snapshot.rows.len(), 1);
        assert_eq!(snapshot.rows[0].notification_kind, NotificationKind::Reply);
        assert_eq!(snapshot.rows[0].target_event_id.as_deref(), Some(TARGET));
        assert_eq!(snapshot.rows[0].source_relays, vec!["wss://relay.example"]);
    }

    #[test]
    fn ignores_self_authored_and_unaddressed_events() {
        let projection = NotificationsProjection::new(VIEWER.to_string());
        let mut self_event = event("self", KIND_REACTION, vec![vec!["p", VIEWER]], "+");
        self_event.author = VIEWER.to_string();
        projection.on_kernel_event(&self_event);
        projection.on_kernel_event(&event("other", KIND_REACTION, vec![], "+"));
        assert!(projection.snapshot().rows.is_empty());
    }

    #[test]
    fn interest_shape_is_bounded_p_tag_inbox() {
        let shape = notifications_interest_shape(VIEWER);
        assert_eq!(shape.limit, Some(NOTIFICATIONS_LIMIT));
        assert!(shape.kinds.contains(&KIND_REACTION));
        assert!(shape
            .tags
            .get("p")
            .is_some_and(|values| values.contains(VIEWER)));
    }
}
