//! Hermetic transparency proof for kind:10007 NIP-51 search relays (#1817).
//!
//! Transparent NIP-50 search — `open_search(SearchTargets::UserPreferred)` —
//! fans out to the relays in the active account's kind:10007 list, resolved via
//! [`nmp_defaults::effective_search_relays`] over the
//! [`SearchRelayListProjection`] handle returned by
//! [`nmp_defaults::register_search_relay_runtime`] (and surfaced as
//! `handles.search_relays` from [`nmp_defaults::register_defaults_with_handles`]).
//!
//! Before #1817 the active account's kind:10007 self-fetch was pushed by a
//! bespoke `SearchRelayRuntimeController` whose interest never compiled into a
//! routed REQ, so NMP never fetched the user's search-relay list,
//! `effective_search_relays()` returned empty, and UserPreferred search never
//! fanned out (a real-relay e2e got 0 hits across 540+ ticks). The fix routes
//! kind:10007 through the kernel's proven self-kinds tailing bundle
//! (`SELF_KINDS_TAILING`, alongside kind:10006) — see
//! `nmp-core`'s `kernel/requests/startup.rs` for the wire-routing half, proven
//! deterministically by `bootstrap_routes_kind10007_search_relay_self_fetch`.
//!
//! This file proves the projection half **end to end through the real defaults
//! composition**, deterministically and with NO live relay:
//!
//! 1. `register_defaults_with_handles` returns a live `search_relays` handle
//!    (the composition actually wires the search-relay runtime).
//! 2. The handle is the SAME `Arc<SearchRelayListProjection>` the kernel
//!    fan-out delivers kind:10007 events to (it is registered as an
//!    `EventObserver`). When that projection observes the active account's
//!    kind:10007 event, `effective_search_relays` returns the published relays
//!    with ZERO app involvement — exactly what UserPreferred search reads.
//! 3. Account switch hides the prior account's relays (no cross-account leak);
//!    the effective list falls back to the explicit app default, or to empty
//!    when the app supplied none, until the new account's kind:10007 arrives.

use std::sync::Arc;

use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::KernelEventObserver;
use nmp_ffi::{nmp_app_free, nmp_app_new};
use nmp_nip51::SearchRelayListProjection;

// kind:10007 — NIP-51 search relays. Numeric literal: nmp_kinds is not a direct
// dep of this test crate, and the wire shape is what production relays deliver.
const KIND_SEARCH_RELAYS: u32 = 10_007;

const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

const APP_DEFAULT_RELAY: &str = "wss://app-default-search.example";
const ALICE_SEARCH_RELAY: &str = "wss://alice-search.example";

/// A kind:10007 search-relay list event authored by `author`, carrying
/// `["relay", <url>]` tags — the NIP-51 wire shape `SearchRelayListProjection`
/// parses.
fn search_relay_event(author: &str, created_at: u64, relays: &[&str]) -> KernelEvent {
    KernelEvent {
        id: EventId::from(
            "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
        ),
        author: author.to_string(),
        kind: KIND_SEARCH_RELAYS,
        created_at,
        tags: relays
            .iter()
            .map(|url| vec!["relay".to_string(), (*url).to_string()])
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

/// Build a real `NmpApp` under the default composition, returning the app
/// pointer, the shared active-account slot, and the `search_relays` projection
/// handle that apps read for transparent search.
///
/// The slot is the SAME `Arc<Mutex<Option<String>>>` the kernel populates for
/// every backend (`AppHost::active_pubkey()`), so writing it here mirrors a
/// production sign-in / account switch exactly.
fn boot_with_search_handle(
    search_defaults: nmp_defaults::SearchDefaults,
) -> (
    *mut nmp_ffi::NmpApp,
    nmp_core::slots::ActiveAccountSlot,
    Arc<SearchRelayListProjection>,
) {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    // SAFETY: `app` is a valid non-null pointer from `nmp_app_new`.
    let app_ref: &nmp_ffi::NmpApp = unsafe { &*app };

    let defaults = nmp_defaults::NmpDefaults {
        search_defaults,
        ..Default::default()
    };
    // SAFETY: exclusive access; no actor thread started.
    let handles = nmp_defaults::register_defaults_with_handles(unsafe { &mut *app }, defaults);

    let search_relays = handles
        .search_relays
        .clone()
        .expect("default social composition must wire the search-relay handle");

    let slot = nmp_core::substrate::HostCapabilities::active_pubkey(app_ref);
    (app, slot, search_relays)
}

/// Resolve the effective search relays the way a NIP-50 search caller would,
/// using a known app default so the user-list-vs-fallback decision is visible.
fn effective(
    search_relays: &Arc<SearchRelayListProjection>,
    defaults: &nmp_defaults::SearchDefaults,
) -> Vec<String> {
    nmp_defaults::effective_search_relays(search_relays, defaults)
}

#[test]
fn register_defaults_wires_empty_search_relay_fallback_by_default() {
    let defaults = nmp_defaults::SearchDefaults::default();
    let (app, _slot, search_relays) = boot_with_search_handle(defaults.clone());

    // Before any kind:10007 arrives the no-arg/default composition remains
    // cache-only: shared NMP crates do not choose a public search relay.
    assert_eq!(
        effective(&search_relays, &defaults),
        Vec::<String>::new(),
        "with no published kind:10007 and no app default, effective search relays must be empty"
    );

    nmp_app_free(app);
}

#[test]
fn app_supplied_search_defaults_are_used_when_user_has_no_list() {
    let defaults =
        nmp_defaults::SearchDefaults::with_default_relays(vec![APP_DEFAULT_RELAY.to_string()]);
    let (app, _slot, search_relays) = boot_with_search_handle(defaults.clone());

    assert_eq!(
        effective(&search_relays, &defaults),
        vec![APP_DEFAULT_RELAY.to_string()],
        "with no published kind:10007, explicit app defaults must be the fallback"
    );

    nmp_app_free(app);
}

#[test]
fn ingested_kind10007_makes_effective_search_relays_the_user_list() {
    let defaults =
        nmp_defaults::SearchDefaults::with_default_relays(vec![APP_DEFAULT_RELAY.to_string()]);
    let (app, slot, search_relays) = boot_with_search_handle(defaults.clone());

    // Sign in as ALICE via the shared active-account slot (the kernel populates
    // this slot for every backend; writing it here is exactly what sign-in does).
    *slot.lock().expect("active-account slot") = Some(ALICE.to_string());

    // The kernel's self-kinds tailing bundle fetches ALICE's kind:10007 and fans
    // it to every registered observer. `register_search_relay_runtime` registered
    // `search_relays` as one such observer, so delivering the event to the SAME
    // `Arc` is exactly what the kernel fan-out does. (The wire-routing half — that
    // the self-fetch REQ is actually compiled/routed — is proven deterministically
    // by `nmp-core`'s `bootstrap_routes_kind10007_search_relay_self_fetch`.)
    search_relays.on_kernel_event(&search_relay_event(ALICE, 100, &[ALICE_SEARCH_RELAY]));

    // Transparent search now resolves to ALICE's published relays — no app code
    // touched the list.
    assert_eq!(
        search_relays.snapshot().relays,
        vec![ALICE_SEARCH_RELAY.to_string()],
        "projection snapshot must carry ALICE's published kind:10007 relays"
    );
    assert_eq!(
        effective(&search_relays, &defaults),
        vec![ALICE_SEARCH_RELAY.to_string()],
        "effective_search_relays must return ALICE's kind:10007 list (not the app default) \
         so open_search(UserPreferred) fans out to her chosen relays"
    );

    nmp_app_free(app);
}

#[test]
fn account_switch_does_not_leak_prior_account_search_relays() {
    let defaults =
        nmp_defaults::SearchDefaults::with_default_relays(vec![APP_DEFAULT_RELAY.to_string()]);
    let (app, slot, search_relays) = boot_with_search_handle(defaults.clone());

    // ALICE signs in and her kind:10007 arrives.
    *slot.lock().expect("active-account slot") = Some(ALICE.to_string());
    search_relays.on_kernel_event(&search_relay_event(ALICE, 100, &[ALICE_SEARCH_RELAY]));
    assert_eq!(
        effective(&search_relays, &defaults),
        vec![ALICE_SEARCH_RELAY.to_string()],
        "precondition: ALICE's search relays resolve while she is active"
    );

    // Switch to BOB. BOB has no kind:10007 yet — ALICE's relays must NOT leak;
    // the effective list falls back to the app default (the same re-targeting the
    // kernel's self-kinds bundle performs on the wire — proven by
    // `account_switch_retargets_kind10007_self_fetch` in nmp-core).
    *slot.lock().expect("active-account slot") = Some(BOB.to_string());
    assert_eq!(
        effective(&search_relays, &defaults),
        vec![APP_DEFAULT_RELAY.to_string()],
        "after switching to BOB, ALICE's search relays must NOT appear; \
         effective list falls back to the app default until BOB's kind:10007 arrives"
    );

    // BOB's own kind:10007 then resolves to BOB's relays.
    const BOB_SEARCH_RELAY: &str = "wss://bob-search.example";
    search_relays.on_kernel_event(&search_relay_event(BOB, 200, &[BOB_SEARCH_RELAY]));
    assert_eq!(
        effective(&search_relays, &defaults),
        vec![BOB_SEARCH_RELAY.to_string()],
        "BOB's own kind:10007 must resolve to BOB's relays after switch"
    );

    nmp_app_free(app);
}
