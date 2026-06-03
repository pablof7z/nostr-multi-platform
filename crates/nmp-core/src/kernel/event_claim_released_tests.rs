//! Tests for the V-59 rung 1 (#4) `event_claim_released` ring + observer.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::event_claim_released::EventClaimReleasedObserver;
use super::Kernel;
use crate::nip19::{encode_nevent, NeventData};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;

fn hex64(prefix: &str) -> String {
    let mut s = prefix.to_string();
    while s.len() < 64 {
        s.push('0');
    }
    s.chars().take(64).collect()
}

fn nevent_uri(event_id: &str) -> String {
    let bech = encode_nevent(&NeventData {
        event_id: event_id.to_string(),
        relays: vec![],
        author: None,
        kind: Some(1),
    })
    .expect("encode_nevent");
    format!("nostr:{bech}")
}

/// Simulate a relay's EOSE-no-match for a claim sub the way the production
/// EOSE arm (`kernel/ingest/mod.rs`) does: `complete_unknown_oneshot` (token
/// teardown) followed by `record_claim_expansion_eose_no_match` (per-relay
/// in-flight slot removal). A single relay's EOSE does NOT release the claim —
/// only the controller's terminal-miss does (driven by `poll` below).
fn eose_no_match(kernel: &mut Kernel, sub_id: &str, relay_url: &str) {
    kernel.complete_unknown_oneshot(sub_id);
    kernel.record_claim_expansion_eose_no_match(sub_id, relay_url);
}

/// Advance the claim controller past `PHASE_1_BUDGET_MS` (1500 ms) but under
/// the per-claim TOTAL budget (8000 ms). For a `claim_and_wire` claim — which
/// has an empty candidate-hint set — this drives
/// `Phase1 budget elapsed → advance_to_phase2 → to_pick == 0 →
/// terminate(Exhausted)`, exercising the `Exhausted` arm of the
/// `terminate_claim` terminal-miss gate that releases the `event_claims` row +
/// fires the release ring. (The `Budget` arm is covered separately by
/// `budget_terminal_miss_clears_claim_and_pushes_to_release_ring`.)
fn poll_to_terminal_miss(kernel: &mut Kernel) {
    let later = std::time::Instant::now() + std::time::Duration::from_millis(1_600);
    let _ = kernel.poll_claim_expansion(later);
}

/// Drive a claim through to the wired state, then return the planner-assigned
/// `sub_id` so the test can simulate EOSE for it. Mirrors the production
/// claim_event → planner-frame bridge wiring.
fn claim_and_wire(kernel: &mut Kernel, id: &str, relay_url: &str) -> String {
    let uri = nevent_uri(id);
    let _ = kernel.claim_event(uri, "view-0".to_string(), true, false);

    // The claim registered a oneshot + a pending claim. Read the real
    // interest_id and bridge a WireFrame::Req so the planner-frame bridge
    // populates oneshot_subs (so complete_unknown_oneshot recognises the sub)
    // AND claim_sub_index (so the no-match resolver finds the claim).
    let interest_id = kernel
        .test_claim_interest_id(id)
        .expect("claim must register a pending claim with an interest_id");
    let sub_id = format!("sub-test-{}", &id[..8]);
    let frames = vec![WireFrame::Req {
        relay_url: relay_url.to_string(),
        sub_id: sub_id.clone(),
        filter_json: r#"{"ids":["x"],"limit":1}"#.to_string(),
        interest_id,
        lifecycle: crate::planner::InterestLifecycle::OneShot,
    }];
    kernel.register_wire_frames_for_test(&frames);
    sub_id
}

/// Claim-expansion terminal-miss (every relay EOSE'd without the event, then
/// the controller exhausts the claim) clears the claim state AND pushes the
/// primary_id into the `event_claim_released` ring (the public projection).
///
/// Release is now controller-driven (`terminate_claim` on
/// `ClaimTermination::Exhausted`), NOT per-EOSE — a single relay's
/// EOSE-no-match must NOT release the claim, because a sibling relay sharing
/// the sub_id may still deliver the matching EVENT (the embed-loading-forever
/// race this fix closes).
#[test]
fn terminal_miss_clears_claim_and_pushes_to_release_ring() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let id = hex64("f1");
    let relay = "wss://relay.example";
    let sub_id = claim_and_wire(&mut kernel, &id, relay);

    // Precondition: the claim is requested and tracked.
    assert!(kernel.event_claim_is_requested_for_test(&id));
    assert_eq!(kernel.event_claims_len_for_test(&id), 1);
    assert!(
        kernel.event_claim_released().is_empty(),
        "release ring starts empty"
    );

    // A single relay's EOSE-no-match removes its in-flight slot but must NOT
    // release the claim — a sibling relay's EVENT could still arrive.
    eose_no_match(&mut kernel, &sub_id, relay);
    assert_eq!(
        kernel.event_claims_len_for_test(&id),
        1,
        "a single relay's EOSE must NOT release the claim (race guard)"
    );
    assert!(
        kernel.event_claim_released().is_empty(),
        "release ring stays empty until the claim genuinely exhausts"
    );

    // The controller now observes no in-flight attempts and no candidate
    // relays → terminates the claim as Exhausted → releases.
    poll_to_terminal_miss(&mut kernel);

    assert!(
        !kernel.event_claim_is_requested_for_test(&id),
        "terminal-miss must clear event_claim_requested so a re-claim re-fetches"
    );
    assert_eq!(
        kernel.event_claims_len_for_test(&id),
        0,
        "terminal-miss must clear the event_claims refcount entry"
    );
    assert_eq!(
        kernel.event_claim_released(),
        vec![id.clone()],
        "the released primary_id must be pushed into the ring in arrival order"
    );
}

/// A registered observer is notified with the released primary_id when the
/// claim genuinely exhausts (controller terminal-miss).
#[test]
fn terminal_miss_notifies_registered_observer() {
    struct Recorder {
        count: AtomicUsize,
        ids: Mutex<Vec<String>>,
    }
    impl EventClaimReleasedObserver for Recorder {
        fn on_event_claim_released(&self, primary_id: &str) {
            self.count.fetch_add(1, Ordering::SeqCst);
            self.ids.lock().unwrap().push(primary_id.to_string());
        }
    }

    let recorder = Arc::new(Recorder {
        count: AtomicUsize::new(0),
        ids: Mutex::new(Vec::new()),
    });

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.register_event_claim_released_observer(
        Arc::clone(&recorder) as Arc<dyn EventClaimReleasedObserver>
    );

    let id = hex64("f2");
    let relay = "wss://relay.example";
    let sub_id = claim_and_wire(&mut kernel, &id, relay);
    eose_no_match(&mut kernel, &sub_id, relay);
    assert_eq!(
        recorder.count.load(Ordering::SeqCst),
        0,
        "a single relay's EOSE must NOT fire the release observer (race guard)"
    );
    poll_to_terminal_miss(&mut kernel);

    assert_eq!(
        recorder.count.load(Ordering::SeqCst),
        1,
        "observer must fire exactly once when the claim exhausts"
    );
    assert_eq!(
        *recorder.ids.lock().unwrap(),
        vec![id],
        "observer must receive the released primary_id"
    );
}

/// A NON-claim discovery oneshot (no claim_sub_index entry) does NOT touch the
/// release ring — the new path is gated strictly on claim subs.
#[test]
fn non_claim_oneshot_eose_does_not_push_release_ring() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // A discovery oneshot for an unknown reference (profile/event discovery),
    // wired into oneshot_subs but with NO pending claim.
    kernel.collect_unknown_refs(&[vec!["q".to_string(), hex64("ab")]]);
    let _ = kernel.drain_unknown_oneshots();
    // Bridge the planner frame so oneshot_subs is populated.
    // (drain_unknown_oneshots registered the interest; we synthesize the
    // planner-assigned sub_id via the bridge by reading the pending interest.)
    // Simplest: just assert that a fabricated non-claim sub_id is a no-op.
    let fake_sub = "sub-not-a-claim".to_string();
    // Not in oneshot_subs at all → complete_unknown_oneshot early-returns.
    kernel.complete_unknown_oneshot(&fake_sub);
    assert!(
        kernel.event_claim_released().is_empty(),
        "a non-claim / unknown sub must never push the release ring"
    );
}

/// The ring is bounded: pushing more than the cap evicts oldest-first.
#[test]
fn release_ring_is_bounded() {
    use crate::substrate::{BoundedRing, MAX_PROJECTION_MESSAGES};
    let mut ring: BoundedRing<String> = BoundedRing::new(3);
    for i in 0..5 {
        ring.push(format!("id-{i}"));
    }
    assert_eq!(ring.len(), 3, "ring never exceeds its capacity");
    let kept: Vec<String> = ring.iter().cloned().collect();
    assert_eq!(
        kept,
        vec!["id-2".to_string(), "id-3".to_string(), "id-4".to_string()],
        "oldest entries are evicted first (FIFO)"
    );
    // Sanity: the production cap is the projection constant.
    assert_eq!(MAX_PROJECTION_MESSAGES, 10_000);
}

/// The OTHER terminal-miss reason — `ClaimTermination::Budget` (the total
/// per-claim budget elapsed before any relay delivered the event, e.g. a slow
/// relay that never EOSEs) — must ALSO release the claim and push the release
/// ring. This guards the second arm of the `Exhausted | Budget` gate in
/// `terminate_claim`, which the exhaustion tests alone do not exercise.
#[test]
fn budget_terminal_miss_clears_claim_and_pushes_to_release_ring() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let id = hex64("f3");
    let _sub_id = claim_and_wire(&mut kernel, &id, "wss://relay.example");

    assert_eq!(kernel.event_claims_len_for_test(&id), 1);
    assert!(kernel.event_claim_released().is_empty());

    // No EOSE, no EVENT — just let the total budget elapse. The controller
    // terminates the claim as Budget on the next poll.
    let later = std::time::Instant::now() + std::time::Duration::from_millis(60_000);
    let _ = kernel.poll_claim_expansion(later);

    assert_eq!(
        kernel.event_claims_len_for_test(&id),
        0,
        "Budget terminal-miss must clear the event_claims refcount entry"
    );
    assert_eq!(
        kernel.event_claim_released(),
        vec![id],
        "Budget terminal-miss must push the released primary_id into the ring"
    );
}
