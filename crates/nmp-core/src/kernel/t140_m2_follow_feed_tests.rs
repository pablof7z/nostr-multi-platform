//! T140 RED tests — M2 follow-feed via `drain_lifecycle_tick()`.
//!
//! These tests are the discriminating gate between the M1 hand-rolled path
//! (pre-T140) and the M2 planner path (post-T140). They MUST FAIL before
//! T140 implementation and MUST PASS after.
//!
//! ## What they verify
//!
//! 1. `t140_ingest_contacts_registers_interests_drain_emits_req` — the core
//!    behavioral contract: after `ingest_contacts` is called for the active
//!    account, `drain_lifecycle_tick()` returns ≥1 `WireFrame::Req` aimed at
//!    the follow's resolved NIP-65 write relay.
//!
//! 2. `t140_follow_list_change_rereg_interests_new_relay_appears` — when kind:3
//!    arrives a SECOND time (follow set expands), the M2 registry is updated and
//!    the next drain emits a REQ for the newly-added author's relay.
//!
//! 3. `t140_follow_list_change_withdraws_removed_follow_and_emits_close` — when
//!    a fresher kind:3 removes a follow, the M2 registry drops that author and
//!    the next drain emits the CLOSE diff for the removed author's relay.
//!
//! The `open_timeline` actor-command test lives in `actor/commands/tests.rs`
//! because `actor::commands` is a private module not reachable from kernel/.
//!
//! ## Why they fail pre-T140
//!
//! - `ingest_contacts` enqueues `FollowListChanged` but the M2 registry is
//!   empty; `drain_tick` sees no interests and returns `Vec::new()`.
//!
//! Design note: these tests live inside `nmp-core` (kernel is `pub(crate)`).
//! Integration tests against the public actor surface live in `nmp-testing`.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CAROL: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn install_relay_list(kernel: &Kernel, author: &str, write_relays: &[&str]) {
    kernel.seed_mailbox_relay_list(
        author,
        vec![],
        write_relays.iter().map(|s| s.to_string()).collect(),
        vec![],
    );
}

/// Build a kind:3 tag list where each entry is `["p", pubkey]`.
fn follow_tags(pubkeys: &[&str]) -> Vec<Vec<String>> {
    pubkeys
        .iter()
        .map(|pk| vec!["p".to_string(), pk.to_string()])
        .collect()
}

fn req_relay_urls(frames: &[WireFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect()
}

fn close_relay_urls(frames: &[WireFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Close { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect()
}

// ─── Test 1 ──────────────────────────────────────────────────────────────────

/// Core T140 discriminator: after `ingest_contacts` for the active account's
/// kind:3 follow list, `drain_lifecycle_tick()` must return ≥1 `WireFrame::Req`
/// aimed at the followed author's resolved NIP-65 write relay.
///
/// Pre-T140: the M2 registry is empty → `drain_tick` → `Vec::new()`. FAILS.
/// Post-T140: `ingest_contacts` pushes `LogicalInterest`s into registry →
/// `drain_tick` → REQ frame for `wss://alice-t140.relay/`. PASSES.
#[test]
fn t140_ingest_contacts_registers_interests_drain_emits_req() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Seed the compiled acquisition kinds the app declaration derived. D0: the
    // substrate no longer hardcodes a social kind set.
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);

    // Active account: ALICE.
    kernel.active_account = Some(ALICE.to_string());

    // ALICE has a resolved NIP-65 write relay.
    install_relay_list(&kernel, ALICE, &["wss://alice-t140.relay/"]);

    // Inject kind:3 where ALICE follows herself (minimal follow set; enough to
    // register one interest).
    let tags = follow_tags(&[ALICE]);
    kernel
        .inject_replaceable_event(
            "0000000000000000000000000000000000000000000000000000000000000001",
            ALICE,
            2_000,
            3,
            tags,
            "wss://alice-t140.relay/",
            2_000_000,
        )
        .expect("inject kind:3 must succeed");

    // M2 drain — this is the actor idle-loop call.
    let frames = kernel.drain_lifecycle_tick();

    let req_urls = req_relay_urls(&frames);
    assert!(
        !req_urls.is_empty(),
        "T140: drain_lifecycle_tick must return REQ frames after ingest_contacts \
         registers follow interests (got {} total frames)",
        frames.len(),
    );
    assert!(
        req_urls.iter().any(|u| u == "wss://alice-t140.relay/"),
        "T140: REQ must target ALICE's resolved write relay wss://alice-t140.relay/; \
         got urls: {req_urls:?}"
    );
}

// ─── Test 2 ──────────────────────────────────────────────────────────────────

/// Follow-list change: when the active account's kind:3 arrives a second time
/// with an expanded follow set (BOB added), the M2 registry must be updated
/// and the next `drain_lifecycle_tick()` must emit a REQ for BOB's relay.
///
/// Pre-T140: registry stays empty → no REQs for BOB. FAILS.
/// Post-T140: second `ingest_contacts` replaces the registry entry → drain
/// emits REQ targeting `wss://bob-t140.relay/`. PASSES.
#[test]
fn t140_follow_list_change_rereg_interests_new_relay_appears() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Seed the compiled acquisition kinds the app declaration derived. D0: the
    // substrate no longer hardcodes a social kind set.
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    kernel.active_account = Some(ALICE.to_string());

    install_relay_list(&kernel, ALICE, &["wss://alice-t140.relay/"]);
    install_relay_list(&kernel, BOB, &["wss://bob-t140.relay/"]);

    // First kind:3: ALICE follows herself only.
    kernel
        .inject_replaceable_event(
            "0000000000000000000000000000000000000000000000000000000000000010",
            ALICE,
            2_000,
            3,
            follow_tags(&[ALICE]),
            "wss://alice-t140.relay/",
            2_000_000,
        )
        .expect("first inject kind:3");

    // Drain first emission (consume triggers, establish baseline plan).
    let _ = kernel.drain_lifecycle_tick();

    // Second kind:3: ALICE now follows BOB too.
    kernel
        .inject_replaceable_event(
            "0000000000000000000000000000000000000000000000000000000000000011",
            ALICE,
            3_000,
            3,
            follow_tags(&[ALICE, BOB]),
            "wss://alice-t140.relay/",
            3_000_000,
        )
        .expect("second inject kind:3 (expanded follows)");

    // Second drain — must include a REQ for BOB's relay.
    let frames = kernel.drain_lifecycle_tick();
    let req_urls = req_relay_urls(&frames);

    assert!(
        req_urls.iter().any(|u| u == "wss://bob-t140.relay/"),
        "T140: after follow-list expands to include BOB, drain must emit REQ \
         targeting wss://bob-t140.relay/; got urls: {req_urls:?}"
    );
}

/// Follow-list contraction: when a fresher active-account kind:3 removes
/// CAROL, the M2 registry must drop her from the author set and the lifecycle
/// diff must emit a CLOSE for her relay while retaining BOB.
#[test]
fn t140_follow_list_change_withdraws_removed_follow_and_emits_close() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    kernel.active_account = Some(ALICE.to_string());
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);

    install_relay_list(&kernel, ALICE, &["wss://alice-t140.relay/"]);
    install_relay_list(&kernel, BOB, &["wss://bob-t140.relay/"]);
    install_relay_list(&kernel, CAROL, &["wss://carol-t140.relay/"]);

    kernel
        .inject_replaceable_event(
            "0000000000000000000000000000000000000000000000000000000000000020",
            ALICE,
            2_000,
            3,
            follow_tags(&[ALICE, BOB, CAROL]),
            "wss://alice-t140.relay/",
            2_000_000,
        )
        .expect("first inject kind:3");
    let first_frames = kernel.drain_lifecycle_tick();
    assert!(
        req_relay_urls(&first_frames)
            .iter()
            .any(|u| u == "wss://carol-t140.relay/"),
        "precondition: initial follow list must open CAROL's relay"
    );

    kernel
        .inject_replaceable_event(
            "0000000000000000000000000000000000000000000000000000000000000021",
            ALICE,
            3_000,
            3,
            follow_tags(&[ALICE, BOB]),
            "wss://alice-t140.relay/",
            3_000_000,
        )
        .expect("second inject kind:3 (removed CAROL)");
    let frames = kernel.drain_lifecycle_tick();

    assert!(kernel.timeline_authors_for_test().contains(BOB));
    assert!(!kernel.timeline_authors_for_test().contains(CAROL));
    assert!(
        close_relay_urls(&frames)
            .iter()
            .any(|u| u == "wss://carol-t140.relay/"),
        "removed follow must close CAROL's prior follow-feed sub; got {frames:?}"
    );
}

// ─── Test 4 — #1497 test (a): >500 follows → ONE interest, no cap ─────────────

/// #1497 amendment 5/6 — a follow set well beyond the retired 500-author cap
/// produces a SINGLE multi-author follow-feed interest whose shape covers EVERY
/// followed author plus self. No per-author fan-out, no `TIMELINE_AUTHOR_LIMIT`
/// truncation, no per-author `limit`.
#[test]
fn t140_follow_set_over_500_is_one_uncapped_multi_author_interest() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    kernel.active_account = Some(ALICE.to_string());

    // 600 distinct valid 64-hex follows — comfortably past the retired 500 cap.
    let follows: Vec<String> = (0..600)
        .map(|i| format!("{:016x}{}", i as u64, "0".repeat(48)))
        .collect();

    kernel.sync_follow_feed_interests(&follows);

    // Exactly ONE follow-feed interest (collapsed from the per-author fan-out).
    assert_eq!(
        kernel.follow_feed_interest_ids_for_test().len(),
        1,
        ">500 follows must collapse into ONE follow-feed interest"
    );

    // Its shape carries EVERY follow plus self — uncapped.
    let authors = kernel
        .follow_feed_interest_authors_for_test()
        .expect("the single follow-feed interest must be registered");
    assert_eq!(
        authors.len(),
        601,
        "the one interest must cover all 600 follows + self (uncapped, #1497)"
    );
    for pk in &follows {
        assert!(
            authors.contains(pk),
            "every follow must be in the single interest's author set: {pk}"
        );
    }
    assert!(
        authors.contains(ALICE),
        "the active account itself must be in the interest's author set"
    );
}
