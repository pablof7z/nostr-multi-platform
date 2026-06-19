//! ADR-0061 surface → existing-pager E2E.
//!
//! Proves the canonical feed surface (`open_feed` + `set_feed_viewport`) drives
//! the EXISTING `PullFeedController` mechanism — no parallel paging path — using
//! the SAME production wiring `register_op_feed_defaults` installs
//! (`install_home_feed_surface`, exercised directly here so the test needs no
//! full actor).
//!
//! Scenario:
//! 1. Build the real OP-feed engine + a `PullFeedController` over a real
//!    `MemEventStore` (mirrors `pull_feed_seq1_e2e.rs`).
//! 2. Install it into a `FeedSurface` via the production helper — the home
//!    `"notes"` profile (carrying the `{1,6}` kinds) + the home descriptor
//!    opener that REUSES this controller.
//! 3. `open_feed(home descriptor)` returns the deterministic key.
//! 4. `set_feed_viewport` past the prefetch threshold engages the pager and the
//!    projection grows; a viewport far from the tail does NOT.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::collections::BTreeSet;

use nmp_core::planner::InterestShape;
use nmp_core::store::{EventStore, MemEventStore, PullPage, RawEvent, ScanLogResult, VerifiedEvent};
use nmp_core::KernelEventObserver;
use nmp_core::{pull_page_over, PullLimits, PullScope};
use nmp_defaults::op_feed_defaults::install_home_feed_surface;
use nmp_feed::{
    ClosureInterestShape, FeedController, FeedDescriptor, FeedScope, FeedSource, FeedSurface,
    FeedViewportIntent, PullFeedController,
};
use nmp_nip01::op_feed::{build_actor_claim_sink, register_op_feed};

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const RELAY: &str = "wss://test.relay/";
const HOME_KINDS: [u32; 2] = [1, 6];

fn raw_event(id: &str, created_at: u64) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: ALICE.to_string(),
        created_at,
        kind: 1,
        tags: vec![],
        content: format!("note {id}"),
        sig: "00".repeat(64),
    }
}

fn insert(store: &MemEventStore, raw: RawEvent) {
    store
        .insert(VerifiedEvent::from_raw_unchecked(raw), &RELAY.to_string(), 1_000)
        .expect("insert");
}

fn home_descriptor_json() -> String {
    serde_json::to_string(&FeedDescriptor {
        profile: "notes".into(),
        source: FeedSource::HomeFollowSet {},
        scope: FeedScope::ActiveAccount,
    })
    .unwrap()
}

#[test]
fn open_home_then_viewport_drives_the_existing_pager_and_grows_the_projection() {
    // ── 1. Real store + OP engine + PullFeedController (production wiring) ────
    let store = Arc::new(MemEventStore::new());
    let engine = register_op_feed(
        ALICE.to_string(),
        Arc::new(|_pk: &str| true),
        Arc::new(|_id: &nmp_core::substrate::EventId| None),
        build_actor_claim_sink(Arc::new(|_cmd| {})),
    );

    let shape = InterestShape {
        authors: [ALICE.to_string()].into_iter().collect::<BTreeSet<_>>(),
        kinds: HOME_KINDS.into_iter().collect(),
        ..Default::default()
    };
    let provider = Arc::new(ClosureInterestShape::new(move || Some(shape.clone())));

    let pull_limits = PullLimits {
        max_entries: NonZeroUsize::new(50).unwrap(),
        max_scan_entries: NonZeroUsize::new(400).unwrap(),
    };
    let store_for_pull = Arc::clone(&store);
    let pull = Arc::new(move |scope: PullScope, after_seq: u64| {
        match pull_page_over(store_for_pull.as_ref(), scope, after_seq, pull_limits) {
            Ok(result) => result,
            Err(_) => ScanLogResult::Page(PullPage {
                entries: vec![],
                next_after_seq: after_seq,
                latest_seq: after_seq,
                has_more: false,
            }),
        }
    });
    let engine_for_apply = engine.clone();
    let apply = Arc::new(move |ev: &nmp_core::substrate::KernelEvent| {
        KernelEventObserver::on_kernel_event(&*engine_for_apply, ev);
    });
    let engine_for_advance = engine.clone();
    let advance = Arc::new(move || {
        engine_for_advance.grow_visible_window();
    });
    let controller: Arc<dyn FeedController> =
        PullFeedController::new(provider, pull, apply, advance);

    // ── 2. Install via the PRODUCTION helper (reuses the controller) ─────────
    let surface = FeedSurface::default();
    install_home_feed_surface(&surface, Arc::clone(&controller), &HOME_KINDS);

    // ── 3. Two events present in the store before any drain ──────────────────
    insert(&store, raw_event(&"a".repeat(64), 1_000));
    insert(&store, raw_event(&"b".repeat(64), 1_100));
    assert_eq!(
        engine.snapshot_current_window().cards.len(),
        0,
        "pull-only: nothing ingested until the surface drives a drain"
    );

    // ── 4. open_feed returns the deterministic key ───────────────────────────
    let handle = surface.open(&home_descriptor_json()).expect("open");
    assert_eq!(
        handle.key,
        nmp_feed::canonical_feed_key(&FeedDescriptor {
            profile: "notes".into(),
            source: FeedSource::HomeFollowSet {},
            scope: FeedScope::ActiveAccount,
        }),
        "key is the deterministic descriptor canonicalization"
    );

    // ── 5a. Viewport at the tail drives the existing pager ───────────────────
    let changed = surface.set_viewport(
        handle.key.as_str(),
        FeedViewportIntent {
            first_visible: 0,
            last_visible: 0,
            rendered_len: 0,
        },
    );
    assert!(changed, "viewport at the tail must drive the pull drain");
    let cards = engine.snapshot_current_window().cards.len();
    assert_eq!(cards, 2, "the drain ingested both events and grew the window");

    // ── 5b. A viewport far from the tail drives nothing ──────────────────────
    let changed2 = surface.set_viewport(
        handle.key.as_str(),
        FeedViewportIntent {
            first_visible: 0,
            last_visible: 0,
            rendered_len: 100,
        },
    );
    assert!(!changed2, "far from the tail ⇒ no drain (NMP owns the policy)");
}
