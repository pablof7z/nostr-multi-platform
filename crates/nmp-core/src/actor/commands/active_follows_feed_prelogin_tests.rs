//! #1493 P4 — `declare_active_follows_feed` must persist the compiled follow-feed
//! acquisition kinds even when NO account is active yet.
//!
//! Both Chirp shells mount the home-feed view at launch (iOS
//! `HomeFeedView.task` / Android `TimelineScreen.LaunchedEffect`), which fires
//! `declare_active_follows_feed` BEFORE the user signs in. The pre-fix no-account branch
//! toasted and DROPPED the kinds, so `kernel.follow_feed_kinds` stayed empty;
//! after sign-in the reconcile registered an empty follow-feed and the user saw
//! no timeline until an app restart / tab toggle. Android masked this with an
//! imperative post-identity `openTimeline()`; iOS (View-driven only) carried
//! the latent bug.
//!
//! Lives in its own file (not `tests.rs`) so it does not push that already
//! over-ceiling module further past the file-size baseline (AGENTS.md 500-LOC
//! rule — split, never bump baseline).

use super::*;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn fresh() -> (IdentityRuntime, Kernel) {
    (
        IdentityRuntime::new(
            new_bunker_handshake_slot(),
            crate::actor::new_signer_state_slot(),
        ),
        Kernel::new(DEFAULT_VISIBLE_LIMIT),
    )
}

/// View-driven ordering: declare kinds with NO account, THEN sign in, THEN a
/// kind:3 arrives. `ingest_contacts` re-registers the follow-feed using the
/// STORED kinds — if `declare_active_follows_feed` had dropped them (the old no-account
/// branch), `follow_feed_kinds` would be empty and `drain_lifecycle_tick` would
/// emit 0 REQs.
#[test]
fn declare_active_follows_feed_before_signin_persists_kinds_for_later_reconcile() {
    let (mut id, mut kernel) = fresh();

    // 1. Home-feed view mounts at launch — declare kinds with NO active account.
    assert!(
        id.active_pubkey().is_none(),
        "precondition: no account active when the view first opens the feed"
    );
    let _ = declare_active_follows_feed(
        &id,
        &mut kernel,
        std::collections::BTreeSet::from([1u32, 6u32]),
    );

    // 2. User signs in. The identity-change reconcile runs; the contacts cache
    //    has no kind:3 yet, so no follow-feed interest registers at this point.
    add_signer(
        &mut id,
        &mut kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        true,
        false,
    );
    let active_pk = id.active_pubkey().expect("active account after sign_in");

    // 3. ALICE has a resolved write relay; a kind:3 listing ALICE arrives. The
    //    kind:3 parser populates the contacts cache and the kernel-owned
    //    follow-feed effect re-registers under the stored compiled acquisition
    //    kinds {1,6}.
    kernel.seed_kind10002_for_test(ALICE, &["wss://alice-prelogin.relay/"]);
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);
    kernel.inject_replaceable_event(
        "0000000000000000000000000000000000000000000000000000000000000002",
        &active_pk,
        2_000,
        3,
        vec![vec!["p".to_string(), ALICE.to_string()]],
        "wss://seed.relay/",
        2_000_000,
    );

    // 4. Drain — REQ must be emitted, proving the kinds survived the no-account
    //    window (an empty `follow_feed_kinds` would emit 0 REQs).
    let frames = kernel.drain_lifecycle_tick();
    let req_urls: Vec<String> = frames
        .iter()
        .filter_map(|f| match f {
            crate::subs::WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();

    assert!(
        req_urls.iter().any(|u| u == "wss://alice-prelogin.relay/"),
        "#1493 P4: kinds declared before sign-in must persist so the post-signin \
         follow-feed registers a REQ to ALICE's write relay; got urls: {req_urls:?}"
    );
}
