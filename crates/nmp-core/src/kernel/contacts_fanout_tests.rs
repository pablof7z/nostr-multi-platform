//! T100 part-2 integration tests — kind:3 follow-list arrival drives a
//! timeline re-fan-out onto the newly-followed authors' NIP-65 write relays.
//!
//! Mirrors the kind:10002 recompilation trigger pattern that lives in
//! `outbox_tests::cold_start_routes_to_bootstrap_then_replans_after_nip65_arrives`,
//! but for kind:3 ingest: when the follow set expands, the next
//! `maybe_open_timeline()` emission must include REQ frames targeted at the
//! resolved write relays of every author in the *new* follow set, not just the
//! pre-existing seed.
//!
//! Test posture per T100 spec:
//! - Seed kernel with author A only (with A's kind:10002 write relay `R_A`).
//! - Inject kind:10002 for B and C (distinct write relays `R_B`, `R_C`).
//! - Inject kind:3 from the active account listing `[A, B, C]`.
//! - Drain `maybe_open_timeline()` and assert REQs land on `R_A`, `R_B`, `R_C`.
//!
//! Why mirror kind:10002, not the `CompileTrigger::FollowListChanged` lifecycle
//! seam: per the comment on `ingest_contacts` (contacts.rs lines 21-25), the
//! compile / registry machinery is dormant until M11 migrates view modules onto
//! `LogicalInterest`. Until then, the direct flip-`timeline_requested = false`
//! pattern that kind:10002 already uses (`ingest_relay_list` lines 71-86) is
//! the production seam.

use super::*;
use crate::kernel::types::AuthorRelayList;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CAROL: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn install_relay_list(kernel: &mut Kernel, author: &str, write: &[&str]) {
    kernel.author_relay_lists.insert(
        author.to_string(),
        AuthorRelayList {
            event_id: "x".to_string(),
            created_at: 1,
            read_relays: vec![],
            write_relays: write.iter().map(|s| s.to_string()).collect(),
            both_relays: vec![],
        },
    );
}

/// T100 part-2 desired behavior: when the active account's kind:3 lands and
/// expands the follow set to include authors with cached kind:10002 write
/// relays, the next emission must fan REQ frames out onto those relays.
///
/// Today this test exercises `ingest_contacts` directly (the same path the
/// echo-back of a self-publish takes through `handle_event`). The kind:3 seed
/// already lists `[ALICE]`; the new kind:3 expands to `[ALICE, BOB, CAROL]`.
/// Each of B and C has a cached kind:10002 with a distinct write relay.
///
/// Status: per the kernel as of HB48, `ingest_contacts` updates
/// `seed_contacts` but does NOT flip `timeline_requested = false`, so the
/// `maybe_open_timeline` short-circuit on line 225 of `ingest/timeline.rs`
/// keeps returning only `pending_profile_claim_requests()`. This test
/// currently FAILS (empirically verified — see
/// `docs/perf/m11/t100-status.md` for the captured failure trace).
///
/// `#[ignore]` is the right posture here: this is an executable spec for the
/// future fix (T100/P2 residual), not a regression. The fix is small (mirror
/// the kind:10002 direct-flip pattern in `ingest_relay_list` lines 71-86)
/// but lives outside this audit's scope. When the fix lands, the implementor
/// removes `#[ignore]` and the test becomes the green sentinel.
#[test]
#[ignore = "T100/P2 residual: kind:3 ingest does not yet re-fan-out the timeline; \
            fix proposal in docs/perf/m11/t100-status.md"]
fn kind3_arrival_fans_out_timeline_onto_new_follows_write_relays() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Active account: ALICE. Bind so ingest_contacts knows whose follows
    // matter (the kernel today keys seed_contacts by event.pubkey regardless
    // of active account — the fix proposal must respect the active-account
    // gate so we don't fan-out on arbitrary peers' kind:3 events).
    kernel.active_account = Some(ALICE.to_string());

    // Cache NIP-65 write relays for all three authors.
    install_relay_list(&mut kernel, ALICE, &["wss://alice.write/"]);
    install_relay_list(&mut kernel, BOB, &["wss://bob.write/"]);
    install_relay_list(&mut kernel, CAROL, &["wss://carol.write/"]);

    // Seed the first follow set: ALICE follows herself only.
    kernel.seed_contacts.insert(ALICE.to_string(), vec![ALICE.to_string()]);
    // Force the open-timeline gate via the contacts-deadline path.
    kernel.contacts_deadline = Some(Instant::now() - Duration::from_secs(60));

    // First emission: only ALICE's relay is in play.
    let first = kernel.maybe_open_timeline();
    let first_timeline: Vec<_> = first
        .iter()
        .filter(|r| r.text.contains("seed-timeline"))
        .collect();
    let first_urls: std::collections::BTreeSet<String> =
        first_timeline.iter().map(|r| r.relay_url.clone()).collect();
    assert!(
        first_urls.contains("wss://alice.write/"),
        "first emission must cover alice's resolved write relay; got {first_urls:?}"
    );
    assert!(
        kernel.timeline_requested,
        "first maybe_open_timeline must flip timeline_requested to true"
    );

    // ── Kind:3 arrival: ALICE re-publishes her follow list, now including
    // BOB and CAROL. This is the same code path a self-publish echo-back
    // takes; using inject_replaceable_event mirrors `handle_event` for kind:3
    // without re-deriving signatures.
    let new_follows_tags = vec![
        vec!["p".to_string(), ALICE.to_string()],
        vec!["p".to_string(), BOB.to_string()],
        vec!["p".to_string(), CAROL.to_string()],
    ];
    let outcome = kernel
        .inject_replaceable_event(
            "1111111111111111111111111111111111111111111111111111111111111111",
            ALICE,
            3_000,
            3,
            new_follows_tags,
            "wss://alice.write/",
            3_000_000,
        )
        .expect("inject kind:3 must succeed");
    use crate::store::InsertOutcome;
    assert!(matches!(
        outcome,
        InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }
    ));

    // Sanity: seed_contacts was updated.
    assert_eq!(
        kernel.seed_contacts.get(ALICE).map(|v| v.len()),
        Some(3),
        "kind:3 ingest must record all three followees"
    );

    // ── Desired behavior: the next emission re-plans onto B and C's
    // resolved write relays. This is what the M10.5 N-star promises for an
    // active-account follow change.
    let second = kernel.maybe_open_timeline();
    let second_timeline: Vec<_> = second
        .iter()
        .filter(|r| r.text.contains("seed-timeline"))
        .collect();
    let second_urls: std::collections::BTreeSet<String> =
        second_timeline.iter().map(|r| r.relay_url.clone()).collect();

    assert!(
        second_urls.contains("wss://bob.write/"),
        "T100/P2: post-kind:3 emission must route to BOB's resolved write \
         relay (he was just added to the follow set); got urls = {second_urls:?}"
    );
    assert!(
        second_urls.contains("wss://carol.write/"),
        "T100/P2: post-kind:3 emission must route to CAROL's resolved write \
         relay (she was just added to the follow set); got urls = {second_urls:?}"
    );
}
