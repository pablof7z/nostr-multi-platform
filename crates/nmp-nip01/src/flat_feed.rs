//! `FlatFeed` — a predicate-gated flat note feed (ADR-0042 §5.1).
//!
//! The M2 author/thread read-path replacement. Unlike the OP-centric home feed
//! ([`crate::OpFeedEngine`] / [`crate::register_op_feed`]), which is a stream of
//! **thread roots only** with a followed author's replies rolled up as
//! *attribution* metadata, a profile screen and a thread screen each render a
//! **flat list** where every matching note is its own top-level row:
//!
//! * **Author feed** — every kind:1/6 authored by one pubkey (including that
//!   author's replies to other people), shown as top-level rows. The
//!   root-indexed engine structurally cannot express this (it would hide the
//!   replies under other people's roots).
//! * **Thread feed** — the root note plus every kind:1/6 that references it via
//!   `#e`, each as its own row (`ThreadScreen` does `ForEach(thread.items)`).
//!
//! Both are the same machine: a flat, newest-first, D5-windowed list of
//! [`TimelineEventCard`]s, gated by an injected admission predicate. The
//! emitted snapshot is the **same** [`RootFeedSnapshot`] wire shape the home
//! feed emits (`RootCard { card, attribution }`), with `attribution` always
//! empty — so the iOS/Android shells decode it through the existing
//! `nmp.feed.home` reader with zero new FlatBuffers schema or codegen. The kind
//! decisions (`{1,6}`) live in the host that builds the predicate (D0-correct);
//! `nmp-nip01` only knows how to render a kind:1/6 card.
//!
//! Registration mirrors [`crate::register_op_feed`]: the host registers a
//! `FlatFeed` as both a [`KernelEventObserver`] (ingest fan-out) and a
//! [`FeedController`] under its own snapshot key (`nmp.feed.author.<pk>` /
//! `nmp.feed.thread.<id>`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;
use nmp_feed::{FeedController, FeedCursor, FeedPage, FeedRequest, RootCard, RootFeedSnapshot};

use crate::{Nip10ReplyAttribution, TimelineEventCard};

/// Admission predicate: `true` when `event` belongs in this flat feed.
///
/// The host builds this — e.g. `move |e| e.author == pk && (e.kind == 1 ||
/// e.kind == 6)` for an author feed, or a root-plus-`#e`-referrers test for a
/// thread feed. Keeping the predicate host-supplied is what keeps the `{1,6}`
/// kind policy out of the substrate (D0).
pub type FlatFeedPredicate = Arc<dyn Fn(&KernelEvent) -> bool + Send + Sync>;

/// One stored row, keyed for newest-first ordering and de-dup by id.
#[derive(Clone)]
struct FlatRow {
    created_at: u64,
    card: TimelineEventCard,
}

#[derive(Default)]
struct FlatFeedState {
    /// `event_id -> row`. A re-arrival of the same id refreshes the card
    /// (mirrors the kernel's replace semantics — the observer only fires on a
    /// genuine insert/replace, so a refresh here is a real update).
    rows: BTreeMap<String, FlatRow>,
}

/// A flat, predicate-gated note feed. Wire-compatible with the home feed's
/// [`RootFeedSnapshot`] (empty `attribution`).
pub struct FlatFeed {
    predicate: FlatFeedPredicate,
    state: Mutex<FlatFeedState>,
}

impl FlatFeed {
    /// Construct a flat feed admitting events for which `predicate` is `true`.
    #[must_use]
    pub fn new(predicate: FlatFeedPredicate) -> Arc<Self> {
        Arc::new(Self {
            predicate,
            state: Mutex::new(FlatFeedState::default()),
        })
    }

    /// Ingest one event: render and store it iff the predicate admits it.
    ///
    /// Cheap and panic-free — runs on the actor thread between relay frames
    /// (the [`KernelEventObserver`] contract). A poisoned lock is a silent
    /// no-op (D6): the feed degrades to whatever it last held rather than
    /// aborting ingest.
    fn ingest(&self, event: &KernelEvent) {
        if !(self.predicate)(event) {
            return;
        }
        let card = TimelineEventCard::from_event_for_op_feed(event, None);
        if let Ok(mut st) = self.state.lock() {
            st.rows.insert(
                event.id.clone(),
                FlatRow {
                    created_at: event.created_at,
                    card,
                },
            );
        }
    }

    /// Build the visible-window snapshot: cards newest-first by
    /// `(created_at, id)`, windowed to the request limit (D5). `attribution` is
    /// always empty — a flat feed has no per-root attribution rollup.
    #[must_use]
    pub fn snapshot(
        &self,
        request: &FeedRequest,
    ) -> RootFeedSnapshot<TimelineEventCard, Nip10ReplyAttribution> {
        let Ok(st) = self.state.lock() else {
            return RootFeedSnapshot {
                cards: Vec::new(),
                page: None,
                metrics: None,
            };
        };

        // Order newest-first by (created_at, id) — same ordering as the
        // RootIndexedFeed snapshot so the two feeds sort identically.
        let mut ordered: Vec<(u64, &String, &TimelineEventCard)> = st
            .rows
            .iter()
            .map(|(id, row)| (row.created_at, id, &row.card))
            .collect();
        ordered.sort_by(|(lt, lid, _), (rt, rid, _)| rt.cmp(lt).then_with(|| rid.cmp(lid)));

        let limit = request.bounded_limit();
        let total = ordered.len();
        let end = limit.min(total);
        let has_more = end < total;
        let next_cursor = if has_more {
            ordered.get(end - 1).map(|(created_at, id, _)| FeedCursor {
                created_at: *created_at,
                id: (*id).clone(),
            })
        } else {
            None
        };

        let cards = ordered[..end]
            .iter()
            .map(|(_, _, card)| RootCard {
                card: (*card).clone(),
                attribution: Vec::new(),
            })
            .collect::<Vec<_>>();

        RootFeedSnapshot {
            cards,
            page: Some(FeedPage {
                limit,
                next_cursor,
                has_more,
                total_blocks: total,
            }),
            metrics: None,
        }
    }

    /// Number of rows currently held (drives the host's `noteCountDisplay`
    /// composition — the count the deleted `author_view.noteCountDisplay`
    /// formatted).
    #[must_use]
    pub fn len(&self) -> usize {
        self.state.lock().map(|st| st.rows.len()).unwrap_or(0)
    }

    /// `true` when no rows are held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl KernelEventObserver for FlatFeed {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.ingest(event);
    }
}

impl FeedController for FlatFeed {
    fn load_older(&self) -> bool {
        // All admitted rows are held in memory bounded by D5 retention; "load
        // older" widens the snapshot request limit at the call site rather than
        // advancing a paging cursor in the feed itself. Mirrors the
        // RootIndexedFeed `FeedController::load_older` no-op.
        false
    }
}

/// Build an **author-feed** predicate: a host-chosen kind set authored by one
/// pubkey. The `{1,6}` decision lives here (in nmp-nip01's helper, callable by
/// the host) — the substrate never sees it.
///
/// `kinds` is the host's note-kind policy (Chirp passes `[1, 6]`). `author` is
/// the raw hex pubkey.
#[must_use]
pub fn author_feed_predicate(author: String, kinds: Vec<u32>) -> FlatFeedPredicate {
    Arc::new(move |event: &KernelEvent| event.author == author && kinds.contains(&event.kind))
}

/// Build a **thread-feed** predicate: the root note itself plus every event of
/// a host-chosen kind that references the root via a NIP-10 `#e` tag.
///
/// Crucially this admits the root event by id (`event.id == root_id`) — a
/// `{"kinds":[1,6],"#e":[root]}` filter alone would fetch the *replies* but not
/// the root, and `ThreadScreen` must show the root as a row. The `#e` match is
/// any `e` tag whose value equals `root_id` (NIP-10 root or reply marker).
#[must_use]
pub fn thread_feed_predicate(root_id: String, kinds: Vec<u32>) -> FlatFeedPredicate {
    Arc::new(move |event: &KernelEvent| {
        if event.id == root_id {
            return true;
        }
        if !kinds.contains(&event.kind) {
            return false;
        }
        event
            .tags
            .iter()
            .any(|tag| tag.first().map(String::as_str) == Some("e") && tag.get(1) == Some(&root_id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(
        id: &str,
        author: &str,
        kind: u32,
        created_at: u64,
        tags: Vec<Vec<String>>,
    ) -> KernelEvent {
        KernelEvent {
            id: id.to_string(),
            author: author.to_string(),
            kind,
            created_at,
            tags,
            content: format!("note {id}"),
            relay_provenance: Vec::new(),
        }
    }

    fn etag(id: &str) -> Vec<String> {
        vec!["e".to_string(), id.to_string()]
    }

    #[test]
    fn author_feed_admits_only_that_author_and_kinds() {
        let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
        feed.ingest(&ev("a1", "alice", 1, 100, vec![]));
        feed.ingest(&ev("a2", "alice", 6, 101, vec![])); // repost — admitted
        feed.ingest(&ev("a3", "alice", 7, 102, vec![])); // reaction — rejected
        feed.ingest(&ev("b1", "bob", 1, 103, vec![])); // other author — rejected
        assert_eq!(feed.len(), 2);
        let snap = feed.snapshot(&FeedRequest::default());
        // Newest-first: a2 (101) before a1 (100).
        assert_eq!(snap.cards.len(), 2);
        assert_eq!(snap.cards[0].card.id, "a2");
        assert_eq!(snap.cards[1].card.id, "a1");
        // Flat feed never carries attribution.
        assert!(snap.cards.iter().all(|c| c.attribution.is_empty()));
    }

    #[test]
    fn author_feed_includes_replies_to_others_as_top_level_rows() {
        // The exact case RootIndexedFeed cannot express: alice's reply to bob's
        // note is a top-level row in alice's profile, not attribution under bob.
        let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
        feed.ingest(&ev("reply", "alice", 1, 200, vec![etag("bobs_root")]));
        assert_eq!(feed.len(), 1);
        assert_eq!(
            feed.snapshot(&FeedRequest::default()).cards[0].card.id,
            "reply"
        );
    }

    #[test]
    fn thread_feed_admits_root_by_id_and_referrers_by_etag() {
        let feed = FlatFeed::new(thread_feed_predicate("root".to_string(), vec![1, 6]));
        feed.ingest(&ev("root", "alice", 1, 100, vec![])); // root — by id
        feed.ingest(&ev("reply1", "bob", 1, 101, vec![etag("root")])); // referrer
        feed.ingest(&ev("reply2", "carol", 1, 102, vec![etag("other")])); // unrelated
        feed.ingest(&ev("react", "dave", 7, 103, vec![etag("root")])); // wrong kind
        assert_eq!(feed.len(), 2);
        let ids: Vec<_> = feed
            .snapshot(&FeedRequest::default())
            .cards
            .iter()
            .map(|c| c.card.id.clone())
            .collect();
        assert_eq!(ids, vec!["reply1".to_string(), "root".to_string()]);
    }

    #[test]
    fn reingest_same_id_refreshes_not_duplicates() {
        let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
        feed.ingest(&ev("a1", "alice", 1, 100, vec![]));
        feed.ingest(&ev("a1", "alice", 1, 100, vec![]));
        assert_eq!(feed.len(), 1);
    }

    #[test]
    fn snapshot_windows_to_request_limit_with_cursor() {
        let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
        for i in 0..5u64 {
            feed.ingest(&ev(&format!("a{i}"), "alice", 1, 100 + i, vec![]));
        }
        let snap = feed.snapshot(&FeedRequest::newest(2));
        assert_eq!(snap.cards.len(), 2);
        let page = snap.page.expect("page");
        assert!(page.has_more);
        assert_eq!(page.total_blocks, 5);
        assert!(page.next_cursor.is_some());
    }

    #[test]
    fn on_kernel_event_observer_entrypoint_renders_matching_event() {
        // The load-bearing seam: in production the kernel admits an
        // open_interest-matched event into `self.events` and then calls
        // `notify_event_observers` → `FlatFeed::on_kernel_event` (NOT the
        // private `ingest`). `event_observer_tests.rs` proves the kernel fires
        // `on_kernel_event` once per accepted ingest; this proves the FlatFeed
        // observer entry point forwards through the predicate + render path, so
        // the full chain (admission → fan-out → snapshot) holds end-to-end.
        let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
        // Drive via the KernelEventObserver trait method, exactly as
        // `notify_event_observers` does.
        KernelEventObserver::on_kernel_event(&*feed, &ev("a1", "alice", 1, 100, vec![]));
        KernelEventObserver::on_kernel_event(&*feed, &ev("b1", "bob", 1, 101, vec![]));
        // Only alice's note rendered (predicate gate honoured at the observer
        // entry point), and it surfaces in the FlatFeed snapshot.
        assert_eq!(feed.len(), 1);
        let snap = feed.snapshot(&nmp_feed::FeedRequest::default());
        assert_eq!(snap.cards.len(), 1);
        assert_eq!(snap.cards[0].card.id, "a1");
    }

    #[test]
    fn feed_controller_emits_home_feed_wire_shape() {
        // The snapshot must produce the RootFeedSnapshot shape the home
        // feed emits, so the existing Swift `nmp.feed.home` reader decodes it.
        let feed = FlatFeed::new(author_feed_predicate("alice".to_string(), vec![1, 6]));
        feed.ingest(&ev("a1", "alice", 1, 100, vec![]));
        let snap = feed.snapshot(&nmp_feed::FeedRequest::default());
        assert_eq!(snap.cards.len(), 1);
        assert!(snap.cards[0].attribution.is_empty());
    }
}
