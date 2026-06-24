#![cfg(test)]
//! T117 core publish engine integration tests.
//!
//! Covers the four canonical bullets from the T117 spec:
//!   1. Successful multi-relay publish settles to `recent_ok`.
//!   2. AUTH-REQUIRED parks one relay; re-dispatches on Authenticated.
//!   3. Transient failure retries with 1s → 4s backoff, then gives up.
//!   4. Actor restart resumes pending row from a shared PublishStore.

use std::sync::Arc;

use crate::kernel::Kernel;
use crate::publish::{InMemoryPublishStore, PublishStore};
use crate::relay::DEFAULT_VISIBLE_LIMIT;

use super::{fake_signed, ok_payload, seed_kind10002, WRITE_R1, WRITE_R2};

#[test]
fn t117_successful_multi_relay_publish_lands_in_engine_recent_ok() {
    // Bullet 1: one publish → two NIP-65 write relays → both ack OK →
    // the engine's `recent_ok` snapshot carries both relays.
    let author = "22".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Seed author's kind:10002 so Nip65OutboxResolver has real write relays.
    // (T-publish-resolver-indexer: no fallback; without this seed the engine
    // would return NoTargets and emit 0 frames.)
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("11".repeat(32).as_str(), &author, 1, "hello t117");
    let outbound = kernel.run_publish_engine_at(
        &signed,
        &[],
        crate::publish::PublishTarget::Auto,
        None,
        1_000,
    );
    // Author has kind:10002 → resolver routes to declared write relays.
    let urls: std::collections::BTreeSet<String> =
        outbound.iter().map(|m| m.relay_url.clone()).collect();
    assert!(
        urls.contains(WRITE_R1),
        "WRITE_R1 must be a routing target; urls={urls:?}"
    );
    assert!(
        urls.contains(WRITE_R2),
        "WRITE_R2 must be a routing target; urls={urls:?}"
    );
    assert_eq!(outbound.len(), 2);

    // Per-relay state is now InFlight — feed OK acks in.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 1_010);
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 1_020);

    let snap = kernel.publish_status_snapshot();
    assert_eq!(
        snap.recent_ok.len(),
        1,
        "two OK acks coalesce into a single recent_ok entry"
    );
    assert_eq!(
        snap.recent_ok[0].accepted_by.len(),
        2,
        "both relays should appear under accepted_by"
    );
    assert!(
        snap.recent_errors.is_empty(),
        "no errors expected on the happy path"
    );
}

#[test]
fn t117_auth_required_on_one_relay_parks_until_authenticated_other_unaffected() {
    // Finding B: relay r1 returns OK-false `auth-required` on attempt 1. The
    // engine PARKS r1 — it does NOT burn a retry budget (the seconds-scale
    // challenge→sign→AUTH→OK round-trip never completes inside a fast retry
    // tick, so a budgeted retry would settle a false terminal failure). r1 is
    // demoted to durable Pending and marked unavailable for publish; a plain
    // tick must NOT re-dispatch it. Only when the kernel calls
    // `mark_publish_relay_available(r1)` — the effect of r1 reaching
    // `RelayAuthState::Authenticated` — does the parked publish re-dispatch and
    // succeed. r2 sees a clean OK on its original attempt and is untouched.
    let author = "44".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("33".repeat(32).as_str(), &author, 1, "auth-required test");
    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 2);

    // r1: AUTH-REQUIRED on attempt 1 → PARK. `on_ack` routes the park through
    // the availability gate (mark_relay_unavailable); it never schedules a
    // retry, so no frames flush here.
    let park_frames = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "auth-required: please AUTH"),
        100,
    );
    assert!(
        park_frames.is_empty(),
        "parking emits no retry frames — re-dispatch is event-driven off Authenticated"
    );

    // r2: clean OK on attempt 1 — settles Ok, untouched by r1's park.
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 110);

    // A plain retry tick must NOT re-dispatch the parked r1 (it is unavailable
    // until it authenticates). No frames queued.
    let tick_frames = kernel.tick_publish_engine(200);
    assert!(
        tick_frames.is_empty(),
        "a parked auth relay is not re-dispatched by a retry tick: {tick_frames:?}"
    );
    // The publish is still in flight (not terminally failed by an auth budget).
    let snap = kernel.publish_status_snapshot();
    assert!(
        snap.recent_errors.is_empty(),
        "parked publish has not failed: {:?}",
        snap.recent_errors
    );
    assert!(
        snap.recent_ok.is_empty(),
        "publish not complete yet — r1 still parked awaiting auth"
    );

    // r1 reaches `Authenticated` → the kernel re-opens the availability gate.
    // This is exactly what `handle_auth_ok` does on the Authenticated
    // transition; the parked publish re-dispatches r1 (one new frame).
    let redispatch = kernel.mark_publish_relay_available(WRITE_R1);
    let redispatch_urls: Vec<String> = redispatch.iter().map(|m| m.relay_url.clone()).collect();
    assert_eq!(
        redispatch_urls,
        vec![WRITE_R1.to_string()],
        "authenticated relay re-dispatches exactly the parked publish"
    );

    // Inject the OK for the re-dispatched attempt now that r1 is authenticated.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), 210);

    let snap = kernel.publish_status_snapshot();
    assert_eq!(
        snap.recent_ok.len(),
        1,
        "publish completes with one recent_ok row across both relays"
    );
    let accepted = &snap.recent_ok[0].accepted_by;
    assert_eq!(accepted.len(), 2);
    assert!(accepted.iter().any(|r| r == WRITE_R1));
    assert!(accepted.iter().any(|r| r == WRITE_R2));
    assert!(snap.recent_errors.is_empty(), "no terminal failures");
}

#[test]
fn t117_transient_failure_retries_with_1s_4s_backoff_then_gives_up() {
    // Bullet 3: r1 returns transient ("io") on every attempt. Default policy
    // is transient_max_retries = 3 (attempt 1, 2, 3). Backoffs:
    //   - after attempt 1 → 1_000 ms
    //   - after attempt 2 → 4_000 ms
    //   - after attempt 3 → give up (FailedAfterRetries).
    // We drive both NIP-65 write relays and assert just on r1,
    // asserting r2 settled as Ok separately.
    let author = "66".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed("55".repeat(32).as_str(), &author, 1, "transient test");
    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 2);

    // r2: settle immediately so the engine isn't tracking it any more.
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 10);

    // r1 attempt 1 → io failure → schedule retry at now + 1s.
    let _ = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "io: connection reset"),
        100,
    );

    // Tick at 1_500ms — past the 1s backoff (100 + 1_000 = 1_100). Engine
    // dispatches attempt 2.
    let retry2 = kernel.tick_publish_engine(1_500);
    assert_eq!(retry2.len(), 1);
    assert_eq!(retry2[0].relay_url, WRITE_R1);

    // r1 attempt 2 → io failure → schedule retry at now + 4s.
    let _ = kernel.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, false, "io: bad"), 1_600);

    // Tick at 6_000ms — past the 4s backoff (1_600 + 4_000 = 5_600). Engine
    // dispatches attempt 3.
    let retry3 = kernel.tick_publish_engine(6_000);
    assert_eq!(retry3.len(), 1);
    assert_eq!(retry3[0].relay_url, WRITE_R1);

    // r1 attempt 3 → io failure → engine gives up (FailedAfterRetries).
    let _ = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "io: still bad"),
        6_100,
    );
    // Tick once more to flush — the give-up settles inside on_ack already,
    // so this is belt-and-braces.
    let _ = kernel.tick_publish_engine(30_000);

    let snap = kernel.publish_status_snapshot();
    assert_eq!(
        snap.recent_errors.len(),
        1,
        "exactly one FailedAfterRetries row expected"
    );
    let failure = &snap.recent_errors[0];
    assert_eq!(failure.relay_url, WRITE_R1);
    assert!(
        failure.reason.contains("transient"),
        "give-up reason should be transient-flavoured: {}",
        failure.reason
    );
    // r2 settled cleanly.
    assert_eq!(snap.recent_ok.len(), 1);
    assert!(snap.recent_ok[0].accepted_by.iter().any(|r| r == WRITE_R2));
}

#[test]
fn t117_actor_restart_with_pending_resumes_from_pending_retries() {
    // Bullet 4: a publish dies mid-backoff in kernel A; a fresh kernel B
    // sharing the same PublishStore resumes the pending retry from the
    // store's `pending_retries` rows. Proves T54 durability still holds
    // through the engine-driven path.
    let publish_store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());

    let author = "88".repeat(32);
    let signed = fake_signed("77".repeat(32).as_str(), &author, 1, "restart test");

    // Kernel A: drive a transient failure so pending_retries gets populated.
    {
        let mut kernel_a =
            Kernel::with_publish_store(DEFAULT_VISIBLE_LIMIT, Arc::clone(&publish_store));
        seed_kind10002(&mut kernel_a, &author, &[WRITE_R1, WRITE_R2]);
        let outbound = kernel_a.run_publish_engine_at(
            &signed,
            &[],
            crate::publish::PublishTarget::Auto,
            None,
            0,
        );
        assert_eq!(outbound.len(), 2);
        // r2 settles OK; r1 transient → pending_retries[r1] = 0 + 1_000 = 1_000.
        let _ = kernel_a.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 10);
        let _ =
            kernel_a.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, false, "io: down"), 100);

        // The store now has one durable row with pending_retries on r1.
        let pending = publish_store.load_pending().unwrap();
        assert_eq!(pending.len(), 1, "row persisted in shared store");
        let retries = &pending[0].pending_retries;
        assert!(
            retries.iter().any(|(url, _)| url == WRITE_R1),
            "r1 retry deadline must be persisted: {:?}",
            retries
        );
        // Drop kernel_a — simulates process restart.
    }

    // Kernel B: same publish store, fresh engine. resume_publish_engine wires
    // through `PublishEngine::resume_from_store`, which restores
    // pending_retries. With now far in the future, the retry fires
    // immediately and we feed OK to settle it.
    let mut kernel_b =
        Kernel::with_publish_store(DEFAULT_VISIBLE_LIMIT, Arc::clone(&publish_store));
    let resumed = kernel_b.resume_publish_engine();
    // `resume_publish_engine` uses the kernel wall-clock seam; the
    // persisted deadline (1_000 ms epoch) is in the deep past so the retry
    // dispatches immediately.
    assert_eq!(
        resumed.len(),
        1,
        "resume must dispatch the pending r1 retry"
    );
    assert_eq!(resumed[0].relay_url, WRITE_R1);

    // Ack the retry — wall-clock so we don't accidentally trip the engine's
    // late-ack idempotence path with a past timestamp.
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let _ = kernel_b.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, true, ""), now);

    let snap = kernel_b.publish_status_snapshot();
    assert_eq!(
        snap.recent_ok.len(),
        1,
        "resumed retry succeeded on the new kernel"
    );
    assert!(
        publish_store.load_pending().unwrap().is_empty(),
        "store cleared after the resumed publish completed"
    );
}
