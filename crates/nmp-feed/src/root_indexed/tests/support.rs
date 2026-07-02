//! Synthetic test fixtures for the engine tests. Everything here is invented
//! in-crate — a fake `ParentResolver` driven by invented tag conventions, a
//! fake `AttributionPayload`, fake closures, and a
//! `Harness` that drives the engine the way the kernel observer would. Proves
//! the engine is substrate-generic: not a single NIP type is named.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{EventId, KernelEvent};
use nmp_threading::{pointer::ThreadPointer, ParentResolver};

use crate::root_indexed::attribution::AttributionPayload;
use crate::root_indexed::card::RootFeedSnapshot;
use crate::root_indexed::engine::{
    CardBuilder, EventGate, EventLookup, FollowPredicate, RootIndexedFeed,
};
use crate::{FeedRequest, FeedWindowPolicy};

// ─── Synthetic resolver ────────────────────────────────────────────────────
//
// Tag conventions (invented for the test, NOT a protocol):
//   ["root", id]         → thread root pointer (Event)
//   ["parent", id]       → direct parent pointer (Event)
//   ["root_addr", coord] → root pointer (Address)
//   ["root_ext", uri]    → root pointer (External)
//   ["repost", target]   → this event supersedes target
//   ["profile", pubkey]  → ignored by the generic feed; mounted components own profiles

pub(super) struct TestResolver;

impl TestResolver {
    pub(super) fn tag<'a>(event: &'a KernelEvent, key: &str) -> Option<&'a str> {
        event
            .tags
            .iter()
            .find(|t| t.first().map(String::as_str) == Some(key))
            .and_then(|t| t.get(1))
            .map(String::as_str)
    }
}

impl ParentResolver for TestResolver {
    fn parent(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        if let Some(id) = Self::tag(event, "parent") {
            return Some(ThreadPointer::Event {
                id: id.to_string(),
                relay: None,
                kind: None,
            });
        }
        self.root(event)
    }

    fn root(&self, event: &KernelEvent) -> Option<ThreadPointer> {
        if let Some(id) = Self::tag(event, "root") {
            return Some(ThreadPointer::Event {
                id: id.to_string(),
                relay: Some("wss://hint.example".to_string()),
                kind: None,
            });
        }
        if let Some(coord) = Self::tag(event, "root_addr") {
            return Some(ThreadPointer::Address {
                coord: coord.to_string(),
                relay: None,
                kind: None,
            });
        }
        if let Some(uri) = Self::tag(event, "root_ext") {
            return Some(ThreadPointer::External {
                uri: uri.to_string(),
            });
        }
        None
    }

    fn parent_author(&self, _event: &KernelEvent) -> Option<String> {
        None
    }

    fn supersedes(&self, event: &KernelEvent) -> Option<EventId> {
        Self::tag(event, "repost").map(str::to_string)
    }
}

// ─── Synthetic payload ─────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct TestPayload {
    pub(super) reply_id: String,
    pub(super) author: String,
    pub(super) created_at: u64,
}

impl AttributionPayload for TestPayload {
    fn from_reply(reply: &KernelEvent, follow: &dyn Fn(&str) -> bool) -> Option<Self> {
        if !follow(&reply.author) {
            return None;
        }
        Some(Self {
            reply_id: reply.id.clone(),
            author: reply.author.clone(),
            created_at: reply.created_at,
        })
    }

    fn reply_event_id(&self) -> &str {
        &self.reply_id
    }
}

// ─── Synthetic card ────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct TestCard {
    pub(super) root_id: String,
    pub(super) body: String,
    /// Populated by the card_builder when the card is built from a repost pair
    /// (`target` present) — the wrapper author. Proves L-5 late-target rebuild
    /// keeps the repost provenance.
    pub(super) reposted_by: Option<String>,
}

// ─── Test harness ──────────────────────────────────────────────────────────

pub(super) type Engine = RootIndexedFeed<TestResolver, TestPayload, TestCard>;

pub(super) struct Harness {
    pub(super) engine: Arc<Engine>,
    lookup: Arc<Mutex<HashMap<EventId, KernelEvent>>>,
}

impl Harness {
    pub(super) fn new(follows: &[&str]) -> Self {
        // Allow-all gate: existing tests exercise the post-gate state machine.
        Self::with_gate(follows, Arc::new(|_| true))
    }

    /// Construct a harness with a caller-supplied [`EventGate`], so a test can
    /// assert that gated-out kinds never touch engine state. Roots admit-all
    /// (the perspective gate is exercised by [`Self::with_root_admission`]).
    pub(super) fn with_gate(follows: &[&str], event_gate: EventGate) -> Self {
        Self::with_root_admission(follows, event_gate, crate::admit_all_roots())
    }

    /// Construct a harness with a caller-supplied ROOT-admission predicate, so a
    /// test can assert the compiled perspective gates which roots enter the feed
    /// (#1740 step 3).
    pub(super) fn with_root_admission(
        follows: &[&str],
        event_gate: EventGate,
        root_admission: crate::RootAdmission,
    ) -> Self {
        Self::with_root_admission_and_window_policy(
            follows,
            event_gate,
            root_admission,
            FeedWindowPolicy::default(),
        )
    }

    pub(super) fn with_root_admission_and_window_policy(
        follows: &[&str],
        event_gate: EventGate,
        root_admission: crate::RootAdmission,
        window_policy: FeedWindowPolicy,
    ) -> Self {
        let follow_set: HashSet<String> = follows.iter().map(|s| (*s).to_string()).collect();
        let follow: FollowPredicate = Arc::new(move |pk: &str| follow_set.contains(pk));

        let lookup: Arc<Mutex<HashMap<EventId, KernelEvent>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let lookup_for_cb = Arc::clone(&lookup);
        let event_lookup: EventLookup =
            Arc::new(move |id: &EventId| lookup_for_cb.lock().unwrap().get(id).cloned());

        // The first arg is the "primary" event the card is built from: a plain
        // root (target = None) or a repost wrapper (target = the reposted note).
        // For a repost the card's identity is the TARGET root; `reposted_by`
        // carries the wrapper author so the renderer can show the banner.
        let card_builder: CardBuilder<TestCard> = Box::new(
            |primary: &KernelEvent, target: Option<&KernelEvent>| match target {
                Some(t) => TestCard {
                    root_id: t.id.clone(),
                    body: t.content.clone(),
                    reposted_by: Some(primary.author.clone()),
                },
                None => {
                    let repost_target = TestResolver::tag(primary, "repost");
                    TestCard {
                        root_id: repost_target.unwrap_or(primary.id.as_str()).to_string(),
                        body: primary.content.clone(),
                        reposted_by: repost_target.map(|_| primary.author.clone()),
                    }
                }
            },
        );

        let engine = Arc::new(RootIndexedFeed::new_with_window_policy(
            TestResolver,
            follow,
            root_admission,
            event_gate,
            event_lookup,
            card_builder,
            window_policy,
        ));

        Self { engine, lookup }
    }

    pub(super) fn store(&self, event: &KernelEvent) {
        self.lookup
            .lock()
            .unwrap()
            .insert(event.id.clone(), event.clone());
    }

    /// Feed an event the way the kernel would: it is in the read cache AND it
    /// fires the observer.
    pub(super) fn ingest(&self, event: &KernelEvent) {
        self.store(event);
        use nmp_core::ObservedProjectionSink;
        self.engine.on_kernel_event(event);
    }

    pub(super) fn snapshot(&self) -> RootFeedSnapshot<TestCard, TestPayload> {
        self.engine.snapshot(&FeedRequest::default())
    }
}

// ─── Event builders ────────────────────────────────────────────────────────

pub(super) fn root_event(id: &str, author: &str, created_at: u64, body: &str) -> KernelEvent {
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
            vec!["root".to_string(), root_id.to_string()],
            vec!["parent".to_string(), root_id.to_string()],
        ],
        content: "a reply".to_string(),
        relay_provenance: Vec::new(),
    }
}

pub(super) fn repost_event(
    id: &str,
    author: &str,
    created_at: u64,
    target: &str,
    body: &str,
) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 6,
        created_at,
        tags: vec![vec!["repost".to_string(), target.to_string()]],
        content: body.to_string(),
        relay_provenance: Vec::new(),
    }
}
