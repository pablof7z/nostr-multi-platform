//! T117 integration tests — kernel publish path goes through `PublishEngine`.
//!
//! These tests drive the kernel's engine seam directly:
//! - The engine's `Nip65OutboxResolver` resolves relays from the kernel's
//!   event store. A kind:10002 for the author is seeded via `seed_kind10002`
//!   so `Nip65OutboxResolver` has real NIP-65 write relays to route to.
//!   (T-publish-resolver-indexer / codex f81f735: the old indexer-fallback
//!   path is removed — an author with no kind:10002 produces `NoTargets`, not
//!   a silent publish to arbitrary public relays.)
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
//! 2. AUTH-REQUIRED on one relay, OK on the other: the auth relay PARKS
//!    (availability gate, no retry budget) until it reaches `Authenticated`,
//!    then re-dispatches and settles; untouched relay stays Ok.
//! 3. Transient failure × 3: 1s backoff → 4s backoff → give-up;
//!    `FailedAfterRetries` row appears on the snapshot.
//! 4. Restart with a Pending row: build a second Kernel sharing the same
//!    `Arc<dyn PublishStore>`; engine resumes via `resume_publish_engine`.

use std::sync::Arc;

use crate::kernel::publish_engine::OkFramePayload;
use crate::kernel::Kernel;
use crate::publish::{InMemoryPublishStore, PublishStore};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::store::{RawEvent, VerifiedEvent};
use crate::substrate::{SignedEvent, UnsignedEvent};

/// T117 test relay URLs — two explicit write relays declared in kind:10002
/// (replaces the old `FALLBACK_R1/R2` indexer-fallback constants; these are
/// now NIP-65-routed, not fallback-routed).
const WRITE_R1: &str = "wss://write-r1.test";
const WRITE_R2: &str = "wss://write-r2.test";

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

/// Seed a kind:10002 into the kernel's event store for `author_pubkey` with
/// `write_urls` as its write-marker relay tags. Required so
/// `Nip65OutboxResolver` has real NIP-65 data and does not return `NoTargets`.
fn seed_kind10002(kernel: &mut Kernel, author_pubkey: &str, write_urls: &[&str]) {
    let tags: Vec<Vec<String>> = write_urls
        .iter()
        .map(|url| vec!["r".to_string(), url.to_string(), "write".to_string()])
        .collect();
    // Use the author pubkey as the event id — guaranteed valid hex (64 hex
    // chars) and unique per author in a fresh-kernel test.  The old two-char
    // prefix approach embedded a literal 'k' which is not a valid hex
    // character; V-70 strengthened `is_structurally_valid()` to check hex
    // chars, so those synthetic events were rejected as Malformed and never
    // entered the store (mirrors the canonical `seed_kind10002_for_test`).
    let id = author_pubkey.to_string();
    let raw = RawEvent {
        id,
        pubkey: author_pubkey.to_string(),
        created_at: 1_700_000_000,
        kind: 10002,
        tags,
        content: String::new(),
        sig: "0".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    kernel
        .store
        .insert(verified, &"wss://seed".to_string(), 1_700_000_000_000)
        .expect("seed_kind10002 insert");
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
    // `resume_publish_engine` uses wall-clock now (`now_epoch_ms`); the
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

// ── T127 follow-up — actor-tick + boot-resume wiring ─────────────────────────
//
// T117 left two honest residuals on the table:
//   - **Residual 1 (actor-tick):** the publish engine was only ticked
//     opportunistically from `kernel::ingest::handle_message` (one inbound
//     frame → one tick). On a quiet socket (no acks, no relay traffic) a
//     transient retry queued in `pending_retries` would wait forever. T127
//     adds a periodic tick in the actor's idle path (`actor/mod.rs::run_actor`,
//     the `Ok(None)` branch of `next_actor_msg`).
//   - **Residual 3 (boot-resume):** `Kernel::resume_publish_engine` shipped
//     in T117 but had no production call site. T127 wires it into the
//     actor's `Start` handler (`actor/dispatch.rs`).
//
// The actor wiring is two lines; the FSM behavior is already proved by the
// T117 tests above. These T127 tests lock the *kernel-level contract* that
// the actor consumes — same convention as `t117_actor_restart_with_pending_
// resumes_from_pending_retries`, which is also kernel-level despite the
// "actor" in its name.

#[test]
fn t127_quiet_socket_tick_progresses_pending_retry_without_inbound() {
    // Residual 1 contract: a transient failure schedules a retry into
    // `pending_retries`; on a quiet socket (no further inbound frames, so
    // no opportunistic tick in `handle_message`) the only thing that drives
    // the engine is the actor's periodic tick. This test calls
    // `tick_publish_engine(now_ms)` exactly once (the actor's idle-path
    // call) and asserts a retry frame is dispatched. Distinct from the T117
    // transient test (`t117_transient_failure_retries_with_1s_4s_backoff_
    // then_gives_up`), which interleaves ticks with synthetic OK-false
    // acks — this one proves the tick alone is sufficient when no further
    // wire activity occurs.
    let author = "bb".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed(
        "aa".repeat(32).as_str(),
        &author,
        1,
        "quiet-socket tick test",
    );
    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 2, "two NIP-65 write relays expected");

    // r2 settles immediately so the engine isn't tracking it any more —
    // the rest of the test is single-relay (r1).
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 10);

    // r1 attempt 1 → transient io failure → engine schedules
    // pending_retries[r1] = 100 + 1_000 = 1_100. NB: this `handle_publish_ok`
    // call is the *last* inbound the kernel sees in this test — every
    // subsequent tick must come from the actor's idle-path call alone.
    let post_ack = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "io: connection reset"),
        100,
    );
    assert!(
        post_ack.is_empty(),
        "on_ack records the verdict but does not eagerly dispatch — \
         the retry must come from the next tick"
    );

    // Tick BEFORE the backoff is due — engine must NOT dispatch yet
    // (proves the tick isn't accidentally firing every retry on every call).
    let too_early = kernel.tick_publish_engine(500);
    assert!(
        too_early.is_empty(),
        "tick before 1s backoff window must be a no-op; got {} frames",
        too_early.len()
    );

    // Tick AFTER the backoff is due — exactly what the actor's
    // `tick_publish_engine_for_now` call on the next idle poll produces.
    // No new inbound frames, no opportunistic ingest-tick — this single
    // call must dispatch the retry on its own.
    let retry = kernel.tick_publish_engine(1_500);
    assert_eq!(
        retry.len(),
        1,
        "quiet-socket retry must dispatch from the actor tick alone"
    );
    assert_eq!(retry[0].relay_url, WRITE_R1);
    assert!(
        retry[0].text.contains("EVENT"),
        "retry frame must be a NIP-01 EVENT publish, got: {}",
        retry[0].text
    );
}

#[test]
fn t127_start_path_drives_resume_publish_engine() {
    // Residual 3 contract: the actor's `Start` handler (in
    // `actor/dispatch.rs`) now calls `kernel.resume_publish_engine()` and
    // returns its outbound frames. This test exercises the kernel-side
    // half of that contract: given a populated `PublishStore` (the LMDB
    // future, simulated today by sharing an `Arc<dyn PublishStore>`
    // across two kernel instances), a freshly-constructed kernel that
    // sees its first `resume_publish_engine` call MUST re-dispatch every
    // due `pending_retries` row.
    //
    // The actor wiring this test pins is the *call* — `Start` invokes
    // `resume_publish_engine` exactly once and routes the returned frames
    // through `send_all_outbound`. The downstream behaviour (the FSM
    // bringing each row back into `InFlight` and dispatching) is exactly
    // what's asserted here.
    let publish_store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());

    let author = "dd".repeat(32);
    let signed = fake_signed("cc".repeat(32).as_str(), &author, 1, "boot-resume test");

    // Kernel A: drive a transient failure so the durable store carries one
    // `pending_retries` row with a past-due deadline. Mirror of T117's
    // restart test, but the deadline is set so that resume must dispatch
    // **immediately** (the engine compares against wall-clock `now` and
    // the seeded deadline is 0).
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
        let _ = kernel_a.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 10);
        let _ =
            kernel_a.handle_publish_ok_at(WRITE_R1, ok_payload(&signed.id, false, "io: down"), 100);

        let pending = publish_store.load_pending().unwrap();
        assert_eq!(
            pending.len(),
            1,
            "store carries one durable row pre-restart"
        );
    }

    // Kernel B: this is exactly the state the actor's `Start` handler
    // produces — a fresh kernel sharing the same `Arc<dyn PublishStore>`.
    // The first `resume_publish_engine` call (which `Start` invokes once,
    // after `spawn_missing_relays`) must dispatch the due retry on r1.
    let mut kernel_b =
        Kernel::with_publish_store(DEFAULT_VISIBLE_LIMIT, Arc::clone(&publish_store));
    let resumed = kernel_b.resume_publish_engine();
    assert_eq!(
        resumed.len(),
        1,
        "Start-equivalent resume must dispatch the persisted r1 retry; got {} frames",
        resumed.len()
    );
    assert_eq!(resumed[0].relay_url, WRITE_R1);
    assert!(
        resumed[0].text.contains("EVENT"),
        "resumed frame must be a NIP-01 EVENT publish, got: {}",
        resumed[0].text
    );

    // The actor's `Start` calls `resume_publish_engine` exactly once per
    // Start command (a Stop → Start cycle reconstructs `relay_controls`
    // and resets `startup_sent`, but the kernel survives — so the engine
    // state survives too and the second resume's behaviour matters less
    // than the first). Locking the once-per-Start invariant is the actor
    // wiring's job, not the kernel's. Ack the dispatched retry so the
    // store clears and we exit clean — proves the resumed publish
    // completes end-to-end through the same path the actor drives.
    let _ = kernel_b.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, true, ""),
        now_ms_after_resume(&signed),
    );
    let snap = kernel_b.publish_status_snapshot();
    assert_eq!(
        snap.recent_ok.len(),
        1,
        "resumed publish must complete after the OK ack"
    );
    assert!(
        publish_store.load_pending().unwrap().is_empty(),
        "store cleared after the resumed publish completed"
    );
}

// ── PD-025 finding 5 — quiet-period retry end-to-end verification ────────────
//
// PD-025/5 (from the 6711b01 codex review): engine retry pump is opportunistic
// on every inbound text frame. If a relay goes quiet between OK and a due
// retry, retries stall until the next inbound.
//
// T127 (`2e249a6`) added `tick_publish_engine_for_now()` to the actor's idle
// path (`actor/mod.rs` — the `Ok(None)` branch of `recv_timeout`). The four
// required conditions (PD-025/5 spec):
//   1. Submit a publish that fails (relay returns OK false / transient).
//   2. Close the relay (no more inbound frames — the engine's opportunistic
//      `handle_message` tick never fires again).
//   3. Wake the kernel via an actor idle tick (or scenePhase/Foreground).
//   4. Assert the retry fires.
//
// This test is a **regression anchor** for the full path. Conditions 1-2-3-4
// are exercised directly at the kernel API boundary that the actor consumes:
//   - Step 1 → `run_publish_engine_at` + `handle_publish_ok_at` (OK=false).
//   - Step 2 → no further inbound calls (silence simulated by test structure).
//   - Step 3 → `tick_publish_engine(now_ms)` — exactly what the actor's
//     idle path calls as `tick_publish_engine_for_now()` on each 250ms poll.
//   - Step 4 → assert retry frame dispatched.
//
// The relationship to T127: `t127_quiet_socket_tick_progresses_pending_retry_
// without_inbound` already pins all four conditions at the same API surface.
// This test annotates that coverage explicitly under the PD-025/5 identifier
// so the regression is searchable and the codex finding has a named resolution.
//
// NOTE on LifecycleEvent(Foreground) as a wake trigger: sending a Foreground
// event to the actor does NOT directly call `tick_publish_engine` — it only
// fires the registered lifecycle observer (for nip-77 reconcile). The retry
// fires because after the `LifecycleEvent` dispatch returns, the actor's next
// `recv_timeout(250ms)` times out and the idle branch calls the tick. The
// 250ms actor poll IS the wakeup; the foreground event is incidental. Testing
// through the actor layer would require real relay sockets; the kernel-level
// API below is the authoritative, deterministic path.

#[test]
fn pd025_finding5_quiet_period_retry_fires_on_actor_tick() {
    // Regression anchor: PD-025/5. Verifies T127's quiet-period fix end-to-end
    // at the kernel API surface. No relay sockets, no sleeps, time injected.
    let author = "ff".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1, WRITE_R2]);
    let signed = fake_signed(
        "ee".repeat(32).as_str(),
        &author,
        1,
        "pd025-finding5 quiet-period retry test",
    );

    // Step 1a: dispatch publish → two NIP-65 write relays.
    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(
        outbound.len(),
        2,
        "publish dispatched to two NIP-65 write relays"
    );

    // Step 1b: r2 settles OK; r1 returns transient failure (io error).
    // After this ack r1's state is InFlight → RelayError + pending_retries[r1]
    // scheduled at 100 + 1_000 = 1_100 ms.
    let _ = kernel.handle_publish_ok_at(WRITE_R2, ok_payload(&signed.id, true, ""), 50);
    let post_failure = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "io: connection reset by peer"),
        100,
    );
    assert!(
        post_failure.is_empty(),
        "on_ack schedules retry but does not eagerly dispatch — the retry must \
         come from the next tick, not from on_ack"
    );

    // Step 2: relay goes QUIET — no further inbound frames; the opportunistic
    // `tick_publish_engine` call in `handle_message` never fires again.
    // (Simulated here by the test simply not calling handle_* any further.)

    // Step 3a: actor idle tick BEFORE backoff window — must be a no-op.
    let premature = kernel.tick_publish_engine(500);
    assert!(
        premature.is_empty(),
        "tick before 1s backoff must not dispatch (pending_retries deadline not yet due)"
    );

    // Step 3b: actor idle tick AFTER backoff window (T127 fix: the actor's
    // `tick_publish_engine_for_now()` in the `Ok(None)` idle branch).
    // This is exactly what `run_actor` calls every ~250ms when running=true.

    // Step 4: retry must fire from the tick alone (no inbound frame triggered it).
    let retry = kernel.tick_publish_engine(1_500);
    assert_eq!(
        retry.len(),
        1,
        "PD-025/5: quiet-period retry must fire from actor tick alone; \
         got {} frames (T127 regression — quiet relay + no inbound = stall)",
        retry.len()
    );
    assert_eq!(
        retry[0].relay_url, WRITE_R1,
        "retry must target r1 (the relay that returned transient failure)"
    );
    assert!(
        retry[0].text.contains("EVENT"),
        "retry frame must be a NIP-01 EVENT publish; got: {}",
        retry[0].text
    );
}

#[test]
fn user_retry_publish_now_dispatches_backoff_state() {
    let author = "ee".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1]);
    let signed = fake_signed("ef".repeat(32).as_str(), &author, 1, "manual retry");

    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 1);
    let scheduled = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "io: temporary outage"),
        100,
    );
    assert!(
        scheduled.is_empty(),
        "transient failure should schedule backoff, not dispatch immediately"
    );

    let retry = kernel.retry_publish_now(&signed.id);
    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].relay_url, WRITE_R1);
    assert!(
        retry[0].text.contains(&signed.id),
        "manual retry must re-dispatch the original signed event"
    );
}

#[test]
fn user_retry_publish_now_requeues_settled_failed_history_row() {
    let author = "e1".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1]);
    let signed = fake_signed("e2".repeat(32).as_str(), &author, 1, "retry terminal");

    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 1);
    let retry = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "blocked: spam"),
        100,
    );
    assert!(retry.is_empty());
    let failed = kernel.publish_queue_snapshot().last().unwrap();
    assert_eq!(failed.status, "failed");
    assert!(failed.can_retry, "settled failure should expose retry");

    let retried = kernel.retry_publish_now(&signed.id);
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0].relay_url, WRITE_R1);
    let rows: Vec<_> = kernel
        .publish_queue_snapshot()
        .iter()
        .filter(|entry| entry.event_id == signed.id)
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "retry should refine the existing history row"
    );
    assert_eq!(rows[0].status, "accepted_locally");
    assert!(!rows[0].can_retry);
}

#[test]
fn user_cancel_publish_clears_settled_history_row() {
    let author = "e3".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1]);
    let signed = fake_signed("e4".repeat(32).as_str(), &author, 1, "clear terminal");

    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 1);
    let retry = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "blocked: spam"),
        100,
    );
    assert!(retry.is_empty());
    assert_eq!(
        kernel.publish_queue_snapshot().last().unwrap().status,
        "failed"
    );

    kernel.cancel_publish(&signed.id);

    assert!(
        kernel
            .publish_queue_snapshot()
            .iter()
            .all(|entry| entry.event_id != signed.id),
        "clear should remove the settled history row"
    );
}

#[test]
fn user_cancel_publish_removes_in_flight_and_store_intent() {
    let publish_store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let author = "ed".repeat(32);
    let mut kernel = Kernel::with_publish_store(DEFAULT_VISIBLE_LIMIT, Arc::clone(&publish_store));
    seed_kind10002(&mut kernel, &author, &[WRITE_R1]);
    let signed = fake_signed("fa".repeat(32).as_str(), &author, 1, "cancel me");

    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert_eq!(outbound.len(), 1);
    assert_eq!(kernel.publish_status_snapshot().in_flight.len(), 1);

    kernel.cancel_publish(&signed.id);

    assert!(
        kernel.publish_status_snapshot().in_flight.is_empty(),
        "cancel must remove the in-flight publish row"
    );
    assert!(
        publish_store.load_pending().unwrap().is_empty(),
        "cancel must delete the durable publish intent"
    );
    assert_eq!(
        kernel.publish_queue_snapshot().last().unwrap().status,
        "cancelled"
    );
}

// ─── T-publish-resolver-indexer: fail-closed for unroutable authors ──────────
//
// Pins the new `NoTargets` semantics: an author with no kind:10002 must not
// silently publish to arbitrary public relays. The engine surfaces `NoTargets`
// so the UI can show "no relay to publish to" rather than a silent failure.
// Mirrors T134's subscription-side `unroutable_authors` discipline.

#[test]
fn t_publish_resolver_unroutable_author_no_kind10002_produces_no_targets() {
    // An author with no kind:10002 in the store must produce ZERO outbound
    // frames and a `RecentFailure` row on the publish-status snapshot.
    // (Previously the old indexer-fallback would produce 2 frames destined
    // for arbitrary public relays; that path is removed per codex f81f735.)
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Intentionally do NOT seed kind:10002 for this author.
    let signed = fake_signed(
        "ab".repeat(32).as_str(),
        "cd".repeat(32).as_str(),
        1,
        "unroutable author publish test",
    );
    let outbound =
        kernel.run_publish_engine_at(&signed, &[], crate::publish::PublishTarget::Auto, None, 0);
    assert!(
        outbound.is_empty(),
        "author with no kind:10002 must produce zero outbound frames (NoTargets, fail-closed); \
         got {} frames targeting: {:?}",
        outbound.len(),
        outbound.iter().map(|m| &m.relay_url).collect::<Vec<_>>()
    );

    // The engine must surface the failure visibly — a `RecentFailure` row
    // on the snapshot (D6: errors never cross FFI silently).
    let snap = kernel.publish_status_snapshot();
    assert!(
        !snap.recent_errors.is_empty(),
        "unroutable publish must record a RecentFailure (D6 — no silent drop)"
    );
}

/// Helper for the boot-resume test: `handle_publish_ok_at` needs a `now_ms`
/// strictly past the engine's most recent recorded ack timestamp, otherwise
/// `apply_ack`'s late-ack idempotence path would discard the OK as stale.
/// `resume_publish_engine` uses wall-clock `now_epoch_ms()`, so this returns
/// the same wall-clock time the engine already saw.
fn now_ms_after_resume(_signed: &SignedEvent) -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
