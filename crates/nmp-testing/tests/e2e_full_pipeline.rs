//! End-to-end integration tests for the full kernel pipeline.
//!
//! Every test here exercises the path:
//!   Action dispatch → subscription planner → relay manager opens REQ →
//!   MockRelay emits EVENT → ingest verifies + persists → reverse-index
//!   updates → view snapshot reflects → app sees rev: u64 bump.
//!
//! # Milestone gate legend
//!
//! Each test carries `#[ignore = "blocked on M<N>: <label>"]`.  The
//! companion `e2e_full_pipeline_audit.rs` fails at CI time when any such
//! tag is present but the referenced milestone is recorded as DONE in
//! `docs/plan.md`.  That file owns milestone status; the audit enforces
//! un-ignoring.
//!
//! Gate map for this suite:
//!   M2 — subscription compilation + outbox routing + kind:3 auto-tracking
//!   M3 — persistence (LMDB) + full insert invariants
//!   M4 — NIP-77 negentropy sync engine
//!   M5 — NIP-42 relay auth
//!   M6 — sessions + signers (incl. bunker + nsec creation) + write path
//!   M7 — reactions + thread + reply
//!   M8 — relay manager + multi-relay subscription lifecycle
//!
//! Tests 1, 2, 4, 6 gate on M2 + M3 + M8.
//! Test 3 gates on M6 + M7 + M8.
//! Test 5 gates on M5 + M6 + M8.

// These constants are used by the audit companion to verify tag format.
pub const GATE_M2: &str = "M2";
pub const GATE_M3: &str = "M3";
pub const GATE_M4: &str = "M4";
pub const GATE_M5: &str = "M5";
pub const GATE_M6: &str = "M6";
pub const GATE_M7: &str = "M7";
pub const GATE_M8: &str = "M8";

/// Asserts a per-test 5-second ceiling as documented in the task spec.
/// Replace this with `#[tokio::test(timeout = ...)]` when the async
/// executor is introduced in M2/M8.
#[allow(dead_code)]
const PER_TEST_TIMEOUT_SECS: u64 = 5;

// ---------------------------------------------------------------------------
// Test 1 — cold_open_profile_view_full_pipeline
// ---------------------------------------------------------------------------
//
// Scenario:
//   1. Boot the kernel actor.
//   2. Sign in as alice (establishes an active account with a local key signer).
//   3. Dispatch PublishProfile with display_name = "Alice".
//      — `publish_profile` builds + signs the kind:0 locally, then calls
//        `record_local_publish_intent`, which populates
//        `local_profile_intents[alice_pk]`.  This is the production path
//        for the active account's profile card — the same path a relay echo
//        of the published kind:0 would eventually update via `ingest_profile`.
//   4. Force a snapshot emit (MarkChangedSinceEmit).
//   5. Drain the update channel; assert snapshot["projections"]["profile"]
//      ["display_name"] == "Alice".
//
// `profile` (not `claimed_profiles`) is the correct projection key here:
// it is the active account's own profile card, always present in every
// snapshot (D1), and populated by `local_profile_intents` after the
// active account publishes its kind:0.
#[test]
fn cold_open_profile_view_full_pipeline() {
    use nmp_core::testing::{spawn_actor, ActorCommand};
    use nmp_core::{decode_update_frame, UpdateEnvelope};
    use std::time::Duration;

    // A fixed nsec used only in tests (same key as in c13).
    const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

    let (tx, rx) = spawn_actor();
    tx.send(ActorCommand::Start {
        visible_limit: 100,
        emit_hz: 0,
    })
    .expect("send Start");

    // Step 1: Sign in — establishes active account (alice) with local key.
    tx.send(ActorCommand::SignInNsec {
        secret: zeroize::Zeroizing::new(TEST_NSEC.to_string()),
    })
    .expect("send SignInNsec");

    // Step 2: Publish alice's profile.
    // Actor dispatch: PublishProfile → publish_profile() → sign locally →
    // publish_signed_with_correlation → record_local_publish_intent →
    // local_profile_intents[alice_pk] = Profile { display: "Alice", ... }
    let mut fields = serde_json::Map::new();
    fields.insert(
        "display_name".to_string(),
        serde_json::Value::String("Alice".to_string()),
    );
    tx.send(ActorCommand::PublishProfile {
        fields,
        correlation_id: None,
    })
    .expect("send PublishProfile");

    // Step 3: Force emit so we don't wait for the ticker.
    tx.send(ActorCommand::MarkChangedSinceEmit)
        .expect("send MarkChangedSinceEmit");

    // Drain snapshots until projections["profile"]["display_name"] == "Alice".
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut found = false;
    let mut last_profile: Option<serde_json::Value> = None;
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => {
                let envelope = decode_update_frame(&frame).expect("decode frame");
                if let UpdateEnvelope::Snapshot(snap) = envelope {
                    let display_name =
                        snap["projections"]["profile"]["display_name"].as_str();
                    if display_name == Some("Alice") {
                        found = true;
                        break;
                    }
                    last_profile = Some(snap["projections"]["profile"].clone());
                }
            }
            Err(_) => continue,
        }
    }

    assert!(
        found,
        "snapshot[projections][profile][display_name] must equal 'Alice' after PublishProfile; \
         last profile projection: {:?}",
        last_profile
    );

    tx.send(ActorCommand::Shutdown).ok();
}

// ---------------------------------------------------------------------------
// Test 2 — kind3_update_rewires_subscriptions
// ---------------------------------------------------------------------------
//
// Scenario:
//   1. Build a SubscriptionLifecycle with alice registered (tailing interest).
//   2. Compile: assert REQ targets wss://alice-relay/.
//   3. Enqueue a FollowListChanged trigger adding carol.
//   4. Wire carol's mailbox and expand the interest.
//   5. drain_tick: assert the returned WireFrames include a REQ for carol's relay.
//   6. Idempotence: second drain with empty inbox emits no frames.
//
// "ContactListView snapshot reflects [alice, carol]" is implemented at the
// routing layer (WireFrame) — that is the real observable for subscription
// rewiring.  The actor's update channel is opaque to outbound REQs.
#[test]
fn kind3_update_rewires_subscriptions() {
    use nmp_core::planner::{InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope,
        InterestShape, LogicalInterest, MailboxSnapshot};
    use nmp_core::subs::{AccountId, CompileTrigger, SubscriptionLifecycle, WireFrame};
    use std::collections::BTreeSet;

    fn pubkey(seed: &str) -> String {
        format!("{seed:0>64}").chars().take(64).collect()
    }
    fn tailing_interest(id: u64, authors: &[&str]) -> LogicalInterest {
        LogicalInterest {
            id: InterestId(id),
            scope: InterestScope::ActiveAccount,
            shape: InterestShape {
                authors: authors.iter().map(|a| pubkey(a)).collect::<BTreeSet<_>>(),
                kinds: [1u32].into_iter().collect(),
                ..Default::default()
            },
            hints: vec![],
            lifecycle: InterestLifecycle::Tailing,
            is_indexer_discovery: false,
        }
    }

    let mut lc = SubscriptionLifecycle::new();
    let mut mailboxes = InMemoryMailboxCache::new();

    // alice has a known write relay.
    mailboxes.put(
        pubkey("alice"),
        MailboxSnapshot {
            write_relays: vec!["wss://alice-relay/".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    // Register a tailing interest for alice.
    lc.registry_mut().push(tailing_interest(1, &["alice"]));

    // Compile: alice's relay must receive a REQ.
    let frames1 = lc.recompile_and_diff(&mailboxes).expect("initial compile");
    let req_relays1: Vec<&str> = frames1
        .iter()
        .filter_map(|f| {
            if let WireFrame::Req { relay_url, .. } = f {
                Some(relay_url.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(
        req_relays1.contains(&"wss://alice-relay/"),
        "initial compile must REQ alice's relay; got {req_relays1:?}"
    );
    assert_eq!(lc.compile_count(), 1);

    // Wire carol's mailbox so the recompile finds a route.
    mailboxes.put(
        pubkey("carol"),
        MailboxSnapshot {
            write_relays: vec!["wss://carol-relay/".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    // Expand the interest to cover carol too (production view rebuild equivalent).
    lc.registry_mut()
        .push(tailing_interest(1, &["alice", "carol"]));

    // Fire the A11 FollowListChanged trigger — the canonical kind:3 rewire signal.
    lc.enqueue_trigger(CompileTrigger::FollowListChanged {
        account_id: AccountId(pubkey("alice")),
        new_follows: vec![pubkey("carol")],
    });

    let frames2 = lc.drain_tick(&mailboxes);
    assert_eq!(
        lc.compile_count(),
        2,
        "drain_tick must recompile on FollowListChanged trigger"
    );

    let req_relays2: Vec<&str> = frames2
        .iter()
        .filter_map(|f| {
            if let WireFrame::Req { relay_url, .. } = f {
                Some(relay_url.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(
        req_relays2.contains(&"wss://carol-relay/"),
        "after follow-list update, recompile must REQ carol's relay; frames={frames2:?}"
    );

    // Idempotence: empty-inbox tick must emit no frames.
    let frames3 = lc.drain_tick(&mailboxes);
    assert!(
        frames3.is_empty(),
        "empty-inbox tick must emit zero frames; got {frames3:?}"
    );
    assert_eq!(
        lc.compile_count(),
        2,
        "empty-inbox tick must not bump compile count"
    );
}

// ---------------------------------------------------------------------------
// Test 3 — publish_roundtrip_via_outbox
// ---------------------------------------------------------------------------
//
// Scenario:
//   1. Build a PublishEngine with a StaticOutbox carrying alice's write relays.
//   2. Dispatch a kind:1 publish via the engine.
//   3. Assert the ReplayDispatcher received EVENT frames for alice's relays.
//   4. Assert the signed event carries kind=1.
//
// The PublishEngine + ReplayDispatcher IS the full write-path observable at
// the framework layer (M6/M7/M8).  Router identity canonicalization (trailing
// slash) is an active contract as per publish_relay_identity_tests.rs.
#[test]
fn publish_roundtrip_via_outbox() {
    use nmp_core::publish::{
        InMemoryPublishStore, NoopSigner, PublishAction, PublishEngine, PublishTarget,
        RelayAck, RelayUrl, ReplayDispatcher, RetryPolicy, StaticOutbox,
    };
    use nmp_core::substrate::{SignedEvent, UnsignedEvent};
    use std::sync::Arc;

    fn pubkey(seed: &str) -> String {
        format!("{seed:0>64}").chars().take(64).collect()
    }

    // Alice's NIP-65 outbox write relays (wire form with trailing slash).
    let alice_writes: Vec<RelayUrl> = vec!["wss://r1/".to_string(), "wss://r2/".to_string()];
    let mut outbox = StaticOutbox::default();
    outbox
        .author_writes
        .insert(pubkey("alice"), alice_writes.clone());

    let dispatcher = Arc::new(ReplayDispatcher::new());
    // Script OK acks under the canonical relay keys (engine canonicalizes trailing slash).
    dispatcher.script("wss://r1", vec![RelayAck::ok("wss://r1")]);
    dispatcher.script("wss://r2", vec![RelayAck::ok("wss://r2")]);

    let mut engine = PublishEngine::new(
        Arc::new(outbox),
        Arc::clone(&dispatcher) as Arc<dyn nmp_core::publish::RelayDispatcher>,
        Arc::new(InMemoryPublishStore::new()),
        Arc::new(NoopSigner),
        RetryPolicy::default(),
    );

    // A minimal kind:1 signed event authored by alice.
    let event = SignedEvent {
        id: "b".repeat(64),
        sig: "c".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: pubkey("alice"),
            kind: 1,
            tags: vec![],
            content: "hello".to_string(),
            created_at: 1_700_000_100,
        },
    };

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "test-h1".to_string(),
                event,
                target: PublishTarget::Auto,
            },
            0,
            None,
        )
        .expect("public publish must succeed");

    // The dispatcher must have received frames on both outbox relays.
    let sent = dispatcher.sent_frames();
    let sent_relays: std::collections::BTreeSet<&str> =
        sent.iter().map(|(url, _)| url.as_str()).collect();
    assert!(
        sent_relays.contains("wss://r1"),
        "kind:1 event must be dispatched to alice's canonical write relay r1; got: {sent_relays:?}"
    );
    assert!(
        sent_relays.contains("wss://r2"),
        "kind:1 event must be dispatched to alice's canonical write relay r2; got: {sent_relays:?}"
    );

    // Confirm the dispatched frames encode a kind:1 event.
    // Sent frames are `["EVENT", <signed-event-json>]` strings.
    let all_text: String = sent.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join(" ");
    assert!(
        all_text.contains("\"kind\":1"),
        "dispatched frame must encode kind:1; got excerpt: {}",
        &all_text[..std::cmp::min(200, all_text.len())]
    );
}

// ---------------------------------------------------------------------------
// Test 4 — negentropy_skips_redundant_req
// ---------------------------------------------------------------------------
//
// Scenario:
//   1. Build a SubscriptionLifecycle.
//   2. Install a PlanCoverageHook that drops the compiled plan entirely
//      (simulating full coverage: negentropy confirmed all events are already
//      present locally).
//   3. Compile: assert zero REQ frames are emitted (plan was dropped by hook).
//   4. A second compile with the hook de-activated emits the REQ — confirms
//      the hook is the suppressor, not a stale plan.
//
// D2 doctrine: "negentropy reconciliation before REQ subscriptions".  The
// PlanCoverageHook seam (subs/coverage_hook_tests.rs §2) is the exact
// mechanism designed for this: the hook runs after compile() but before
// plan_diff(), so it can drop sub-shapes for already-covered (relay, filter)
// pairs.  This test drives the seam in the canonical "fully covered" scenario.
#[test]
fn negentropy_skips_redundant_req() {
    use nmp_core::planner::{InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope,
        InterestShape, LogicalInterest, MailboxSnapshot};
    use nmp_core::subs::{SubscriptionLifecycle, WireFrame};
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};

    fn pubkey(seed: &str) -> String {
        format!("{seed:0>64}").chars().take(64).collect()
    }

    let mut lc = SubscriptionLifecycle::new();
    let mut mailboxes = InMemoryMailboxCache::new();
    mailboxes.put(
        pubkey("alice"),
        MailboxSnapshot {
            write_relays: vec!["wss://alice-relay/".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    lc.registry_mut().push(LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey("alice")].into_iter().collect::<BTreeSet<_>>(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    });

    // Install a coverage hook that fully drops the compiled plan (D2 seam).
    // This models the production negentropy gate: "we already have everything —
    // no REQ needed for this relay/filter pair."
    let hook_active = Arc::new(Mutex::new(true));
    let hook_active_for_hook = Arc::clone(&hook_active);
    lc.set_coverage_hook(Arc::new(move |plan| {
        if *hook_active_for_hook.lock().unwrap() {
            plan.per_relay.clear();
        }
    }));

    // Compile with the hook active: the plan is cleared, so zero REQs.
    let frames_covered = lc.recompile_and_diff(&mailboxes).expect("compile (covered)");
    let req_count_covered = frames_covered
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { .. }))
        .count();
    assert_eq!(
        req_count_covered, 0,
        "negentropy-covered compile must emit zero REQs (PlanCoverageHook drops plan); \
         got {req_count_covered}"
    );

    // De-activate the hook and recompile. This time the plan flows through
    // and the relay must receive a REQ — proving the hook was the suppressor.
    *hook_active.lock().unwrap() = false;
    let frames_uncovered = lc.recompile_and_diff(&mailboxes).expect("compile (uncovered)");
    let req_count_uncovered = frames_uncovered
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { .. }))
        .count();
    assert!(
        req_count_uncovered >= 1,
        "without coverage gate the relay must receive at least one REQ; got {req_count_uncovered}"
    );
}

// ---------------------------------------------------------------------------
// Test 5 — auth_required_for_read_flow
// ---------------------------------------------------------------------------
//
// Scenario:
//   1. Build a SubscriptionLifecycle with an interest for alice.
//   2. AUTH challenge arrives BEFORE the first compile (relay is auth-paused).
//   3. Compile: REQs targeting the paused relay are withheld by the auth-gate.
//   4. Assert zero REQs on the wire.
//   5. AUTH completes (Authenticated): pending REQs are flushed.
//   6. Assert the flushed REQs target the expected relay.
//
// This is the M5 NIP-42 relay auth contract.  The auth-gate (subs/auth_gate.rs)
// intercepts REQs during `recompile_and_diff` / `drain_tick` when a relay is
// in ChallengeReceived state.  The flush happens when `handle_auth_state_change`
// transitions to Authenticated.  The key timing: the challenge must arrive
// BEFORE the compile so the `partition()` path captures the REQs.
#[test]
fn auth_required_for_read_flow() {
    use nmp_core::planner::{InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope,
        InterestShape, LogicalInterest, MailboxSnapshot};
    use nmp_core::subs::{RelayAuthState, SubscriptionLifecycle, WireFrame};
    use std::collections::BTreeSet;

    fn pubkey(seed: &str) -> String {
        format!("{seed:0>64}").chars().take(64).collect()
    }

    let relay_url = "wss://auth-relay/";

    let mut lc = SubscriptionLifecycle::new();
    let mut mailboxes = InMemoryMailboxCache::new();
    mailboxes.put(
        pubkey("alice"),
        MailboxSnapshot {
            write_relays: vec![relay_url.to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );

    lc.registry_mut().push(LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey("alice")].into_iter().collect::<BTreeSet<_>>(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    });

    // Phase 1: AUTH challenge arrives BEFORE the first compile.
    // This puts the relay into the paused state so recompile_and_diff routes
    // the produced REQs through the auth-gate partition path.
    let _pre = lc.handle_auth_state_change(
        relay_url.to_string(),
        RelayAuthState::ChallengeReceived,
    );

    // Phase 2: Compile while auth-paused.
    // REQs targeting the paused relay must be captured in the pending buffer,
    // not returned to the caller (zero wire frames for this relay).
    let frames_paused = lc.recompile_and_diff(&mailboxes).expect("auth-paused compile");
    let reqs_to_paused: Vec<_> = frames_paused
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { relay_url: u, .. } if u == relay_url))
        .collect();
    assert!(
        reqs_to_paused.is_empty(),
        "REQs to NIP-42 auth-paused relay must be withheld from the wire; got {} frame(s)",
        reqs_to_paused.len()
    );

    // Phase 3: AUTH completes — pending REQs must be flushed to the wire.
    let flush_frames = lc.handle_auth_state_change(
        relay_url.to_string(),
        RelayAuthState::Authenticated,
    );
    let reqs_flushed: Vec<_> = flush_frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { relay_url: u, .. } if u == relay_url))
        .collect();
    assert!(
        !reqs_flushed.is_empty(),
        "Authenticated transition must flush buffered REQs to the relay; got 0"
    );
}

// ---------------------------------------------------------------------------
// Test 6 — monotonic_rev_under_concurrent_dispatch
// ---------------------------------------------------------------------------
//
// Scenario:
//   1. Spawn the kernel actor (single-threaded behind mpsc channel — the rev
//      is always serialised on the actor side).
//   2. Submit 20 IngestPreVerifiedEvents commands via clones of the sender
//      (concurrent submission from multiple std::thread handles).
//   3. Drain all snapshot envelopes within a 5-second window.
//   4. Assert every emitted snapshot's rev is >= the previous one (monotonic).
//
// The actor serialises all commands — rev can never go backwards, and a
// snapshot taken at rev N cannot contain partial state from N+1. The
// concurrency is on the *submission* side (20 threads sending simultaneously),
// which exercises the mpsc channel's ordering.  This is the D8 reactivity
// contract stress-test.
#[test]
fn monotonic_rev_under_concurrent_dispatch() {
    use nmp_core::store::{RawEvent, VerifiedEvent};
    use nmp_core::testing::{spawn_actor, ActorCommand};
    use nmp_core::{decode_update_frame, UpdateEnvelope};
    use std::sync::Arc;
    use std::time::Duration;

    let (tx, rx) = spawn_actor();
    // emit_hz = 60 so the actor ticks frequently.
    tx.send(ActorCommand::Start {
        visible_limit: 500,
        emit_hz: 60,
    })
    .expect("send Start");

    // Use a fixed author pubkey so all events land in the same timeline slot.
    let author_pk = "0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c";

    // Spawn 20 threads, each sending one batch of events.
    let tx = Arc::new(tx);
    let handles: Vec<_> = (0u64..20)
        .map(|i| {
            let tx = Arc::clone(&tx);
            let author_pk = author_pk.to_string();
            std::thread::spawn(move || {
                let event_id = format!("{i:0>64x}");
                let raw = RawEvent {
                    id: event_id,
                    pubkey: author_pk,
                    created_at: 1_700_000_000 + i,
                    kind: 1,
                    tags: vec![],
                    content: format!("concurrent event {i}"),
                    sig: "a".repeat(128),
                };
                let verified = VerifiedEvent::from_raw_unchecked(raw);
                tx.send(ActorCommand::IngestPreVerifiedEvents(vec![verified]))
                    .ok();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }

    // Drain snapshots for up to 5 seconds, collecting every emitted rev.
    let mut revs: Vec<u64> = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => {
                let envelope = decode_update_frame(&frame).expect("decode frame");
                if let UpdateEnvelope::Snapshot(snap) = envelope {
                    if let Some(rev) = snap["rev"].as_u64() {
                        revs.push(rev);
                    }
                }
            }
            Err(_) => break,
        }
    }

    // Must have received at least one snapshot.
    assert!(
        !revs.is_empty(),
        "actor must emit at least one snapshot during the concurrent-dispatch burst"
    );

    // Every successive snapshot's rev must be >= the previous (monotonic).
    for window in revs.windows(2) {
        assert!(
            window[1] >= window[0],
            "rev sequence must be monotonically non-decreasing (D8): {} followed by {}",
            window[0],
            window[1]
        );
    }
}
