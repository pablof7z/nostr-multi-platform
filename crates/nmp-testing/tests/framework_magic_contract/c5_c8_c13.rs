//! Framework Magic Contract — M2-gated tests: C5, C8, C13.
//!
//! C5  kind:3 auto-tracking (FollowListChanged trigger)
//! C8  Subscription coalescing, auto-close, and auto-buffer
//! C13 Best-effort rendering with non-Option placeholders
//!
//! All three gating milestones (M2) are DONE on master.
//!
//! Design: `docs/design/framework-magic/`

use nmp_core::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};
use nmp_core::subs::{AccountId, CompileTrigger, InvalidateReason, RelayAuthState, SubscriptionLifecycle, WireFrame};
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX};

/// T132 helper — populate a mailbox cache with one (author, write_relays) pair.
fn put_mailbox(cache: &mut InMemoryMailboxCache, author: &str, write_relays: &[&str]) {
    cache.put(
        pubkey(author),
        MailboxSnapshot {
            write_relays: write_relays.iter().map(|r| r.to_string()).collect(),
            read_relays: vec![],
            both_relays: vec![],
        },
    );
}

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
/// `FollowListChanged` (A11) is now live in the trigger enum — this test uses
/// it directly instead of the `InvalidateCompile` placeholder. The registry
/// push that expands the author set to include bob is still synthetic: in
/// production a ViewModule rebuilds its authors set from the follow-set; that
/// wiring lands in M11 when ViewModules migrate onto `LogicalInterest`. What
/// IS real here: the trigger variant, the ingest fan from `ingest_contacts`,
/// and `drain_tick` routing into the compiler.
///
/// Design: `docs/design/framework-magic/kind3.md`
#[test]
fn c5_kind3_change_recompiles_follow_dependent_subs() {
    let mut lc = SubscriptionLifecycle::new();

    // T132: the lifecycle no longer owns the mailbox cache. The test owns one
    // and passes it in; in production the kernel passes its `KernelMailboxes`
    // adapter (a borrow of `author_relay_lists`).
    let mut mailboxes = InMemoryMailboxCache::new();

    // Author alice's mailbox is known.
    put_mailbox(&mut mailboxes, "alice", &["wss://r1/"]);

    // Register a follow-list interest for alice.
    lc.registry_mut().push(tailing_interest(1, &["alice"]));

    // First compile — expect a REQ for alice at wss://r1/.
    let frames1 = lc.recompile_and_diff(&mailboxes).expect("first compile");
    let req_urls1: Vec<_> = frames1
        .iter()
        .filter_map(|f| if let WireFrame::Req { relay_url, .. } = f { Some(relay_url.as_str()) } else { None })
        .collect();
    assert!(req_urls1.contains(&"wss://r1/"), "first compile must REQ alice's relay");
    assert_eq!(lc.compile_count(), 1);

    // Store the kind:3 in the harness (exercises the store path the real
    // ingest fan calls before emitting the trigger).
    let h = StoreHarness::mem();
    let kind3 = h.make_event_with_tags(ALICE_HEX, 3, 2_000, vec![
        vec!["p".to_string(), pubkey("bob")],
    ]);
    let _ = h.insert_raw(kind3, "wss://r1/", 2_000_000);

    // "bob" has a mailbox — wire it so the recompile finds a route.
    put_mailbox(&mut mailboxes, "bob", &["wss://r2/"]);

    // Expand the follow-list interest to include bob (synthetic stand-in for
    // the M11 ViewModule rebuild; the trigger does not rewrite registry
    // entries — that is the ViewModule's responsibility).
    lc.registry_mut().push(tailing_interest(1, &["alice", "bob"]));

    // Fire the real A11 FollowListChanged trigger (replaces the old A6
    // InvalidateCompile placeholder used before this variant existed).
    lc.enqueue_trigger(CompileTrigger::FollowListChanged {
        account_id: AccountId(pubkey("alice")),
        new_follows: vec![pubkey("bob")],
    });

    // drain_tick must recompile and emit the new REQ diff.
    let frames2 = lc.drain_tick(&mailboxes);
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
    let mut mailboxes = InMemoryMailboxCache::new();
    put_mailbox(&mut mailboxes, "alice", &["wss://r1/"]);
    lc.registry_mut().push(tailing_interest(1, &["alice"]));

    for _ in 0..3 {
        lc.enqueue_trigger(CompileTrigger::InvalidateCompile {
            reason: InvalidateReason::TestForceRecompile,
        });
    }
    let _frames = lc.drain_tick(&mailboxes);
    assert_eq!(lc.compile_count(), 1, "3 triggers in one tick must produce exactly 1 compile");

    // --- 2. Empty-tick is a no-op -------------------------------------------
    let frames_empty = lc.drain_tick(&mailboxes);
    assert!(frames_empty.is_empty(), "empty tick must emit no frames");
    assert_eq!(lc.compile_count(), 1, "empty tick must not compile");

    // --- 3. Auto-close (OneShot) on EOSE ------------------------------------
    let mut lc2 = SubscriptionLifecycle::new();
    let mut mailboxes2 = InMemoryMailboxCache::new();
    put_mailbox(&mut mailboxes2, "carol", &["wss://rc/"]);
    lc2.registry_mut().push(oneshot_interest(10, &["carol"]));

    let open_frames = lc2.recompile_and_diff(&mailboxes2).expect("oneshot open");
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
    let mut mailboxes3 = InMemoryMailboxCache::new();
    put_mailbox(&mut mailboxes3, "dave", &["wss://rd/"]);
    lc3.registry_mut().push(tailing_interest(20, &["dave"]));

    // Mark the relay as auth-challenged BEFORE the first compile.
    let _pre = lc3.handle_auth_state_change(
        "wss://rd/".to_string(),
        RelayAuthState::ChallengeReceived,
    );

    let frames_paused = lc3
        .recompile_and_diff(&mailboxes3)
        .expect("auth-paused compile");
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

/// C13: Best-effort rendering — `author_picture_url` is a non-`Option` `String`
/// on the JSON wire format; authoritative data refines in place.
///
/// Integration proof of TWO contracts in one drain:
///
/// 1. **ADR-0017 (D1 placeholder shape).** With no kind:0 ingested, the
///    timeline item's `author_picture_url` is the deterministic
///    `identicon:<pubkey-prefix>` URI and `author_avatar_source` is
///    `"placeholder"` (the discriminator tracks the actual URL selection).
/// 2. **ADR-0001 / T103 (FFI envelope).** Every frame on the channel decodes
///    as the single `UpdateEnvelope` discriminated type — the tag *is* the
///    discriminant (D6).  This test never sniffs payload keys to decide
///    snapshot-vs-update; it pattern-matches on `UpdateEnvelope::Snapshot`.
///
/// In-place refinement (placeholder → kind:0 URL) is covered by the kernel
/// companion `c13_kernel_*` in `kernel/tests.rs`, per the ADR-0017 split.
///
/// Design: `docs/product-spec/doctrine.md` §D1, ADR-0017,
///         `docs/design/0001-ffi-update-channel-envelope.md` (T103).
#[test]
fn c13_view_payload_uses_placeholders_then_refines_in_place() {
    use nmp_core::store::RawEvent;
    use nmp_core::testing::{spawn_actor, ActorCommand};
    use nmp_core::UpdateEnvelope;
    use std::time::Duration;

    let (tx, rx) = spawn_actor();

    // Start the actor with a visible limit that will include our injected event.
    tx.send(ActorCommand::Start {
        visible_limit: 100,
        emit_hz: 0,
    })
    .expect("send Start");

    // Build a kind:1 event with a fixed author pubkey (no kind:0 will arrive).
    let author_pk = "c13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13a";
    let event_id  = "c13e0000c13e0000c13e0000c13e0000c13e0000c13e0000c13e0000c13e0000";
    let raw = RawEvent {
        id: event_id.to_string(),
        pubkey: author_pk.to_string(),
        created_at: 1_000,
        kind: 1,
        tags: vec![],
        content: "D1 placeholder test note".to_string(),
        sig: "a".repeat(128),
    };

    use nmp_core::store::VerifiedEvent;
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    tx.send(ActorCommand::IngestPreVerifiedEvents(vec![verified]))
        .expect("send IngestPreVerifiedEvents");

    // Drain envelopes until we find a `Snapshot` carrying our event in `items`.
    // Every frame on the channel is wrapped as `{"t":"…","v":…}` per ADR-0001
    // (T103); decoding through `UpdateEnvelope` here proves the snapshot is
    // delivered with the canonical discriminator — discrete `Update` frames
    // (e.g. `Started`) are skipped on the typed tag, never by key sniffing.
    let snapshot = {
        let mut found: Option<serde_json::Value> = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(frame) => {
                    let envelope: UpdateEnvelope = serde_json::from_str(&frame)
                        .unwrap_or_else(|e| {
                            panic!("every channel frame must decode as UpdateEnvelope (ADR-0001 / T103) — got error {e} on frame: {frame}")
                        });
                    if let UpdateEnvelope::Snapshot(snapshot) = envelope {
                        let items = snapshot
                            .get("items")
                            .and_then(|value| value.as_array());
                        if let Some(items) = items {
                            if items
                                .iter()
                                .any(|item| item.get("id").and_then(|id| id.as_str()) == Some(event_id))
                            {
                                found = Some(snapshot);
                                break;
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }
        found.expect(
            "actor must emit a `snapshot` envelope whose `items` contains our event within 5 s",
        )
    };

    // The snapshot's `items` field is the projection of the kernel's visible
    // timeline — required by the view payload contract (see ADR-0001 §"Periodic
    // snapshot" and `Kernel::make_update`).
    let items = snapshot["items"]
        .as_array()
        .expect("snapshot must have an items array (Kernel::make_update contract)");
    let our_item = items
        .iter()
        .find(|item| item["id"].as_str() == Some(event_id))
        .expect("our event must appear in items");

    // C13 core assertion: author_picture_url must be a non-null, non-empty String.
    let pic_url = our_item["author_picture_url"]
        .as_str()
        .expect("author_picture_url must be a JSON string (not null) — D1 violation if missing");
    assert!(
        !pic_url.is_empty(),
        "author_picture_url must be non-empty (D1 placeholder contract)"
    );
    assert!(
        pic_url.starts_with("identicon:"),
        "no-profile placeholder must be an identicon URI, got: {pic_url}"
    );

    // author_avatar_source distinguishes placeholder from authoritative.
    // ADR-0017: with no kind:0 ingested, the discriminator MUST be
    // "placeholder" (not "kind0"), tracking the actual URL selection.
    let source = our_item["author_avatar_source"]
        .as_str()
        .expect("author_avatar_source must be present");
    assert_eq!(source, "placeholder");

    tx.send(ActorCommand::Shutdown).ok();
}
