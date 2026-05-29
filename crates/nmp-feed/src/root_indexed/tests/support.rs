//! Synthetic test fixtures for the engine tests. Everything here is invented
//! in-crate — a fake `ParentResolver` driven by invented tag conventions, a
//! fake `AttributionPayload` with a trivial `Profile`, fake closures, and a
//! `Harness` that drives the engine the way the kernel observer would. Proves
//! the engine is substrate-generic: not a single NIP type is named.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{EventId, KernelEvent};
use nmp_threading::{pointer::ThreadPointer, ParentResolver};

use crate::root_indexed::attribution::AttributionPayload;
use crate::root_indexed::card::RootFeedSnapshot;
use crate::root_indexed::claim::ClaimRequest;
use crate::root_indexed::engine::{
    CardBuilder, ClaimSink, EventGate, EventLookup, FollowPredicate, ProfileDetector,
    RootIndexedFeed,
};
use crate::FeedRequest;

// ─── Synthetic resolver ────────────────────────────────────────────────────
//
// Tag conventions (invented for the test, NOT a protocol):
//   ["root", id]         → thread root pointer (Event)
//   ["parent", id]       → direct parent pointer (Event)
//   ["root_addr", coord] → root pointer (Address)
//   ["root_ext", uri]    → root pointer (External)
//   ["repost", target]   → this event supersedes target
//   ["profile", pubkey]  → handled by the profile_detector closure

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

// ─── Synthetic payload + profile ───────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct TestProfile {
    pub(super) display_name: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct TestPayload {
    pub(super) reply_id: String,
    pub(super) author: String,
    pub(super) created_at: u64,
    pub(super) display_name: Option<String>,
}

impl AttributionPayload for TestPayload {
    type Profile = TestProfile;

    fn from_reply(
        reply: &KernelEvent,
        follow: &dyn Fn(&str) -> bool,
        profile_for: &dyn Fn(&str) -> Option<Self::Profile>,
    ) -> Option<Self> {
        if !follow(&reply.author) {
            return None;
        }
        Some(Self {
            reply_id: reply.id.clone(),
            author: reply.author.clone(),
            created_at: reply.created_at,
            display_name: profile_for(&reply.author).map(|p| p.display_name),
        })
    }

    fn reply_event_id(&self) -> &str {
        &self.reply_id
    }

    fn author_pubkey(&self) -> &str {
        &self.author
    }

    fn reply_created_at(&self) -> u64 {
        self.created_at
    }

    fn refresh_for_profile(&mut self, profile: &Self::Profile) {
        self.display_name = Some(profile.display_name.clone());
    }
}

// ─── Synthetic card ────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct TestCard {
    pub(super) root_id: String,
    pub(super) body: String,
    /// Populated by the card_builder when the card is built from a repost pair
    /// (`target` present) — the wrapper author. Proves L-5 backward hydration
    /// keeps the repost provenance.
    pub(super) reposted_by: Option<String>,
}

// ─── Test harness ──────────────────────────────────────────────────────────

pub(super) type Engine = RootIndexedFeed<TestResolver, TestPayload, TestCard>;

pub(super) struct Harness {
    pub(super) engine: Arc<Engine>,
    claims: Arc<Mutex<Vec<ClaimRequest>>>,
    lookup: Arc<Mutex<HashMap<EventId, KernelEvent>>>,
}

impl Harness {
    pub(super) fn new(follows: &[&str]) -> Self {
        // Allow-all gate: existing tests exercise the post-gate state machine.
        Self::with_gate(follows, Arc::new(|_| true))
    }

    /// Construct a harness with a caller-supplied [`EventGate`], so a test can
    /// assert that gated-out kinds never touch engine state.
    pub(super) fn with_gate(follows: &[&str], event_gate: EventGate) -> Self {
        let follow_set: HashSet<String> = follows.iter().map(|s| (*s).to_string()).collect();
        let follow: FollowPredicate = Arc::new(move |pk: &str| follow_set.contains(pk));

        let lookup: Arc<Mutex<HashMap<EventId, KernelEvent>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let lookup_for_cb = Arc::clone(&lookup);
        let event_lookup: EventLookup =
            Arc::new(move |id: &EventId| lookup_for_cb.lock().unwrap().get(id).cloned());

        let claims: Arc<Mutex<Vec<ClaimRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let claims_for_cb = Arc::clone(&claims);
        let claim_sink: ClaimSink =
            Arc::new(move |req| claims_for_cb.lock().unwrap().push(req));

        let profile_detector: ProfileDetector<TestPayload> = Box::new(|event: &KernelEvent| {
            TestResolver::tag(event, "profile").map(|pk| {
                (
                    pk.to_string(),
                    TestProfile {
                        display_name: event.content.clone(),
                    },
                )
            })
        });

        // The first arg is the "primary" event the card is built from: a plain
        // root (target = None) or a repost wrapper (target = the reposted note).
        // For a repost the card's identity is the TARGET root; `reposted_by`
        // carries the wrapper author so the renderer can show the banner.
        let card_builder: CardBuilder<TestCard> =
            Box::new(|primary: &KernelEvent, target: Option<&KernelEvent>| match target {
                Some(t) => TestCard {
                    root_id: t.id.clone(),
                    body: t.content.clone(),
                    reposted_by: Some(primary.author.clone()),
                },
                None => TestCard {
                    root_id: primary.id.clone(),
                    body: primary.content.clone(),
                    reposted_by: None,
                },
            });

        let engine = Arc::new(RootIndexedFeed::new(
            TestResolver,
            follow,
            event_gate,
            event_lookup,
            claim_sink,
            profile_detector,
            card_builder,
            "nmp.feed.home",
        ));

        Self {
            engine,
            claims,
            lookup,
        }
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
        use nmp_core::KernelEventObserver;
        self.engine.on_kernel_event(event);
    }

    pub(super) fn claims(&self) -> Vec<ClaimRequest> {
        self.claims.lock().unwrap().clone()
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
    }
}

pub(super) fn profile_event(author: &str, subject: &str, display_name: &str) -> KernelEvent {
    KernelEvent {
        id: format!("profile-{subject}"),
        author: author.to_string(),
        kind: 0,
        created_at: 100,
        tags: vec![vec!["profile".to_string(), subject.to_string()]],
        content: display_name.to_string(),
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
    }
}
