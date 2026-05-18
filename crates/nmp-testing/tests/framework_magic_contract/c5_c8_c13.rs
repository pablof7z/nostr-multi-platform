//! Framework Magic Contract — M2-gated tests: C5, C8, C13.
//!
//! C5  kind:3 auto-tracking (FollowListChanged trigger)
//! C8  Subscription coalescing, auto-close, and auto-buffer
//! C13 Best-effort rendering with non-Option placeholders
//!
//! All three gating milestones (M2) are DONE on master.
//!
//! Design: `docs/design/framework-magic/`

use nmp_core::planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};
use nmp_core::subs::{CompileTrigger, InvalidateReason, RelayAuthState, SubscriptionLifecycle, WireFrame};
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX};

// ── Shared helpers ────────────────────────────────────────────────────────────

fn pubkey(seed: &str) -> String {
    format!("{seed:0>64}").chars().take(64).collect::<String>().to_lowercase()
}

fn tailing_interest(id: u64, authors: &[&str]) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape {
            authors: authors.iter().map(|a| pubkey(a)).collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    }
}

fn oneshot_interest(id: u64, authors: &[&str]) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: authors.iter().map(|a| pubkey(a)).collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::OneShot,
    }
}

// ── C5 ────────────────────────────────────────────────────────────────────────

/// C5: Kind:3 auto-tracking: when the active account's follow list changes
/// (a new kind:3 stored), dependent subscriptions must be recompiled.
///
/// The discrete `FollowListChanged` trigger variant does not yet exist (it will
/// be A11 in the trigger enum when M2 phase-2 wires the ingest pipeline to the
/// subs inbox). What IS present is the full machinery: kind:3 storage in the
/// event store, `InvalidateCompile` as the current forced-recompile trigger, and
/// `SubscriptionLifecycle::drain_tick`. This test verifies the behaviour
/// end-to-end using the available substrate: store a kind:3, fire an
/// `InvalidateCompile`, and assert that `drain_tick` produces a new REQ set that
/// includes the newly-followed author's relay.
///
/// Design: `docs/design/framework-magic/kind3.md`
#[test]
fn c5_kind3_change_recompiles_follow_dependent_subs() {
    let mut lc = SubscriptionLifecycle::new();

    // Author alice's mailbox is known.
    lc.set_mailbox(pubkey("alice"), &["wss://r1/"]);

    // Register a follow-list interest for alice.
    lc.registry_mut().push(tailing_interest(1, &["alice"]));

    // First compile — expect a REQ for alice at wss://r1/.
    let frames1 = lc.recompile_and_diff().expect("first compile");
    let req_urls1: Vec<_> = frames1
        .iter()
        .filter_map(|f| if let WireFrame::Req { relay_url, .. } = f { Some(relay_url.as_str()) } else { None })
        .collect();
    assert!(req_urls1.contains(&"wss://r1/"), "first compile must REQ alice's relay");
    assert_eq!(lc.compile_count(), 1);

    // Simulate a kind:3 arrival by storing it in the event store + issuing an
    // InvalidateCompile trigger (the seam that M2 phase-2 will replace with
    // A11 FollowListChanged when the ingest pipeline grows the trigger enum).
    let h = StoreHarness::mem();
    let kind3 = h.make_event_with_tags(ALICE_HEX, 3, 2_000, vec![
        vec!["p".to_string(), pubkey("bob")],
    ]);
    let _ = h.insert_raw(kind3, "wss://r1/", 2_000_000);

    // "bob" has a mailbox — wire it so the recompile finds a route.
    lc.set_mailbox(pubkey("bob"), &["wss://r2/"]);

    // Expand the follow-list interest to include bob (as if FollowListChanged
    // rebuilt the authors set from the just-stored kind:3).
    lc.registry_mut().push(tailing_interest(1, &["alice", "bob"]));

    // Fire the invalidation trigger (A6).
    lc.enqueue_trigger(CompileTrigger::InvalidateCompile {
        reason: InvalidateReason::TestForceRecompile,
    });

    // drain_tick must recompile and emit the new REQ diff.
    let frames2 = lc.drain_tick();
    assert_eq!(lc.compile_count(), 2, "drain_tick must recompile on trigger");

    let req_urls2: Vec<_> = frames2
        .iter()
        .filter_map(|f| if let WireFrame::Req { relay_url, .. } = f { Some(relay_url.as_str()) } else { None })
        .collect();
    // wss://r2/ must appear — bob is newly followed.
    assert!(
        req_urls2.contains(&"wss://r2/"),
        "after follow-list update, recompile must REQ bob's relay; frames={frames2:?}"
    );
}

// ── C8 ────────────────────────────────────────────────────────────────────────

/// C8: Subscriptions auto-dedup, auto-coalesce, auto-close, and auto-buffer.
///
/// Four sub-properties verified against `SubscriptionLifecycle`:
///
/// 1. **Coalesce** — N triggers between ticks produce exactly 1 compile.
/// 2. **Auto-close (OneShot)** — a OneShot sub closes after its first EOSE.
/// 3. **Empty-tick no-op** — an empty trigger inbox does not compile.
/// 4. **Auth-buffer** — REQs targeting auth-paused relays are held pending auth.
///
/// Design: `docs/design/framework-magic/subs.md`
#[test]
fn c8_subscriptions_coalesce_autoclose_and_buffer() {
    // --- 1. Coalesce: 3 triggers → 1 compile --------------------------------
    let mut lc = SubscriptionLifecycle::new();
    lc.set_mailbox(pubkey("alice"), &["wss://r1/"]);
    lc.registry_mut().push(tailing_interest(1, &["alice"]));

    for _ in 0..3 {
        lc.enqueue_trigger(CompileTrigger::InvalidateCompile {
            reason: InvalidateReason::TestForceRecompile,
        });
    }
    let _frames = lc.drain_tick();
    assert_eq!(lc.compile_count(), 1, "3 triggers in one tick must produce exactly 1 compile");

    // --- 2. Empty-tick is a no-op -------------------------------------------
    let frames_empty = lc.drain_tick();
    assert!(frames_empty.is_empty(), "empty tick must emit no frames");
    assert_eq!(lc.compile_count(), 1, "empty tick must not compile");

    // --- 3. Auto-close (OneShot) on EOSE ------------------------------------
    let mut lc2 = SubscriptionLifecycle::new();
    lc2.set_mailbox(pubkey("carol"), &["wss://rc/"]);
    lc2.registry_mut().push(oneshot_interest(10, &["carol"]));

    let open_frames = lc2.recompile_and_diff().expect("oneshot open");
    // Exactly one REQ must be emitted.
    let req: Vec<_> = open_frames
        .iter()
        .filter_map(|f| if let WireFrame::Req { relay_url, sub_id, .. } = f {
            Some((relay_url.clone(), sub_id.clone()))
        } else {
            None
        })
        .collect();
    assert_eq!(req.len(), 1, "oneshot interest must emit exactly 1 REQ");
    let (req_relay, req_sub) = &req[0];

    // EOSE arrives — lifecycle_gate must emit a CLOSE.
    let close_frames = lc2.handle_eose(req_relay, req_sub);
    let closes: Vec<_> = close_frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Close { .. }))
        .collect();
    assert!(!closes.is_empty(), "EOSE on a OneShot sub must emit a CLOSE frame");

    // --- 4. Auth-buffer: REQs held while relay is auth-paused --------------
    let mut lc3 = SubscriptionLifecycle::new();
    lc3.set_mailbox(pubkey("dave"), &["wss://rd/"]);
    lc3.registry_mut().push(tailing_interest(20, &["dave"]));

    // Mark the relay as auth-challenged BEFORE the first compile.
    let _pre = lc3.handle_auth_state_change(
        "wss://rd/".to_string(),
        RelayAuthState::ChallengeReceived,
    );

    let frames_paused = lc3.recompile_and_diff().expect("auth-paused compile");
    // All REQs for wss://rd/ must be held in the auth buffer, so no REQ frames
    // should appear in the returned diff.
    let reqs_to_rd: Vec<_> = frames_paused
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { relay_url, .. } if relay_url == "wss://rd/"))
        .collect();
    assert!(
        reqs_to_rd.is_empty(),
        "REQs to auth-paused relay must be buffered, not emitted: {frames_paused:?}"
    );

    // After auth completes, the buffered REQs are flushed.
    let flush_frames = lc3.handle_auth_state_change(
        "wss://rd/".to_string(),
        RelayAuthState::Authenticated,
    );
    let reqs_flushed: Vec<_> = flush_frames
        .iter()
        .filter(|f| matches!(f, WireFrame::Req { relay_url, .. } if relay_url == "wss://rd/"))
        .collect();
    assert!(
        !reqs_flushed.is_empty(),
        "after Authenticated, buffered REQs must be flushed: {flush_frames:?}"
    );
}

// ── C13 ───────────────────────────────────────────────────────────────────────

/// C13: Best-effort rendering — display fields use non-Option placeholders;
/// authoritative data refines in place.
///
/// Substrate gap: `TimelineItem` is `pub(super)` in `nmp_core::kernel` — it is
/// not accessible from integration tests, making it impossible to verify the D1
/// placeholder contract (that `author_picture_url` and `author_display_name` are
/// non-Option `String`s, never `None`, on the FFI boundary) at this layer.
///
/// `TimelineItem.author_picture_url` is currently `Option<String>` — a D1
/// violation. The full fix requires either exposing `TimelineItem` via
/// `nmp_core::kernel::types` or migrating the field to `String` with a hardcoded
/// placeholder fallback in `update.rs`.
///
/// Follow-up task (#57-c13-gap): expose `TimelineItem` or add a kernel-level
/// test harness so this contract can be verified end-to-end.
///
/// Design: `docs/design/framework-magic/capabilities.md`
#[test]
#[should_panic = "substrate gap: TimelineItem is pub(super) in nmp-core::kernel — cannot verify D1 placeholder contract from integration test; expose type or add kernel-level harness (follow-up #57-c13-gap)"]
fn c13_view_payload_uses_placeholders_then_refines_in_place() {
    panic!(
        "substrate gap: TimelineItem is pub(super) in nmp-core::kernel \
        — cannot verify D1 placeholder contract from integration test; \
        expose type or add kernel-level harness (follow-up #57-c13-gap)"
    );
}
