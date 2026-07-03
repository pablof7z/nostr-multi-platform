//! #2923 — fresh-account self-publish routing regression tests.
//!
//! Before this fix, `Kernel::build_routing_context` always built the
//! `SessionKeySet` with `active_write` left at `SessionKeySet::default()`
//! (empty) — so lane 4 (`UserConfigured::ActiveAccountWrite`) could never
//! contribute a URL, even though `active_account` was already threaded
//! through. A fresh account with no cached kind:10002 (lane 1 empty) and no
//! configured discovery/indexer relays (lane 7 empty) publishing a
//! non-discovery kind (lane 6 doesn't apply) had no way to resolve a
//! self-publish target — exactly the `kind:7375`/`17375`/`10019` wallet
//! onboarding failure #2923 describes ("pre-signed publish target
//! rejected: explicit publish target requires at least one relay").
//!
//! `Kernel::recipient_publish_relays` now feeds `local_write_relays_handle`
//! into `active_write` whenever the recipient IS the caller's own active
//! account — the same locally-configured write-relay slot
//! `nmp_router::Nip65OutboxResolver` already consults as its cold-start
//! fallback for `PublishTarget::Auto` self-publishes, so both publish paths
//! agree on where an account's own events go before its kind:10002 exists.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// kind:7375 (NIP-60 token event) — non-discovery, well below the
/// kind:10000-19999 replaceable range lane 6 fans out to indexers.
const KIND_NIP60_TOKEN: u32 = 7375;

fn bare_kernel() -> Kernel {
    // `Kernel::new` (not `new_for_test`) so no fallback content/indexer
    // relays are seeded into `configured_relays` — this test needs lanes
    // 1/6/7 to be genuinely empty so only lane 4 can resolve a target.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_routing(
        Arc::new(test_router::TestOutboxRouter::new()),
        kernel.mailbox_cache_arc(),
    );
    kernel
}

#[test]
fn self_publish_resolves_local_write_relay_with_no_cached_mailbox() {
    let mut kernel = bare_kernel();
    kernel.set_active_account_for_test(ALICE);

    // Simulate an `NmpApp::add_relay(url, "both")` edit that has already
    // synced into the local-write-relays slot but has NOT produced a
    // kind:10002 (the fresh-account onboarding window #2923 describes).
    kernel
        .local_write_relays_handle()
        .lock()
        .unwrap()
        .replace(vec!["wss://my-relay.example/".to_string()]);

    let resolved = kernel.recipient_publish_relays(ALICE, KIND_NIP60_TOKEN);

    assert_eq!(
        resolved,
        vec!["wss://my-relay.example/".to_string()],
        "self-publish of a non-discovery kind must resolve the active \
         account's local-write-relays fallback when no kind:10002 is cached"
    );
}

#[test]
fn self_publish_with_no_local_write_relay_stays_unroutable() {
    // No local-write-relays entry, no cached mailbox, no discovery/indexer
    // seed — the pre-fix behavior, still correct when there is truly
    // nothing to fall back to (fail closed, not a silent broadcast).
    let mut kernel = bare_kernel();
    kernel.set_active_account_for_test(ALICE);

    let resolved = kernel.recipient_publish_relays(ALICE, KIND_NIP60_TOKEN);

    assert!(
        resolved.is_empty(),
        "no lane has anything to resolve from, so the target must stay \
         empty rather than inventing a relay: {resolved:?}"
    );
}

#[test]
fn non_self_publish_ignores_local_write_relays() {
    // The local-write-relays fallback must NOT leak onto a recipient who is
    // NOT the active account — that would leak the viewer's own configured
    // relays onto e.g. an LNURL zap-receipt recipient's publish target.
    let mut kernel = bare_kernel();
    kernel.set_active_account_for_test(ALICE);
    kernel
        .local_write_relays_handle()
        .lock()
        .unwrap()
        .replace(vec!["wss://my-relay.example/".to_string()]);

    let resolved = kernel.recipient_publish_relays(BOB, KIND_NIP60_TOKEN);

    assert!(
        resolved.is_empty(),
        "a non-active-account recipient must not inherit the viewer's \
         local-write-relays fallback: {resolved:?}"
    );
}
