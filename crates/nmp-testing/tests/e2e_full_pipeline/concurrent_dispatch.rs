//! Test 6 — monotonic_rev_under_concurrent_dispatch
//!
//! Scenario:
//!   1. Spawn the kernel actor (single-threaded behind mpsc channel — the rev
//!      is always serialised on the actor side).
//!   2. Submit 20 IngestPreVerifiedEvents commands via clones of the sender
//!      (concurrent submission from multiple std::thread handles).
//!   3. Drain all snapshot envelopes within a 5-second window.
//!   4. Assert every emitted snapshot's rev is >= the previous one (monotonic).
//!
//! The actor serialises all commands — rev can never go backwards, and a
//! snapshot taken at rev N cannot contain partial state from N+1. The
//! concurrency is on the *submission* side (20 threads sending simultaneously),
//! which exercises the mpsc channel's ordering.  This is the D8 reactivity
//! contract stress-test.

use nmp_core::actor::{LifecycleCommand, TestSupportCommand};

#[test]
fn monotonic_rev_under_concurrent_dispatch() {
    use nmp_core::testing::{spawn_actor, ActorCommand};
    use nmp_core::{decode_update_frame, UpdateEnvelope};
    use nmp_store::{RawEvent, VerifiedEvent};
    use std::sync::Arc;
    use std::time::Duration;
    // PR-B: `UpdateEnvelope::Snapshot` carries the typed `SnapshotEnvelope`
    // (rev is a typed Tier-3 field, no JSON indexing).

    let (tx, rx) = spawn_actor();
    // emit_hz = 60 so the actor ticks frequently.
    tx.send(ActorCommand::Lifecycle(LifecycleCommand::Start {
        visible_limit: 500,
        emit_hz: 60,
        initial_relays: Vec::new(),
    }))
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
                tx.send(ActorCommand::TestSupport(
                    TestSupportCommand::IngestPreVerifiedEvents(vec![verified]),
                ))
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
                    revs.push(snap.rev);
                }
            }
            Err(_) => break,
        }
    }

    // Must have observed at least 2 snapshots so the windows(2) check is
    // meaningful (a single snapshot cannot demonstrate monotonic progression).
    assert!(
        revs.len() >= 2,
        "actor must emit at least 2 snapshots during the 20-command burst \
         so the monotonic check is non-vacuous; got {} snapshots",
        revs.len()
    );

    // The final rev must strictly exceed the first — confirms the actor
    // actually processed commands and bumped the revision counter.
    assert!(
        revs.last() > revs.first(),
        "rev must advance over the burst: first={:?}, last={:?}",
        revs.first(),
        revs.last()
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
