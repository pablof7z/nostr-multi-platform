//! PR-3 correctness tests: B1 (idempotence gate).
//!
//! Split from `tests.rs` to keep that file under the 500-LOC hard ceiling.
//!
//! B1 — Calling `set_active_account` twice with the same pubkey must be a
//!      no-op (idempotence gate) — it must not re-run the follow-feed
//!      cache-serve teardown on an unchanged account.

use super::*;
const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

#[test]
fn set_active_account_twice_same_pubkey_is_a_noop() {
    // B1 — The idempotence gate: a redundant same-account call must
    // early-return `Vec::new()`. The surviving invariant is the no-op gate
    // itself.
    let mut r = KernelReducer::new();

    // First call: active_account was None → account changes → reconcile runs.
    let _ = r.set_active_account(PK.to_string());

    // Second call with the SAME pubkey — must be a no-op.
    let out = r.set_active_account(PK.to_string());
    assert!(
        out.is_empty(),
        "same-account set_active_account must return Vec::new() (idempotence gate)"
    );
    assert_eq!(
        r.kernel.active_account_pubkey(),
        Some(PK),
        "the active account must be unchanged after the redundant call"
    );
}
