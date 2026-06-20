use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::KernelEventObserver;
use nmp_feed::{EventLookup, FeedRequest, FollowPredicate};

use super::attribution::Nip10ReplyAttribution;
use super::wiring::{register_op_feed, OpFeedEngine};

pub(super) const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
pub(super) const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";
pub(super) const CAROL: &str = "cccc000000000000000000000000000000000000000000000000000000000003";

// 64-hex event ids so the nevent encoder (32-byte TLV) accepts them.
pub(super) const OP_ID: &str = "0000000000000000000000000000000000000000000000000000000000000abc";
pub(super) const REPLY_ID: &str =
    "0000000000000000000000000000000000000000000000000000000000000de1";
pub(super) const REPOST_ID: &str =
    "0000000000000000000000000000000000000000000000000000000000000f06";

pub(super) struct Harness {
    pub(super) engine: Arc<OpFeedEngine>,
    lookup: Arc<Mutex<HashMap<EventId, KernelEvent>>>,
}

impl Harness {
    pub(super) fn new(follows: &[&str]) -> Self {
        let follow_set: std::collections::HashSet<String> =
            follows.iter().map(|s| (*s).to_string()).collect();
        let follow: FollowPredicate = Arc::new(move |pk: &str| follow_set.contains(pk));

        let lookup: Arc<Mutex<HashMap<EventId, KernelEvent>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let lookup_for_cb = Arc::clone(&lookup);
        let event_lookup: EventLookup =
            Arc::new(move |id: &EventId| lookup_for_cb.lock().unwrap().get(id).cloned());

        let engine = register_op_feed(ALICE.to_string(), follow, event_lookup);
        Self { engine, lookup }
    }

    pub(super) fn ingest(&self, event: &KernelEvent) {
        self.lookup
            .lock()
            .unwrap()
            .insert(event.id.clone(), event.clone());
        self.engine.on_kernel_event(event);
    }

    pub(super) fn store(&self, event: &KernelEvent) {
        self.lookup
            .lock()
            .unwrap()
            .insert(event.id.clone(), event.clone());
    }

    pub(super) fn snapshot(
        &self,
    ) -> nmp_feed::RootFeedSnapshot<
        crate::timeline_projection::TimelineEventCard,
        Nip10ReplyAttribution,
    > {
        self.engine.snapshot(&FeedRequest::default())
    }
}

pub(super) fn op_event(id: &str, author: &str, created_at: u64, body: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at,
        tags: Vec::new(),
        content: body.to_string(),
        relay_provenance: Vec::new(),
    }
}

pub(super) fn reply_event(id: &str, author: &str, created_at: u64, root_id: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at,
        tags: vec![
            vec![
                "e".to_string(),
                root_id.to_string(),
                String::new(),
                "root".to_string(),
            ],
            vec![
                "e".to_string(),
                root_id.to_string(),
                String::new(),
                "reply".to_string(),
            ],
        ],
        content: "a reply".to_string(),
        relay_provenance: Vec::new(),
    }
}

pub(super) fn reply_to_parent(
    id: &str,
    author: &str,
    created_at: u64,
    parent_id: &str,
) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at,
        tags: vec![vec![
            "e".to_string(),
            parent_id.to_string(),
            String::new(),
            "reply".to_string(),
        ]],
        content: "reply to a repost".to_string(),
        relay_provenance: Vec::new(),
    }
}

pub(super) fn repost_etag(id: &str, author: &str, created_at: u64, target: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 6,
        created_at,
        tags: vec![vec!["e".to_string(), target.to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

pub(super) fn repost_embedded(
    id: &str,
    author: &str,
    created_at: u64,
    target: &KernelEvent,
) -> KernelEvent {
    let embedded = serde_json::json!({
        "id": target.id,
        "pubkey": target.author,
        "kind": target.kind,
        "created_at": target.created_at,
        "tags": target.tags,
        "content": target.content,
    });
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 6,
        created_at,
        tags: vec![vec!["e".to_string(), target.id.clone()]],
        content: embedded.to_string(),
        relay_provenance: Vec::new(),
    }
}
