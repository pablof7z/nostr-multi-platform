//! T140 fix-forward RED tests — proof of M1 follow-feed *retirement*.
//!
//! The original T140 claimed "M1 hand-rolled req() retired / M2 is the live
//! follow-feed hot path". A codex post-merge review proved the cutover never
//! happened: `maybe_open_timeline()` still emitted `seed-timeline-*` REQs and
//! M2 `sub-*` subs auto-CLOSEd after first EOSE. These tests are the
//! discriminating gate that the prior agent's "done" claim could not pass.
//!
//! ## What they assert (negative existence — the gap that hid the bug)
//!
//! - [`live_follow_feed_path_emits_no_seed_timeline_req`] — driving the
//!   *live* follow-feed path (a kind:3 EVENT frame for the active account
//!   through `handle_text`, which internally runs `ingest_contacts` AND the
//!   `maybe_open_timeline()` tail) MUST NOT produce any frame whose sub-id
//!   starts with `seed-timeline-`. Proves retirement, not merely M2 addition.
//!
//! - [`m2_follow_feed_sub_survives_eose`] — an M2 follow-feed `sub-*`
//!   subscription registered via `drain_lifecycle_tick()` MUST stay `live`
//!   after an EOSE frame (no auto-CLOSE), at parity with the old
//!   `seed-timeline-*` keep-alive behaviour.
//!
//! - [`m2_follow_feed_interest_carries_limit`] — the M2 follow-feed REQ must
//!   carry `"limit":200` (parity with the retired M1 REQ; no unbounded
//!   backfill).
//!
//! - [`empty_follows_clears_timeline_authors_and_interests`] — an active
//!   account with no cached follows must CLEAR stale follow-feed interest ids
//!   and follow-derived `timeline_authors`, not no-op.
//!
//! - [`empty_kind_10002_emits_nip65_arrived`] — an empty (cleared) kind:10002
//!   must still fan a `Nip65Arrived` trigger so M2 re-routes off stale relays.

use super::*;
use crate::kernel::types::AuthorRelayList;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::WireFrame;

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn install_relay_list(kernel: &mut Kernel, author: &str, write_relays: &[&str]) {
    kernel.author_relay_lists.insert(
        author.to_string(),
        AuthorRelayList {
            event_id: format!("relay-list-{author}"),
            created_at: 1_000,
            read_relays: vec![],
            write_relays: write_relays.iter().map(|s| s.to_string()).collect(),
            both_relays: vec![],
        },
    );
}

/// A kind:3 EVENT relay frame string where the author follows `follows`.
fn kind3_event_frame(sub_id: &str, id: &str, author: &str, created_at: u64, follows: &[&str]) -> String {
    let tags: Vec<Vec<String>> = follows
        .iter()
        .map(|pk| vec!["p".to_string(), pk.to_string()])
        .collect();
    let event = serde_json::json!({
        "id": id,
        "pubkey": author,
        "created_at": created_at,
        "kind": 3,
        "tags": tags,
        "content": "",
        "sig": "a".repeat(128),
    });
    serde_json::json!(["EVENT", sub_id, event]).to_string()
}

/// All sub-ids appearing in `REQ` outbound frames (M1 OutboundMessage form).
fn req_sub_ids_from_outbound(out: &[crate::relay::OutboundMessage]) -> Vec<String> {
    out.iter()
        .filter_map(|m| {
            let v: serde_json::Value = serde_json::from_str(&m.text).ok()?;
            let arr = v.as_array()?;
            if arr.first()?.as_str()? != "REQ" {
                return None;
            }
            Some(arr.get(1)?.as_str()?.to_string())
        })
        .collect()
}

// ─── Retirement gate (the discriminator) ─────────────────────────────────────

/// Driving the LIVE follow-feed path (active-account kind:3 through
/// `handle_text`, which runs `ingest_contacts` + the `maybe_open_timeline()`
/// tail) MUST NOT emit any `seed-timeline-*` REQ.
///
/// Pre-fix: `maybe_open_timeline()` is still active and emits
/// `seed-timeline-<hash>` → this assertion FAILS.
/// Post-fix: the M1 follow-feed REQ emission is retired → no `seed-timeline-*`
/// frame is produced from the live path → PASSES.
#[test]
fn live_follow_feed_path_emits_no_seed_timeline_req() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ALICE.to_string());
    install_relay_list(&mut kernel, ALICE, &["wss://alice-t140.relay/"]);
    install_relay_list(&mut kernel, BOB, &["wss://bob-t140.relay/"]);

    // Force `should_open_timeline()` to be satisfied by tripping the
    // contacts deadline (the M1 gate the prior agent left active).
    kernel.contacts_deadline = Some(Instant::now() - Duration::from_secs(1));

    // Drive the LIVE path: a kind:3 EVENT for the active account through
    // handle_text. This runs ingest_contacts AND the maybe_open_timeline()
    // tail in handle_text — exactly the production hot path.
    let frame = kind3_event_frame(
        "seed-contacts",
        "0000000000000000000000000000000000000000000000000000000000000001",
        ALICE,
        2_000,
        &[ALICE, BOB],
    );
    let outbound = kernel.handle_message(
        crate::relay::RelayRole::Indexer,
        "wss://indexer.relay/",
        Message::Text(frame),
    );

    // Also drain the M2 lifecycle tick (the actor idle loop call).
    let m2_frames = kernel.drain_lifecycle_tick();

    let m1_sub_ids = req_sub_ids_from_outbound(&outbound);
    let seed_timeline_emitted: Vec<&String> = m1_sub_ids
        .iter()
        .filter(|s| s.starts_with("seed-timeline-") || s.as_str() == "seed-timeline")
        .collect();
    assert!(
        seed_timeline_emitted.is_empty(),
        "T140 RETIREMENT: the live follow-feed path must emit NO seed-timeline-* \
         REQ (M1 retired). Got seed-timeline ids: {seed_timeline_emitted:?}; \
         all M1 outbound REQ ids: {m1_sub_ids:?}"
    );

    // Positive parity: the M2 planner must carry the follow feed instead.
    let m2_req_count = m2_frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { .. }))
        .count();
    assert!(
        m2_req_count > 0,
        "T140 RETIREMENT: with M1 retired, M2 drain_lifecycle_tick must carry \
         the follow feed (expected ≥1 WireFrame::Req, got {m2_req_count})"
    );
}

// ─── EOSE keep-live parity ───────────────────────────────────────────────────

/// An M2 follow-feed `sub-*` subscription must survive an EOSE frame (stay
/// `live`), at parity with the retired `seed-timeline-*` keep-alive.
///
/// Pre-fix: the EOSE keep-live predicate only recognizes `seed-timeline*` /
/// `diag-firehose-*` / persistent ids, so the `sub-*` follow-feed sub is
/// auto-CLOSEd after first EOSE → assertion FAILS.
/// Post-fix: `Tailing` M2 subs are registered persistent at emit time → the
/// sub stays `live` after EOSE → PASSES.
#[test]
fn m2_follow_feed_sub_survives_eose() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ALICE.to_string());
    install_relay_list(&mut kernel, ALICE, &["wss://alice-t140.relay/"]);

    // Register M2 follow-feed interests and emit the REQ via the actor path.
    kernel
        .inject_replaceable_event(
            "0000000000000000000000000000000000000000000000000000000000000002",
            ALICE,
            2_000,
            3,
            vec![vec!["p".to_string(), ALICE.to_string()]],
            "wss://alice-t140.relay/",
            2_000_000,
        )
        .expect("inject kind:3");
    let frames = kernel.drain_lifecycle_tick();

    // Find the M2 follow-feed sub-id (planner emits `sub-<hash>`).
    let sub_id = frames
        .iter()
        .find_map(|f| match f {
            WireFrame::Req { sub_id, .. } => Some(sub_id.clone()),
            _ => None,
        })
        .expect("M2 drain must emit a follow-feed REQ");

    // Register the wire frames so the kernel tracks the sub (actor wiring).
    kernel.register_wire_frames_for_test(&frames);

    // Relay answers EOSE for that sub.
    let eose = serde_json::json!(["EOSE", sub_id]).to_string();
    kernel.handle_message(
        crate::relay::RelayRole::Content,
        "wss://alice-t140.relay/",
        Message::Text(eose),
    );

    let state = kernel.wire_sub_state_for_test(&sub_id);
    assert_eq!(
        state.as_deref(),
        Some("live"),
        "T140: M2 follow-feed sub {sub_id} must stay `live` after EOSE \
         (keep-alive parity with retired seed-timeline-*); got state {state:?}"
    );
}

// ─── limit parity (no unbounded backfill) ────────────────────────────────────

/// The M2 follow-feed REQ must carry `"limit":200` — parity with the retired
/// M1 `{"kinds":[1,6],"authors":[...],"limit":200}` REQ.
#[test]
fn m2_follow_feed_interest_carries_limit() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ALICE.to_string());
    install_relay_list(&mut kernel, ALICE, &["wss://alice-t140.relay/"]);
    kernel
        .inject_replaceable_event(
            "0000000000000000000000000000000000000000000000000000000000000003",
            ALICE,
            2_000,
            3,
            vec![vec!["p".to_string(), ALICE.to_string()]],
            "wss://alice-t140.relay/",
            2_000_000,
        )
        .expect("inject kind:3");
    let frames = kernel.drain_lifecycle_tick();

    let filter = frames
        .iter()
        .find_map(|f| match f {
            WireFrame::Req { filter_json, .. } => Some(filter_json.clone()),
            _ => None,
        })
        .expect("M2 drain must emit a follow-feed REQ");
    assert!(
        filter.contains("\"limit\":200"),
        "T140: M2 follow-feed REQ must carry limit:200 (parity with retired M1 \
         REQ; no unbounded backfill). Got filter: {filter}"
    );
}

// ─── empty-follows clears stale state ────────────────────────────────────────

/// `register_follow_feed_for_active_account()` with no cached follows must
/// CLEAR stale follow-feed interest ids and follow-derived `timeline_authors`,
/// not no-op (codex finding #4).
#[test]
fn empty_follows_clears_timeline_authors_and_interests() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ALICE.to_string());
    install_relay_list(&mut kernel, ALICE, &["wss://alice-t140.relay/"]);

    // Establish a non-empty follow set first.
    kernel
        .inject_replaceable_event(
            "0000000000000000000000000000000000000000000000000000000000000004",
            ALICE,
            2_000,
            3,
            vec![vec!["p".to_string(), BOB.to_string()]],
            "wss://alice-t140.relay/",
            2_000_000,
        )
        .expect("inject kind:3");
    assert!(
        !kernel.follow_feed_interest_ids_for_test().is_empty(),
        "precondition: follow interests registered"
    );
    assert!(
        kernel.timeline_authors_for_test().contains(BOB),
        "precondition: BOB is a follow-derived timeline author"
    );

    // Now the active account switches to one with NO cached follows.
    kernel.seed_contacts.remove(ALICE);
    kernel.register_follow_feed_for_active_account();

    assert!(
        kernel.follow_feed_interest_ids_for_test().is_empty(),
        "T140: empty/no-follows must CLEAR stale follow_feed_interest_ids"
    );
    assert!(
        !kernel.timeline_authors_for_test().contains(BOB),
        "T140: empty/no-follows must drop stale follow-derived timeline_authors \
         (BOB must no longer be present)"
    );
}

// ─── empty kind:10002 still re-routes M2 ─────────────────────────────────────

/// An empty (cleared) kind:10002 must still fan a `Nip65Arrived` trigger so M2
/// re-routes off stale relays (codex finding #5).
///
/// Behavioral assertion: with a follow-feed interest for ALICE routed to her
/// (now stale) write relay, clearing her kind:10002 must cause the next
/// `drain_lifecycle_tick()` to recompile and emit a CLOSE for the stale-relay
/// sub. Pre-fix the empty-list branch `return`s without enqueuing a trigger,
/// so no recompile happens and the stale sub survives → FAILS.
#[test]
fn empty_kind_10002_emits_nip65_arrived() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ALICE.to_string());

    // Prime a cached relay list and register ALICE's follow-feed interest so a
    // plan exists routed to wss://stale.relay/.
    install_relay_list(&mut kernel, ALICE, &["wss://stale.relay/"]);
    kernel
        .inject_replaceable_event(
            "0000000000000000000000000000000000000000000000000000000000000050",
            ALICE,
            2_000,
            3,
            vec![vec!["p".to_string(), ALICE.to_string()]],
            "wss://stale.relay/",
            2_000_000,
        )
        .expect("inject kind:3");
    let baseline = kernel.drain_lifecycle_tick();
    assert!(
        baseline.iter().any(|f| matches!(
            f,
            WireFrame::Req { relay_url, .. } if relay_url == "wss://stale.relay/"
        )),
        "precondition: plan routed to wss://stale.relay/"
    );

    // ALICE clears her kind:10002 (empty relay list).
    kernel
        .inject_replaceable_event(
            "0000000000000000000000000000000000000000000000000000000000000051",
            ALICE,
            5_000,
            10002,
            vec![], // empty relay list → author cleared NIP-65
            "wss://indexer.relay/",
            5_000_000,
        )
        .expect("inject empty kind:10002");

    // The empty kind:10002 must have enqueued Nip65Arrived → this drain
    // recompiles. The stale-relay sub must be CLOSEd (re-routed off stale).
    let after = kernel.drain_lifecycle_tick();
    let closed_stale = after.iter().any(|f| matches!(
        f,
        WireFrame::Close { relay_url, .. } if relay_url == "wss://stale.relay/"
    ));
    assert!(
        closed_stale,
        "T140: empty kind:10002 must enqueue Nip65Arrived so the next drain \
         recompiles and CLOSEs the stale-relay follow-feed sub; got {after:?}"
    );
}
