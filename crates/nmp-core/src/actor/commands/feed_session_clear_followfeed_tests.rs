//! #1740 step 2 (BLOCKING) — `close_feed` must release the ACTOR-OWNED
//! active-follows interests, not just the registry artifacts.
//!
//! `open_feed`'s `ActiveUserFollows` compiler (nmp-defaults
//! `register_op_feed_defaults`) declares the actor-owned active-follows feed by
//! issuing `ActorCommand::DeclareActiveFollowsFeed`, which the actor dispatches
//! to [`super::declare_active_follows_feed`] →
//! `kernel.set_follow_feed_kinds(..)`. That registers the active account's M2
//! follow-feed INTERESTS against the kernel — state that survives unregistering
//! the feed controller / revoking observers / removing the projection.
//!
//! The teardown-recipe fix wires `close_feed` to ALSO issue
//! `ActorCommand::ClearActiveFollowsFeed`, which the actor dispatches to
//! [`super::clear_active_follows_feed`] →
//! `kernel.set_follow_feed_kinds(empty)`. THIS is the path that withdraws the
//! follow-feed interests + clears the active-follows internal state.
//!
//! These tests drive the EXACT command handlers the actor's `dispatch_command`
//! invokes for those two `ActorCommand`s (the t168 / prelogin idiom — the
//! canonical, deterministic, kernel-introspectable proof of an actor command's
//! effect), and assert the open↔close symmetry the BLOCKING finding required:
//! after the open-side declare the interests + active-follows state are PRESENT;
//! after the close-side clear they are GONE.
//!
//! Why here (nmp-core) and not nmp-defaults: the follow-feed interest set
//! (`follow_feed_interest_ids`) and `timeline_authors` are kernel-private; only
//! `nmp-core` (via the `pub(crate)` `*_for_test` accessors) can OBSERVE the
//! actor-owned interest release. The nmp-defaults `feed_session_open_close_test`
//! is a pre-start/direct-registry test that cannot see this state at all — which
//! is exactly why it could not catch the leak. The nmp-ffi `feed_session` order
//! test proves `close_feed`'s recipe actually SENDS `ClearActiveFollowsFeed`
//! (before the final notify); these prove what that command DOES.

use super::*;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;

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

fn close_count(frames: &[WireFrame]) -> usize {
    frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Close { .. }))
        .count()
}

/// Sign in account A, then OPEN-side declare the active-follows feed (what
/// `register_op_feed_defaults` issues via `DeclareActiveFollowsFeed`), then
/// ingest A's kind:3 (follows ALICE) and drain — so the M2 follow-feed interest
/// is registered + live, exactly as it is after `open_feed` over the real
/// op-feed composition with a signed-in account that has a contact list.
fn open_active_follows_for_a(id: &mut IdentityRuntime, kernel: &mut Kernel) -> String {
    add_signer(
        id,
        kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        true,
        false,
    );
    let active_pk = id.active_pubkey().expect("active account after sign_in");

    // OPEN-side: the active-follows acquisition declaration. This is the actor
    // handler `ActorCommand::DeclareActiveFollowsFeed` dispatches to, i.e. the
    // command `open_feed`'s `register_op_feed_defaults` compiler issues.
    let _ = declare_active_follows_feed(id, kernel, std::collections::BTreeSet::from([1u32, 6u32]));

    kernel.seed_kind10002_for_test(ALICE, &["wss://alice-clear.relay/"]);
    kernel.inject_replaceable_event(
        "0000000000000000000000000000000000000000000000000000000000000001",
        &active_pk,
        2_000,
        3,
        vec![vec!["p".to_string(), ALICE.to_string()]],
        "wss://seed.relay/",
        2_000_000,
    );
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);
    let frames = kernel.drain_lifecycle_tick();
    kernel.register_wire_frames_for_test(&frames);
    active_pk
}

/// BLOCKING-finding proof: the close-side `clear_active_follows_feed` (what
/// `close_feed`'s teardown recipe now issues via `ClearActiveFollowsFeed`)
/// WITHDRAWS the actor-owned active-follows interests AND clears the
/// active-follows internal state — symmetric with the open-side declare.
///
/// Pre-fix (`close_feed` only unregistered controller + revoked observers +
/// removed projection, NEVER issuing `ClearActiveFollowsFeed`): the follow-feed
/// interests + `timeline_authors` stay LIVE after close → the post-clear
/// assertions FAIL. Post-fix: close issues `ClearActiveFollowsFeed` → interests
/// withdrawn, `timeline_authors` cleared, CLOSE diff emitted → PASSES.
#[test]
fn close_feed_clears_active_follows_interests_and_state() {
    let (mut id, mut kernel) = fresh();
    let _a = open_active_follows_for_a(&mut id, &mut kernel);

    // After OPEN: the actor-owned active-follows state is PRESENT.
    assert!(
        !kernel.follow_feed_interest_ids_for_test().is_empty(),
        "open: declaring the active-follows feed must register A's follow-feed \
         interest"
    );
    assert!(
        kernel.timeline_authors_for_test().contains(ALICE),
        "open: ALICE (A's follow) must be in timeline_authors after the open-side \
         declare + kind:3"
    );

    // CLOSE: the EXACT handler `ActorCommand::ClearActiveFollowsFeed` dispatches
    // — the command the fixed `close_feed` teardown recipe now issues.
    let _outbound = clear_active_follows_feed(&id, &mut kernel);
    let frames = kernel.drain_lifecycle_tick();

    // After CLOSE: the actor-owned active-follows state is GONE — proof the
    // teardown releases internal interests, not just registry artifacts.
    assert!(
        kernel.follow_feed_interest_ids_for_test().is_empty(),
        "close: clearing the active-follows feed must WITHDRAW A's follow-feed \
         interests; still registered: {:?}",
        kernel.follow_feed_interest_ids_for_test()
    );
    assert!(
        !kernel.timeline_authors_for_test().contains(ALICE),
        "close: ALICE must be gone from timeline_authors after the active-follows \
         clear (active-follows internal state cleared)"
    );
    assert!(
        close_count(&frames) >= 1,
        "close: clearing the active-follows feed must emit a CLOSE diff for A's \
         now-orphaned follow-feed sub; got frames: {frames:?}"
    );
}

/// Open↔close↔open symmetry: after a close-side clear, a subsequent open-side
/// declare (+ the still-cached kind:3) RE-REGISTERS the interest. Proves the
/// clear genuinely RELEASED state (a fresh declare rebuilds it) rather than the
/// state never having existed, and that close is not a one-way latch.
#[test]
fn reopen_after_close_reregisters_active_follows_interest() {
    let (mut id, mut kernel) = fresh();
    let _a = open_active_follows_for_a(&mut id, &mut kernel);
    assert!(
        !kernel.follow_feed_interest_ids_for_test().is_empty(),
        "sanity: interest present after first open"
    );

    // Close → released.
    let _ = clear_active_follows_feed(&id, &mut kernel);
    let _ = kernel.drain_lifecycle_tick();
    assert!(
        kernel.follow_feed_interest_ids_for_test().is_empty(),
        "interest withdrawn after close"
    );

    // Re-open: declare again. The cached kind:3 means
    // `register_follow_feed_for_active_account` re-installs the interest.
    let _ = declare_active_follows_feed(
        &id,
        &mut kernel,
        std::collections::BTreeSet::from([1u32, 6u32]),
    );
    let _ = kernel.drain_lifecycle_tick();
    assert!(
        !kernel.follow_feed_interest_ids_for_test().is_empty(),
        "re-open: a fresh active-follows declare must re-register the interest \
         the prior close released; ids: {:?}",
        kernel.follow_feed_interest_ids_for_test()
    );
    assert!(
        kernel.timeline_authors_for_test().contains(ALICE),
        "re-open: ALICE back in timeline_authors after the second declare"
    );
}
