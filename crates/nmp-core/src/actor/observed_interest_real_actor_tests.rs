//! #3088 — real-actor-thread regression coverage.
//!
//! Issue #3088 reports: a freshly-opened observed-projection interest that
//! shares a shape-set reconciliation pass with a reopened (already-existing)
//! interest does not reliably receive its own live event fan-out, discovered
//! via `crates/nmp-feed-session/src/dynamic_observer.rs`'s
//! `DynamicObservedProjectionSet::sync()` (lines 48-71): when its live shape
//! set grows from one shape to two, it closes ALL currently-open interests
//! then reopens ALL desired ones, sharing one `Arc<dyn ObservedProjectionSink>`
//! per lane.
//!
//! The first two tests below drive that EXACT scenario — close+reopen one
//! shape alongside a brand-new one, in one pass, through a REAL spawned actor
//! thread and a real bounded command queue via the same production API a
//! host uses (`ObservedProjectionCommandHandle::open`/`close`) — and they
//! PASS: reopening-alongside-a-fresh-open, and a dependent (REQ-only)
//! interest sharing the fresh shape's `SubKey`, are both fine. They are kept
//! as regression coverage for what #3088 is NOT.
//!
//! The actual root cause is in the THIRD test,
//! `addressable_target_shape_never_gets_live_fanout_though_replay_works`:
//! `crates/nmp-feed-session/src/pointer_target_hydration.rs`'s
//! `target_shape` builds the "newly demanded target's kind/coordinate shape"
//! mentioned in #3088 with a non-empty `InterestShape::addresses` set. That
//! field has no NIP-01 wire representation for LIVE activation purposes: see
//! the fix + root-cause note on `Kernel::open_interest_with_observer_replay`
//! in `kernel/observer_replay.rs`.

use crate::actor::test_actor_spawn::spawn_test_actor_with_event_observers;
use crate::actor::{
    new_event_observer_slot, ActorCommand, CommandSender, LifecycleCommand, ObservedProjectionSink,
    TestSupportCommand,
};
use crate::planner::{InterestShape, NaddrCoord};
use crate::store::{RawEvent, VerifiedEvent};
use crate::substrate::{KernelEvent, ObservedProjection, ObservedProjectionCommandHandle};
use crate::testing::wait_barrier;
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

const BARRIER_TIMEOUT: Duration = Duration::from_secs(5);

struct CapturingObserver {
    events: Mutex<Vec<KernelEvent>>,
}

impl CapturingObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(Vec::new()),
        })
    }

    fn count(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}

impl ObservedProjectionSink for CapturingObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

fn raw_event(id: &str, author: &str, kind: u32, created_at: u64) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at,
        kind,
        tags: vec![],
        content: "test".into(),
        sig: "a".repeat(128),
    }
}

fn ingest_and_settle(cmd_tx: &CommandSender, event: RawEvent) {
    cmd_tx
        .send(ActorCommand::TestSupport(
            TestSupportCommand::IngestPreVerifiedEvents(vec![VerifiedEvent::from_raw_unchecked(
                event,
            )]),
        ))
        .expect("actor inbox open");
    assert!(
        wait_barrier(cmd_tx, BARRIER_TIMEOUT),
        "actor must settle the ingest before the barrier ack"
    );
}

/// #3088 — see module docs.
#[test]
fn fresh_interest_coalesced_with_reopened_one_gets_live_fanout_via_real_actor() {
    let (cmd_tx, cmd_rx) = CommandSender::bounded_channel();
    let (upd_tx, _upd_rx) = mpsc::channel();
    let actor_self_tx = cmd_tx.clone();
    let event_observers = new_event_observer_slot();
    {
        let event_observers = event_observers.clone();
        thread::spawn(move || {
            spawn_test_actor_with_event_observers(cmd_rx, actor_self_tx, upd_tx, event_observers);
        });
    }

    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handle = ObservedProjectionCommandHandle::new(event_observers, sessions, cmd_tx.clone());

    let author_x = "1".repeat(64);
    let author_y = "2".repeat(64);
    let shape_x = InterestShape::from_filter_json(&format!(
        r#"{{"kinds":[1],"authors":["{author_x}"]}}"#
    ))
    .expect("valid shape x");
    let shape_y = InterestShape::from_filter_json(&format!(
        r#"{{"kinds":[1],"authors":["{author_y}"]}}"#
    ))
    .expect("valid shape y");
    // Same consumer_id for both shapes — `DynamicObservedProjectionSet` opens
    // every lane under one logical source consumer.
    let consumer_id = "composite-feed-source";

    // ── Pass 1: one live shape (X). ──────────────────────────────────────
    let obs_a = CapturingObserver::new();
    let id_a = handle.open(ObservedProjection::from_shape(
        obs_a.clone(),
        consumer_id,
        1,
        shape_x.clone(),
        80,
    ));
    assert!(id_a.0 != 0, "interest A must open");
    assert!(
        wait_barrier(&cmd_tx, BARRIER_TIMEOUT),
        "actor must settle the open before the baseline probe"
    );

    ingest_and_settle(&cmd_tx, raw_event(&"a".repeat(64), &author_x, 1, 100));
    assert_eq!(
        obs_a.count(),
        1,
        "baseline: the sole live interest receives its own live event"
    );

    // ── Pass 2: live_shapes() grows 1 -> 2 in one reconciliation pass.
    // `sync()` closes ALL current (X/A) then reopens ALL desired (X as a new
    // observer B, Y fresh as a new observer C) — dynamic_observer.rs:53-65.
    // `handle.close`/`handle.open` register/unregister the Rust sink
    // SYNCHRONOUSLY (on this thread) but only QUEUE the paired
    // `CloseInterest`/`OpenObservedInterest` `ActorCommand` — those are
    // applied later, asynchronously, by the real actor thread.
    handle.close(id_a);

    // `DynamicObservedProjectionSet::sync()` reopens EVERY shape with
    // `Arc::clone(&self.observer)` — the SAME sink instance for every lane,
    // not a fresh one per shape (dynamic_observer.rs:56). Mirror that here.
    let shared_obs = CapturingObserver::new();
    let id_b = handle.open(ObservedProjection::from_shape(
        shared_obs.clone(),
        consumer_id,
        1,
        shape_x.clone(),
        80,
    ));
    assert!(id_b.0 != 0, "interest B (reopened X) must open");

    let id_c = handle.open(ObservedProjection::from_shape(
        shared_obs.clone(),
        consumer_id,
        1,
        shape_y.clone(),
        80,
    ));
    assert!(id_c.0 != 0, "interest C (fresh Y) must open");

    assert!(
        wait_barrier(&cmd_tx, BARRIER_TIMEOUT),
        "actor must settle the close+reopen+open pass"
    );

    let shared_before_y = shared_obs.count();

    // A brand-new event matching ONLY the freshly-opened shape Y must reach C.
    ingest_and_settle(&cmd_tx, raw_event(&"b".repeat(64), &author_y, 1, 200));

    assert_eq!(
        shared_obs.count(),
        shared_before_y + 1,
        "fresh interest C (coalesced with reopened B in the same reconciliation \
         pass, driven through the real async actor command queue) must receive \
         its own live event — #3088"
    );

    handle.close(id_b);
    handle.close(id_c);
    let _ = cmd_tx.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown));
}

/// #3088 — same reconciliation pass as above, but ALSO fires the dependent
/// (REQ-only, no-observer) interest resync for the SAME fresh shape Y under a
/// DIFFERENT owner, in the SAME order `observed_source.rs`'s reactivity-hook
/// trigger uses: `sync_observer.sync()` (observer side) THEN
/// `acquisition_adapter.schedule_source_effect(...)` (dependent-interest
/// side) — see `crates/nmp-feed-session/src/observed_source.rs:112-124`.
/// `DependentInterestChild::tailing`'s doc comment states it intentionally
/// derives the SAME `SubKey` as `open_interest` "so a dependent child and an
/// explicit `OpenInterest` for the same shape/scope dedup onto one live
/// slot" (`kernel/dependent_interests.rs:20-23`) — i.e. the fresh observed
/// interest C and the dependent acquisition owner land on ONE registry slot.
#[test]
fn fresh_interest_still_gets_fanout_when_dependent_interest_shares_its_shape() {
    let (cmd_tx, cmd_rx) = CommandSender::bounded_channel();
    let (upd_tx, _upd_rx) = mpsc::channel();
    let actor_self_tx = cmd_tx.clone();
    let event_observers = new_event_observer_slot();
    {
        let event_observers = event_observers.clone();
        thread::spawn(move || {
            spawn_test_actor_with_event_observers(cmd_rx, actor_self_tx, upd_tx, event_observers);
        });
    }

    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handle = ObservedProjectionCommandHandle::new(event_observers, sessions, cmd_tx.clone());

    let author_x = "3".repeat(64);
    let author_y = "4".repeat(64);
    let shape_x = InterestShape::from_filter_json(&format!(
        r#"{{"kinds":[1],"authors":["{author_x}"]}}"#
    ))
    .expect("valid shape x");
    let shape_y = InterestShape::from_filter_json(&format!(
        r#"{{"kinds":[1],"authors":["{author_y}"]}}"#
    ))
    .expect("valid shape y");
    let consumer_id = "composite-feed-source-2";

    let obs_a = CapturingObserver::new();
    let id_a = handle.open(ObservedProjection::from_shape(
        obs_a.clone(),
        consumer_id,
        1,
        shape_x.clone(),
        80,
    ));
    assert!(id_a.0 != 0);
    assert!(wait_barrier(&cmd_tx, BARRIER_TIMEOUT));

    // ── Same reconciliation pass: observer side first (close+reopen X,
    // open fresh Y), THEN the dependent (REQ-only) acquisition-adapter
    // resync for the SAME shape Y under a DIFFERENT owner. ──────────────
    handle.close(id_a);
    let shared_obs = CapturingObserver::new();
    let id_b = handle.open(ObservedProjection::from_shape(
        shared_obs.clone(),
        consumer_id,
        1,
        shape_x.clone(),
        80,
    ));
    let id_c = handle.open(ObservedProjection::from_shape(
        shared_obs.clone(),
        consumer_id,
        1,
        shape_y.clone(),
        80,
    ));
    assert!(id_b.0 != 0 && id_c.0 != 0);

    let acquisition_owner = crate::subs::SubOwnerKey::new("acquisition-adapter");
    let dependent_child = crate::kernel::DependentInterestChild::tailing(
        shape_y.clone(),
        crate::planner::InterestScope::Global,
    );
    cmd_tx
        .send(ActorCommand::Interests(
            crate::actor::InterestsCommand::ApplyDependentInterestDelta {
                owner: acquisition_owner,
                delta: crate::kernel::DependentInterestDelta {
                    commands: vec![crate::kernel::DependentInterestDeltaCommand::Open(
                        dependent_child,
                    )],
                },
                reason: "feed-observed-source-acquisition".to_string(),
            },
        ))
        .expect("actor inbox open");

    assert!(
        wait_barrier(&cmd_tx, BARRIER_TIMEOUT),
        "actor must settle the observer opens + dependent-interest resync"
    );

    let shared_before_y = shared_obs.count();
    ingest_and_settle(&cmd_tx, raw_event(&"e".repeat(64), &author_y, 1, 300));

    assert_eq!(
        shared_obs.count(),
        shared_before_y + 1,
        "fresh interest C must receive its own live event even when a \
         dependent (REQ-only) interest shares its SubKey in the same pass — #3088"
    );

    handle.close(id_b);
    handle.close(id_c);
    let _ = cmd_tx.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown));
}


/// #3088 root cause — an observed-projection shape carrying `addresses`
/// (the "newly demanded target's kind/coordinate shape" from the issue)
/// receives cached-event REPLAY but never LIVE fan-out, with NO reopen or
/// coalescing involved at all — a single, lone, freshly-opened interest for
/// an addressable target is enough.
///
/// `ObservedProjection::from_shape` round-trips a shape through
/// `crate::subs::wire::filter_json_for` (wire JSON) and back through
/// `InterestShape::from_filter_json` (`crate::subs::interest_builder::build_open_interest`)
/// to build the `LogicalInterest` used for LIVE activation
/// (`kernel/observer_replay.rs`'s `let live_shape = interest.shape.clone();`).
/// `filter_json_for` serializes non-empty `shape.addresses` as a NIP-01 `#a`
/// generic-tag filter (`subs/wire.rs:264-280`, `Filter::coordinates`), but
/// `InterestShape::from_filter_json` has no `addresses` case — it reads
/// `#a` back through the generic single-letter-tag branch into
/// `shape.tags["a"]` instead (`nmp-planner/src/interest/shape.rs:258-275`),
/// producing a live-activation shape that requires the event to carry a
/// literal `["a", "<coord>"]` tag. An addressable event never self-tags its
/// own coordinate, so this spurious requirement can never be satisfied and
/// the live fan-out gate (`RustObserverDelivery::matches`,
/// `actor/commands/event_observer/delivery.rs:21-28`) permanently rejects
/// the very event the shape was opened to receive.
///
/// The REPLAY path is unaffected because it uses `replay_shapes` (the
/// ORIGINAL, un-round-tripped shape) directly, so a cached event replays
/// fine — only LIVE delivery of a freshly-arriving event is broken, exactly
/// matching #3088: "the demanded event IS fetched... via the relay
/// round-trip... [but] the sink's on_kernel_event is NEVER invoked."
#[test]
fn addressable_target_shape_never_gets_live_fanout_though_replay_works() {
    let (cmd_tx, cmd_rx) = CommandSender::bounded_channel();
    let (upd_tx, _upd_rx) = mpsc::channel();
    let actor_self_tx = cmd_tx.clone();
    let event_observers = new_event_observer_slot();
    {
        let event_observers = event_observers.clone();
        thread::spawn(move || {
            spawn_test_actor_with_event_observers(cmd_rx, actor_self_tx, upd_tx, event_observers);
        });
    }

    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let handle = ObservedProjectionCommandHandle::new(event_observers, sessions, cmd_tx.clone());

    let article_author = "5".repeat(64);
    // Mirrors `pointer_target_hydration.rs::target_shape`'s `EmbedTarget::Address`
    // arm exactly: `kinds = {kind}`, `addresses = {NaddrCoord}`, no `authors`.
    let shape = InterestShape {
        kinds: std::collections::BTreeSet::from([30_023]),
        addresses: std::collections::BTreeSet::from([NaddrCoord {
            pubkey: article_author.clone(),
            kind: 30_023,
            d_tag: "article-1".to_string(),
        }]),
        ..InterestShape::default()
    };

    let obs = CapturingObserver::new();
    let id = handle.open(ObservedProjection::from_shape(
        obs.clone(),
        "pointer-target-hydration",
        1,
        shape,
        80,
    ));
    assert!(id.0 != 0, "addressable-target interest must open");
    assert!(
        wait_barrier(&cmd_tx, BARRIER_TIMEOUT),
        "actor must settle the open"
    );

    // The demanded article event arrives LIVE (fetched via the relay round
    // trip in production; injected directly here) — kind matches, and it
    // carries its OWN `d` tag (not a self-referential `#a` tag, which no
    // addressable event ever carries).
    let mut article = raw_event(&"f1".repeat(32), &article_author, 30_023, 500);
    article.tags = vec![vec!["d".to_string(), "article-1".to_string()]];
    cmd_tx
        .send(ActorCommand::TestSupport(
            TestSupportCommand::IngestPreVerifiedEvents(vec![VerifiedEvent::from_raw_unchecked(
                article,
            )]),
        ))
        .expect("actor inbox open");
    assert!(wait_barrier(&cmd_tx, BARRIER_TIMEOUT), "actor settles ingest");

    assert_eq!(
        obs.count(),
        1,
        "a freshly-arriving event matching an addressable-target shape's kind \
         must reach its observer live — #3088"
    );

    handle.close(id);
    let _ = cmd_tx.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown));
}
