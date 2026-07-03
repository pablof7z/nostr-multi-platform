//! #2937 — composition-aware subscription-routing diagnostic.
//!
//! `Kernel::route_subscription_relays` silently degrades to an empty relay
//! set whenever the router returns `RoutingError::Unroutable` — by design,
//! for the legitimate "composed, but nothing routes for this author" case
//! (see `mailboxes.rs` module doc). The uncomposed case (a bare `new_app()`
//! that never called `NmpApp::set_routing_substrate` / `Kernel::set_routing`,
//! so the kernel is still running the fail-closed `EmptyOutboxRouter`
//! default) used to degrade *identically* — no diagnostic distinguished "no
//! routing substrate exists yet" from "this author has none". These tests
//! prove the two are now distinguishable via the kernel's log ring buffer
//! (the `router_composed` bit set at the `Kernel::set_routing` seam).

use std::sync::Arc;

use super::test_router::TestOutboxRouter;
use super::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn logs_mention_composition_gap(kernel: &Kernel) -> bool {
    kernel
        .logs
        .iter()
        .any(|line| line.contains("nmp_substrate::install"))
}

#[test]
fn uncomposed_router_logs_the_missing_composition_step() {
    // `Kernel::new` never calls `set_routing` — same shape a bare
    // `nmp_native_runtime::new_app()` leaves behind: `EmptyOutboxRouter`,
    // `router_composed == false`.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert!(!kernel.router_composed);

    let resolved = kernel.route_subscription_relays(
        1,
        &[ALICE],
        &[],
        super::mailboxes::BootstrapSeed::Discovery,
    );

    assert!(resolved.is_empty(), "EmptyOutboxRouter must resolve nothing");
    assert!(
        logs_mention_composition_gap(&kernel),
        "uncomposed router must log the missing `nmp_substrate::install` \
         step, not degrade silently: {:?}",
        kernel.logs
    );
}

#[test]
fn composed_router_with_legitimately_unroutable_author_stays_silent() {
    // A real router IS installed (`set_routing` ran, `router_composed ==
    // true`), but this particular author has no cached mailbox and no
    // app-relay/indexer seed — the router accurately returns `Unroutable`.
    // No composition-gap message: nothing was misconfigured.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_routing(
        Arc::new(TestOutboxRouter::new()),
        kernel.mailbox_cache_arc(),
    );
    assert!(kernel.router_composed);

    let resolved = kernel.route_subscription_relays(
        2,
        &[ALICE],
        &[],
        super::mailboxes::BootstrapSeed::Discovery,
    );

    assert!(
        resolved.is_empty(),
        "no lane has anything to resolve from for an unseeded author"
    );
    assert!(
        !logs_mention_composition_gap(&kernel),
        "composed-but-legitimately-unroutable must NOT log the composition \
         message — that would misfire on ordinary fail-closed routing: {:?}",
        kernel.logs
    );
}
