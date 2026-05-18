//! T117 integration tests — kernel publish path goes through `PublishEngine`.
//!
//! These tests drive the kernel's engine seam directly:
//! - The engine's `Nip65OutboxResolver` resolves relays from the kernel's
//!   event store (no kind:10002 yet → `DEFAULT_INDEXER_FALLBACK` per D3).
//! - The engine pushes per-relay `EVENT` frames into the `QueueDispatcher`,
//!   which the kernel drains into `OutboundMessage`s.
//! - OK frames are folded back via `Kernel::handle_publish_ok_at` (the
//!   time-injected variant; the wire path calls `handle_publish_ok` which
//!   reads `SystemTime::now()`).
//! - Retries fire on `tick_publish_engine(now_ms)`.
//!
//! Time is injected throughout (`now_ms` deterministic), no sockets, no
//! sleeps. The four bullets the spec calls out:
//! 1. Successful multi-relay publish: engine settles each per-relay to Ok →
//!    snapshot `recent_ok` carries the relay set.
//! 2. AUTH-REQUIRED on one relay, OK on the other: reauth path schedules a
//!    retry (engine fires it on tick), settles on the second attempt;
//!    untouched relay stays Ok.
//! 3. Transient failure × 3: 1s backoff → 4s backoff → give-up;
//!    `FailedAfterRetries` row appears on the snapshot.
//! 4. Restart with a Pending row: build a second Kernel sharing the same
//!    `Arc<dyn PublishStore>`; engine resumes via `resume_publish_engine`.

use std::sync::Arc;

use crate::kernel::publish_engine::OkFramePayload;
use crate::kernel::Kernel;
use crate::publish::{InMemoryPublishStore, PublishStore};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::substrate::{SignedEvent, UnsignedEvent};

const FALLBACK_R1: &str = "wss://relay.damus.io";
const FALLBACK_R2: &str = "wss://nos.lol";

fn fake_signed(id: &str, author: &str, kind: u32, content: &str) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{}", id),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind,
            tags: Vec::new(),
            content: content.to_string(),
            created_at: 1_700_000_000,
        },
    }
}

fn ok_payload<'a>(event_id: &'a str, accepted: bool, reason: &'a str) -> OkFramePayload<'a> {
    OkFramePayload {
        event_id,
        ok: accepted,
        message: reason,
    }
}

#[test]
fn t117_successful_multi_relay_publish_lands_in_engine_recent_ok() {
    // Bullet 1: one publish → two outbox-fallback relays → both ack OK →
    // the engine's `recent_ok` snapshot carries both relays.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let signed = fake_signed(
        "11".repeat(32).as_str(),
        "22".repeat(32).as_str(),
        1,
        "hello t117",
    );
    let outbound = kernel.run_publish_engine_at(&signed, &[], 1_000);
    // No kind:10002 → resolver falls back to DEFAULT_INDEXER_FALLBACK
    // (`wss://relay.damus.io` + `wss://nos.lol` per `publish/nip65/mod.rs`).
    let urls: std::collections::BTreeSet<String> =
        outbound.iter().map(|m| m.relay_url.clone()).collect();
    assert!(urls.contains(FALLBACK_R1));
    assert!(urls.contains(FALLBACK_R2));
    assert_eq!(outbound.len(), 2);

    // Per-relay state is now InFlight — feed OK acks in.
    let _ = kernel.handle_publish_ok_at(FALLBACK_R1, ok_payload(&signed.id, true, ""), 1_010);
    let _ = kernel.handle_publish_ok_at(FALLBACK_R2, ok_payload(&signed.id, true, ""), 1_020);

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
fn t117_auth_required_on_one_relay_reauths_and_other_unaffected() {
    // Bullet 2: relay r1 returns OK-false with `auth-required` on attempt 1.
    // The engine's `apply_ack` routes that to `Reauth` (delay_ms = 0,
    // next_attempt = 2). The kernel ticks the engine; the engine fires a
    // retry (one new frame queued for r1). The second attempt succeeds.
    // Meanwhile r2 sees a clean OK on the original attempt and is untouched.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let signed = fake_signed(
        "33".repeat(32).as_str(),
        "44".repeat(32).as_str(),
        1,
        "auth-required test",
    );
    let outbound = kernel.run_publish_engine_at(&signed, &[], 0);
    assert_eq!(outbound.len(), 2);

    // r1: AUTH-REQUIRED on attempt 1 (no retry frames flushed yet — the
    // engine's Reauth verdict moves the row to RelayError + schedules
    // pending_retries[r1] = now (delay_ms = 0)).
    let retry_now =
        kernel.handle_publish_ok_at(FALLBACK_R1, ok_payload(&signed.id, false, "auth-required: please AUTH"), 100);
    // Reauth's delay_ms = 0 means the very next dispatch_pending fires the
    // retry on the same on_ack call (apply_ack → dispatch on schedule). The
    // engine in `on_ack` runs `apply_verdict` but does NOT call
    // `dispatch_pending`; that happens on `tick`. So no retry frames yet.
    assert!(
        retry_now.is_empty(),
        "on_ack does not eagerly dispatch retries"
    );

    // r2: clean OK on attempt 1.
    let _ = kernel.handle_publish_ok_at(FALLBACK_R2, ok_payload(&signed.id, true, ""), 110);

    // Tick — pending_retries[r1] = 100 + 0 = 100; now = 200 fires the
    // reauth-retry dispatch. The queue dispatcher receives the second frame.
    let retry_frames = kernel.tick_publish_engine(200);
    let retry_urls: Vec<String> = retry_frames.iter().map(|m| m.relay_url.clone()).collect();
    assert_eq!(retry_urls, vec![FALLBACK_R1.to_string()]);

    // Inject the OK for the retry attempt.
    let _ = kernel.handle_publish_ok_at(FALLBACK_R1, ok_payload(&signed.id, true, ""), 210);

    let snap = kernel.publish_status_snapshot();
    assert_eq!(
        snap.recent_ok.len(),
        1,
        "publish completes with one recent_ok row across both relays"
    );
    let accepted = &snap.recent_ok[0].accepted_by;
    assert_eq!(accepted.len(), 2);
    assert!(accepted.iter().any(|r| r == FALLBACK_R1));
    assert!(accepted.iter().any(|r| r == FALLBACK_R2));
    assert!(snap.recent_errors.is_empty(), "no terminal failures");
}

#[test]
fn t117_transient_failure_retries_with_1s_4s_backoff_then_gives_up() {
    // Bullet 3: r1 returns transient ("io") on every attempt. Default policy
    // is transient_max_retries = 3 (attempt 1, 2, 3). Backoffs:
    //   - after attempt 1 → 1_000 ms
    //   - after attempt 2 → 4_000 ms
    //   - after attempt 3 → give up (FailedAfterRetries).
    // We use a single-relay scenario by signing with a kind:10002 that
    // declares only a single explicit relay (via the static-outbox-overridden
    // event store would be heavy — instead we drive both fallback relays and
    // assert just on r1, asserting r2 settled as Ok separately).
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let signed = fake_signed(
        "55".repeat(32).as_str(),
        "66".repeat(32).as_str(),
        1,
        "transient test",
    );
    let outbound = kernel.run_publish_engine_at(&signed, &[], 0);
    assert_eq!(outbound.len(), 2);

    // r2: settle immediately so the engine isn't tracking it any more.
    let _ = kernel.handle_publish_ok_at(FALLBACK_R2, ok_payload(&signed.id, true, ""), 10);

    // r1 attempt 1 → io failure → schedule retry at now + 1s.
    let _ = kernel.handle_publish_ok_at(FALLBACK_R1, ok_payload(&signed.id, false, "io: connection reset"), 100);

    // Tick at 1_500ms — past the 1s backoff (100 + 1_000 = 1_100). Engine
    // dispatches attempt 2.
    let retry2 = kernel.tick_publish_engine(1_500);
    assert_eq!(retry2.len(), 1);
    assert_eq!(retry2[0].relay_url, FALLBACK_R1);

    // r1 attempt 2 → io failure → schedule retry at now + 4s.
    let _ = kernel.handle_publish_ok_at(FALLBACK_R1, ok_payload(&signed.id, false, "io: bad"), 1_600);

    // Tick at 6_000ms — past the 4s backoff (1_600 + 4_000 = 5_600). Engine
    // dispatches attempt 3.
    let retry3 = kernel.tick_publish_engine(6_000);
    assert_eq!(retry3.len(), 1);
    assert_eq!(retry3[0].relay_url, FALLBACK_R1);

    // r1 attempt 3 → io failure → engine gives up (FailedAfterRetries).
    let _ = kernel.handle_publish_ok_at(FALLBACK_R1, ok_payload(&signed.id, false, "io: still bad"), 6_100);
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
    assert_eq!(failure.relay_url, FALLBACK_R1);
    assert!(
        failure.reason.contains("transient"),
        "give-up reason should be transient-flavoured: {}",
        failure.reason
    );
    // r2 settled cleanly.
    assert_eq!(snap.recent_ok.len(), 1);
    assert!(snap.recent_ok[0]
        .accepted_by
        .iter()
        .any(|r| r == FALLBACK_R2));
}

#[test]
fn t117_actor_restart_with_pending_resumes_from_pending_retries() {
    // Bullet 4: a publish dies mid-backoff in kernel A; a fresh kernel B
    // sharing the same PublishStore resumes the pending retry from the
    // store's `pending_retries` rows. Proves T54 durability still holds
    // through the engine-driven path.
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());

    let signed = fake_signed(
        "77".repeat(32).as_str(),
        "88".repeat(32).as_str(),
        1,
        "restart test",
    );

    // Kernel A: drive a transient failure so pending_retries gets populated.
    {
        let mut kernel_a = Kernel::with_publish_store(DEFAULT_VISIBLE_LIMIT, Arc::clone(&store));
        let outbound = kernel_a.run_publish_engine_at(&signed, &[], 0);
        assert_eq!(outbound.len(), 2);
        // r2 settles OK; r1 transient → pending_retries[r1] = 0 + 1_000 = 1_000.
        let _ = kernel_a.handle_publish_ok_at(FALLBACK_R2, ok_payload(&signed.id, true, ""), 10);
        let _ = kernel_a.handle_publish_ok_at(FALLBACK_R1, ok_payload(&signed.id, false, "io: down"), 100);

        // The store now has one durable row with pending_retries on r1.
        let pending = store.load_pending().unwrap();
        assert_eq!(pending.len(), 1, "row persisted in shared store");
        let retries = &pending[0].pending_retries;
        assert!(
            retries.iter().any(|(url, _)| url == FALLBACK_R1),
            "r1 retry deadline must be persisted: {:?}",
            retries
        );
        // Drop kernel_a — simulates process restart.
    }

    // Kernel B: same store, fresh engine. resume_publish_engine wires
    // through `PublishEngine::resume_from_store`, which restores
    // pending_retries. With now far in the future, the retry fires
    // immediately and we feed OK to settle it.
    let mut kernel_b = Kernel::with_publish_store(DEFAULT_VISIBLE_LIMIT, Arc::clone(&store));
    let resumed = kernel_b.resume_publish_engine();
    // `resume_publish_engine` uses wall-clock now (`now_epoch_ms`); the
    // persisted deadline (1_000 ms epoch) is in the deep past so the retry
    // dispatches immediately.
    assert_eq!(
        resumed.len(),
        1,
        "resume must dispatch the pending r1 retry"
    );
    assert_eq!(resumed[0].relay_url, FALLBACK_R1);

    // Ack the retry — wall-clock so we don't accidentally trip the engine's
    // late-ack idempotence path with a past timestamp.
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let _ = kernel_b.handle_publish_ok_at(FALLBACK_R1, ok_payload(&signed.id, true, ""), now);

    let snap = kernel_b.publish_status_snapshot();
    assert_eq!(
        snap.recent_ok.len(),
        1,
        "resumed retry succeeded on the new kernel"
    );
    assert!(
        store.load_pending().unwrap().is_empty(),
        "store cleared after the resumed publish completed"
    );
}
