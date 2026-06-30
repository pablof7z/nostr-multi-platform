//! Unit tests for `InboxStore` internals — `admit`, `chain_done`, and
//! `decrypt_status`. These are store-level tests that do not require the full
//! port-chain harness; the end-to-end projection tests live in `chain_tests.rs`.
//!
//! The regression tests for issue #1349 live here:
//! * `over_bound_decrements_on_successful_re_admit` — Defect 1: over_bound must
//!   reflect CURRENTLY-DEFERRED envelopes, not a monotonic reject count.
//! * `stale_epoch_chain_done_does_not_corrupt_new_account_counter` — Defect 2:
//!   chain_done(generation) must be a no-op when the epoch has advanced.

use std::sync::Arc;

use super::store::{DecryptState, InboxStore, MAX_IN_FLIGHT_DECRYPTS};

// ── Regression #1349 Defect 1: over_bound drains on successful re-admit ──────

#[test]
fn over_bound_decrements_on_successful_re_admit() {
    // Scenario (mirrors real bunker backfill where the Tailing subscription
    // re-delivers every rejected envelope once in-flight slots free up):
    //
    //   Phase 1 — fill the bound. Admit MAX chains; then attempt 3 more.
    //     Those 3 are over-bound: over_bound=3, in_flight=MAX.
    //   Phase 2 — drain the admitted batch. Call chain_done(gen) MAX times.
    //     in_flight=0, over_bound=3 → undecrypted=3, state=Limited.
    //   Phase 3 — re-admit the 3 deferred slots (Tailing relay re-delivers).
    //     Each successful admit must decrement over_bound by 1.
    //     After 3 re-admits + chain_done: over_bound=0, in_flight=0 → Ok.
    //
    // Before the fix, over_bound was only reset by clear(); it stayed at 3
    // permanently, keeping decrypt_status=(Limited, 3) even after all
    // chains finished — the permanent stuck "limited" UI.

    let store = Arc::new(InboxStore::new());
    let gen = store.generation();
    let n_over: u64 = 3;
    let total = MAX_IN_FLIGHT_DECRYPTS + n_over;

    // Phase 1: fill the bound.
    for _ in 0..MAX_IN_FLIGHT_DECRYPTS {
        assert!(store.admit(), "slots 0..MAX must be admitted");
    }
    // 3 more are rejected and recorded as over-bound.
    for _ in 0..n_over {
        assert!(!store.admit(), "slot > MAX must be rejected");
    }
    let (state, count) = store.decrypt_status(true);
    assert_eq!(
        state,
        DecryptState::Limited,
        "Phase 1: bound full + deferred → limited"
    );
    assert_eq!(
        u64::from(count),
        total,
        "Phase 1: in_flight + over_bound = MAX + n_over"
    );

    // Phase 2: drain the admitted batch (bunker chains resolve).
    for _ in 0..MAX_IN_FLIGHT_DECRYPTS {
        store.chain_done(gen);
    }
    let (state, count) = store.decrypt_status(true);
    assert_eq!(
        state,
        DecryptState::Limited,
        "Phase 2: deferred still outstanding → limited"
    );
    assert_eq!(
        u64::from(count),
        n_over,
        "Phase 2: only the 3 deferred remain"
    );

    // Phase 3: Tailing sub re-delivers the 3 over-bound envelopes.
    // Each admit must (a) succeed (slots freed) and (b) consume one over_bound.
    for i in 0..n_over {
        assert!(
            store.admit(),
            "re-delivered envelope {i} must be admitted (slot free)"
        );
    }
    let (state, count) = store.decrypt_status(true);
    assert_eq!(
        state,
        DecryptState::Limited,
        "Phase 3 mid: re-admitted but not yet drained"
    );
    assert_eq!(
        u64::from(count),
        n_over,
        "Phase 3 mid: in_flight=n_over, over_bound=0"
    );

    // Drain the re-admitted chains.
    for _ in 0..n_over {
        store.chain_done(gen);
    }
    let (state, count) = store.decrypt_status(true);
    assert_eq!(
        state,
        DecryptState::Ok,
        "#1349 Defect 1 regression: after all deferred envelopes re-admitted and drained, \
         over_bound must be 0 and decrypt_status must return Ok"
    );
    assert_eq!(
        count, 0,
        "#1349 Defect 1 regression: undecrypted_count must be 0 after full drain"
    );
}

// ── Regression #1349 Defect 2: epoch-safe chain_done ────────────────────────

#[test]
fn stale_epoch_chain_done_does_not_corrupt_new_account_counter() {
    // Regression for #1349 Defect 2: chain_done(generation) must be a no-op
    // when the epoch has advanced (account switch mid-flight).
    //
    // Before the fix: chain_done was epoch-blind. If clear() zeroed in_flight
    // and a new account admitted one chain (in_flight=1), a stale old-epoch
    // chain_done would load in_flight=1, pass the `> 0` guard, and decrement
    // to 0 — making the new account transiently report Ok with a decrypt still
    // outstanding (the race described in #1349 §D2).

    let store = Arc::new(InboxStore::new());
    let old_gen = store.generation(); // G

    // Old account admits one chain (in_flight=1 at epoch G).
    assert!(store.admit(), "first admit must succeed");
    assert_eq!(
        store.decrypt_status(true).0,
        DecryptState::Limited,
        "old account: one in-flight → limited"
    );

    // Account switch: clear() bumps generation to G+1, resets in_flight=0.
    store.clear();
    let new_gen = store.generation();
    assert_ne!(new_gen, old_gen, "clear() must bump the generation");
    assert_eq!(
        store.decrypt_status(true).0,
        DecryptState::Ok,
        "fresh account after clear starts ok"
    );

    // New account admits one chain (in_flight=1 at epoch G+1).
    assert!(store.admit(), "new account admit must succeed");
    assert_eq!(
        store.decrypt_status(true).0,
        DecryptState::Limited,
        "new account has one in-flight → limited"
    );

    // OLD epoch's parked chain_done fires with stale generation G.
    // Before the fix this would decrement in_flight from 1→0, making the new
    // account spuriously report Ok while a decrypt is still in flight.
    store.chain_done(old_gen); // must be a no-op (epoch mismatch)

    let (state, count) = store.decrypt_status(true);
    assert_eq!(
        state,
        DecryptState::Limited,
        "#1349 Defect 2 regression: stale chain_done(old_gen) must not corrupt the new \
         account's in_flight counter — state must remain Limited"
    );
    assert_eq!(
        count, 1,
        "#1349 Defect 2 regression: in_flight must remain 1 after a no-op stale chain_done"
    );

    // Properly terminate the new account's chain — now it should clear.
    store.chain_done(new_gen);
    assert_eq!(
        store.decrypt_status(true).0,
        DecryptState::Ok,
        "after the legitimate chain_done(new_gen) the new account's state returns to ok"
    );
}
