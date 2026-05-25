//! NIP-47 NWC tag conformance — moved from
//! `crates/nmp-core/tests/nip_tag_conformance.rs` in V-38.
//!
//! V-38 follow-up: the original test drove `wallet_connect` against a real
//! `Kernel` via the (now-deleted) `ConformanceHarness::emit_wallet_connect`
//! helper. Restoring that surface here needs a `Kernel::new_for_test()`
//! public constructor (today the ctor is `pub(crate)`). The in-crate
//! `action::tests` cover the action-seam shape and the runtime's own unit
//! tests cover its NWC encode/decode path; the integration test below is a
//! pinned placeholder for the "real kernel" conformance gate the suite
//! used to assert.

#[test]
#[ignore = "V-38 follow-up: needs `Kernel::new_for_test()` public ctor"]
fn kind23194_nwc_request_carries_wallet_p_tag_placeholder() {
    // V-38 follow-up — see module docs.
}
