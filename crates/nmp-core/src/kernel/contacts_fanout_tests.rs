//! T100 part-2 / T140 integration tests — kind:3 follow-list arrival drives a
//! timeline re-fan-out onto the newly-followed authors' NIP-65 write relays.
//!
//! **T140 migration note**: the original T100 spec tested the M1 hand-rolled
//! `maybe_open_timeline()` path. Post-T140, the M2 planner (`drain_lifecycle_tick`)
//! is the authoritative source — these tests now verify the M2 path instead.
//!
//! Test posture (updated for T140):
//! - Seed kernel with author A's kind:10002 write relay `R_A`.
//! - Inject kind:10002 for B and C (distinct write relays `R_B`, `R_C`).
//! - Inject kind:3 from the active account listing `[A, B, C]`.
//! - Call `drain_lifecycle_tick()` and assert REQs land on `R_A`, `R_B`, `R_C`.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CAROL: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn install_relay_list(kernel: &Kernel, author: &str, write: &[&str]) {
    kernel.seed_mailbox_relay_list(
        author,
        vec![],
        write.iter().map(|s| s.to_string()).collect(),
        vec![],
    );
}

/// T100 part-2 / T140: when the active account's kind:3 arrives and expands
/// the follow set to include authors with cached kind:10002 write relays, the
/// M2 planner must fan REQ frames out onto those relays on the next
/// `drain_lifecycle_tick()` call.
///
/// Updated from M1 `maybe_open_timeline()` path (retired by T140) to the M2
/// `drain_lifecycle_tick()` path.
#[test]
fn kind3_arrival_fans_out_timeline_onto_new_follows_write_relays() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Declare the host kinds {1, 6} the contact-feed subscription REQs for
    // (D0: the substrate no longer hardcodes a kind set; the host declares it
    // via `ActorCommand::OpenContactFeed { kinds }`).
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);

    // Active account: ALICE.
    kernel.active_account = Some(ALICE.to_string());

    // Cache NIP-65 write relays for all three authors.
    install_relay_list(&kernel, ALICE, &["wss://alice.write/"]);
    install_relay_list(&kernel, BOB, &["wss://bob.write/"]);
    install_relay_list(&kernel, CAROL, &["wss://carol.write/"]);

    // Set generous lifecycle budget so the compiler routes freely.
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);

    // First kind:3: ALICE follows herself only.
    let first_tags = vec![vec!["p".to_string(), ALICE.to_string()]];
    kernel
        .inject_replaceable_event(
            "1111111111111111111111111111111111111111111111111111111111111111",
            ALICE,
            2_000,
            3,
            first_tags,
            "wss://alice.write/",
            2_000_000,
        )
        .expect("inject first kind:3 must succeed");

    // Drain first emission: establishes baseline plan.
    let first_frames = kernel.drain_lifecycle_tick();
    let first_req_urls: Vec<String> = first_frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();
    assert!(
        first_req_urls.iter().any(|u| u == "wss://alice.write/"),
        "first M2 drain must route to ALICE's write relay; got {first_req_urls:?}"
    );

    // Second kind:3: ALICE now follows BOB and CAROL too.
    let new_follows_tags = vec![
        vec!["p".to_string(), ALICE.to_string()],
        vec!["p".to_string(), BOB.to_string()],
        vec!["p".to_string(), CAROL.to_string()],
    ];
    kernel
        .inject_replaceable_event(
            "2222222222222222222222222222222222222222222222222222222222222222",
            ALICE,
            3_000,
            3,
            new_follows_tags,
            "wss://alice.write/",
            3_000_000,
        )
        .expect("inject second kind:3 (expanded follows) must succeed");

    // Drain second emission — must include REQs for BOB and CAROL's relays.
    let second_frames = kernel.drain_lifecycle_tick();
    let second_req_urls: Vec<String> = second_frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();

    assert!(
        second_req_urls.iter().any(|u| u == "wss://bob.write/"),
        "T100/T140: post-kind:3 M2 drain must route to BOB's resolved write \
         relay (he was just added to the follow set); got urls = {second_req_urls:?}"
    );
    assert!(
        second_req_urls.iter().any(|u| u == "wss://carol.write/"),
        "T100/T140: post-kind:3 M2 drain must route to CAROL's resolved write \
         relay (she was just added to the follow set); got urls = {second_req_urls:?}"
    );
}
