#![cfg(test)]
//! T127 follow-up tests — actor-tick and boot-resume wiring, plus PD-025/5.
//!
//! T117 left two honest residuals:
//!   - **Residual 1 (actor-tick):** the publish engine was only ticked
//!     opportunistically from `kernel::ingest::handle_message`. On a quiet
//!     socket a transient retry queued in `pending_retries` would wait forever.
//!     T127 adds a periodic tick in the actor's idle path.
//!   - **Residual 3 (boot-resume):** `Kernel::resume_publish_engine` shipped
//!     in T117 but had no production call site. T127 wires it into the
//!     actor's `Start` handler.
//!
//! PD-025/5: codex review finding — quiet-period retry end-to-end verification.

use std::sync::Arc;

use crate::kernel::Kernel;
use crate::publish::{InMemoryPublishStore, PublishStore};
use crate::relay::DEFAULT_VISIBLE_LIMIT;

use super::{fake_signed, now_ms_after_resume, ok_payload, seed_kind10002, WRITE_R1, WRITE_R2};

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
