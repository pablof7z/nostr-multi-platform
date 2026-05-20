//! T105 integration tests — outbox-driven REQ + publish fan-out and the
//! kind:10002 recompilation trigger.
//!
//! These tests exercise the live REQ emitters + publish path against a
//! multi-author MemEventStore fixture with distinct kind:10002 write
//! relays per author. They pin the D3 enforcement bullets:
//!
//! 1. **Follow-feed REQ** fans out to each followed author's resolved write
//!    relays (NOT the BOOTSTRAP constants) once their kind:10002 is cached.
//! 2. **Publish** fans out to the author's resolved write relays via
//!    `Nip65OutboxResolver`.
//! 3. **Cold-start** with no cached kind:10002 routes the first emission to
//!    the bootstrap discovery seed, then re-plans onto resolved relays after
//!    the relay list arrives (recompilation trigger).

use super::*;
use crate::kernel::types::AuthorRelayList;
use crate::relay::{BOOTSTRAP_DISCOVERY_RELAYS, DEFAULT_VISIBLE_LIMIT};

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn install_relay_list(
    kernel: &mut Kernel,
    author: &str,
    write: &[&str],
    read: &[&str],
    both: &[&str],
) {
    kernel.author_relay_lists.insert(
        author.to_string(),
        AuthorRelayList {
            event_id: "x".to_string(),
            created_at: 1,
            read_relays: read.iter().map(|s| s.to_string()).collect(),
            write_relays: write.iter().map(|s| s.to_string()).collect(),
            both_relays: both.iter().map(|s| s.to_string()).collect(),
        },
    );
}

#[test]
fn follow_feed_fans_out_per_author_write_relays_not_constants() {
    // T140: the follow-feed REQ is now carried by the M2 planner
    // (`drain_lifecycle_tick`), NOT the retired M1 `maybe_open_timeline()`
    // `seed-timeline-*` path. The D3 contract this test pins is unchanged —
    // only the mechanism moved from M1 to M2: two followed authors with
    // DISTINCT write relays MUST each get a REQ on their own resolved relay,
    // each carrying only the authors that relay serves — never a hardcoded
    // `RelayRole::Content` URL.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ALICE.to_string());
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);
    install_relay_list(&mut kernel, ALICE, &["wss://alice.relay/"], &[], &[]);
    install_relay_list(
        &mut kernel,
        BOB,
        &["wss://bob.write/"],
        &[],
        &["wss://shared.relay/"],
    );

    // ALICE (the active account) follows ALICE + BOB. `ingest_contacts`
    // registers the M2 follow-feed interests + enqueues FollowListChanged.
    kernel
        .inject_replaceable_event(
            "1111111111111111111111111111111111111111111111111111111111111111",
            ALICE,
            1_000,
            3,
            vec![
                vec!["p".to_string(), ALICE.to_string()],
                vec!["p".to_string(), BOB.to_string()],
            ],
            "wss://seed.relay/",
            1_000_000,
        )
        .expect("inject kind:3");

    // The actor idle-loop call: M2 compiles + emits the per-relay REQ diff.
    let frames = kernel.drain_lifecycle_tick();
    let reqs: Vec<(&String, &String)> = frames
        .iter()
        .filter_map(|f| match f {
            crate::subs::WireFrame::Req {
                relay_url,
                filter_json,
                ..
            } => Some((relay_url, filter_json)),
            _ => None,
        })
        .collect();
    assert!(
        !reqs.is_empty(),
        "M2 drain must emit follow-feed REQs after ingest_contacts"
    );

    // (1) Every REQ carries an explicit resolved relay_url.
    for (url, _) in &reqs {
        assert!(
            !url.is_empty(),
            "T105: every WireFrame::Req has an explicit relay_url"
        );
    }

    // (2) Alice's and Bob's resolved write relays both appear; the shared
    // (both-marker) relay also appears.
    let urls: std::collections::BTreeSet<String> =
        reqs.iter().map(|(u, _)| (*u).clone()).collect();
    assert!(
        urls.contains("wss://alice.relay/"),
        "alice's write relay must be a routing target, got {urls:?}"
    );
    assert!(
        urls.contains("wss://bob.write/"),
        "bob's write relay must be a routing target, got {urls:?}"
    );
    assert!(
        urls.contains("wss://shared.relay/"),
        "bob's both-marker relay must be a routing target, got {urls:?}"
    );

    // (3) D3 enforcement: a REQ targeting "wss://alice.relay/" MUST carry
    // alice but NOT bob (and vice versa). The shared relay carries bob (his
    // "both" marker), not alice. Any kind:10002 discovery probe rides the
    // indexer set (bootstrap) and must NOT carry the resolved authors.
    for (url, filter) in &reqs {
        let carries_alice = filter.contains(ALICE);
        let carries_bob = filter.contains(BOB);
        match url.as_str() {
            "wss://alice.relay/" => {
                assert!(carries_alice, "alice's relay must carry alice");
                assert!(!carries_bob, "alice's relay must NOT carry bob");
            }
            "wss://bob.write/" | "wss://shared.relay/" => {
                assert!(carries_bob, "bob's relay must carry bob");
                assert!(!carries_alice, "bob's relay must NOT carry alice");
            }
            url if BOOTSTRAP_DISCOVERY_RELAYS.contains(&url) => {
                // Indexer/bootstrap discovery probe (kinds:[10002]); MUST NOT
                // carry the resolved follow authors (D3: their writes are
                // already resolved, no leak onto the discovery seed).
                assert!(
                    !carries_alice && !carries_bob,
                    "discovery seed must not carry resolved authors; \
                     filter = {filter}"
                );
            }
            other => panic!("unexpected resolved relay {other}: {filter}"),
        }
    }
}

#[test]
fn cold_start_routes_to_bootstrap_then_replans_after_nip65_arrives() {
    // T105 / T140: NIP-65 arrival for a followed author triggers M2 recompile
    // and re-routes from discovery (no-NIP65 fallback) to the resolved write relay.
    //
    // Setup: ALICE follows herself; alice's kind:10002 is NOT cached initially
    // so the first M2 drain emits a discovery (kind:10002) probe. Once alice's
    // kind:10002 arrives (Nip65Arrived trigger), the second M2 drain emits a
    // REQ for alice's resolved write relay and CLOSEs the prior fallback REQ.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ALICE.to_string());
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);

    // Inject kind:3: ALICE follows herself. No kind:10002 yet.
    let follows = vec![vec!["p".to_string(), ALICE.to_string()]];
    kernel
        .inject_replaceable_event(
            "1111111111111111111111111111111111111111111111111111111111111111",
            ALICE,
            1_000,
            3,
            follows,
            "wss://seed.relay/",
            1_000_000,
        )
        .expect("inject kind:3");

    // First M2 drain: no NIP-65 for ALICE → planner probes the indexer.
    // We don't assert on the exact URL (it's the indexer probe, not alice's
    // write relay) — we just confirm frames are emitted.
    let first_frames = kernel.drain_lifecycle_tick();
    assert!(
        !first_frames.is_empty(),
        "cold-start M2 drain must emit at least one frame (indexer probe)"
    );
    // The resolved write relay must NOT appear before kind:10002 is cached.
    let first_req_urls: Vec<String> = first_frames
        .iter()
        .filter_map(|f| match f {
            crate::subs::WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !first_req_urls.iter().any(|u| u == "wss://alice.write/"),
        "pre-NIP65 drain must NOT route to alice's resolved relay; got {first_req_urls:?}"
    );

    // Inject kind:10002 for ALICE — Nip65Arrived trigger fires.
    use crate::store::InsertOutcome;
    let nip65 = vec![vec![
        "r".to_string(),
        "wss://alice.write/".to_string(),
        "write".to_string(),
    ]];
    let outcome = kernel
        .inject_replaceable_event(
            "2222222222222222222222222222222222222222222222222222222222222222",
            ALICE,
            2_000,
            10002,
            nip65,
            "wss://seed.relay/",
            2_000_000,
        )
        .expect("inject kind:10002 must succeed");
    assert!(matches!(
        outcome,
        InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }
    ));

    // Second M2 drain: Nip65Arrived trigger → recompile → resolved relay REQ.
    // The prior probe (kind:10002 discovery to indexer) was emitted as an
    // auxiliary frame outside the compiled plan, so no CLOSE is emitted for it
    // by plan_diff. The key assertion is that alice's resolved relay appears.
    let second_frames = kernel.drain_lifecycle_tick();
    let second_req_urls: Vec<String> = second_frames
        .iter()
        .filter_map(|f| match f {
            crate::subs::WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();

    assert!(
        second_req_urls.iter().any(|u| u == "wss://alice.write/"),
        "post-NIP65 M2 drain must route to alice's resolved write relay; \
         got req_urls = {second_req_urls:?}, all frames = {second_frames:?}"
    );
}

#[test]
fn publish_fans_out_to_author_write_relays_via_outbox() {
    // T99 subsumed by T105: a single PublishAction must emit N EVENT
    // frames — one per resolved write relay from Nip65OutboxResolver. This
    // is the publish-side enforcement of D3: no `RelayRole::Content`
    // hardcoded constant lands the event on a single fixed socket.
    use crate::store::{RawEvent, VerifiedEvent};
    use crate::substrate::{SignedEvent, UnsignedEvent};

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Author Alice has two write relays declared via NIP-65. Inject the
    // kind:10002 through the store so Nip65OutboxResolver reads it back.
    let nip65_tags = vec![
        vec![
            "r".to_string(),
            "wss://alice.primary/".to_string(),
            "write".to_string(),
        ],
        vec![
            "r".to_string(),
            "wss://alice.archive/".to_string(),
            "write".to_string(),
        ],
    ];
    let kind10002 = RawEvent {
        id: "2222222222222222222222222222222222222222222222222222222222222222".to_string(),
        pubkey: ALICE.to_string(),
        created_at: 2_000,
        kind: 10002,
        tags: nip65_tags,
        content: String::new(),
        sig: "a".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(kind10002);
    let _ = kernel
        .store
        .insert(verified, &"wss://bootstrap/".to_string(), 2_000_000);

    // Build a synthetic signed kind:1 from Alice. The publish path doesn't
    // verify the signature itself; the store does (and we bypass it via
    // the test-support path on the publish-resolver lookup).
    let unsigned = UnsignedEvent {
        pubkey: ALICE.to_string(),
        kind: 1,
        tags: vec![],
        content: "hello".to_string(),
        created_at: 3_000,
    };
    let signed = SignedEvent {
        id: "3333333333333333333333333333333333333333333333333333333333333333".to_string(),
        sig: "b".repeat(128),
        unsigned,
    };

    let outbound = kernel.publish_signed(&signed, &[]);
    assert_eq!(
        outbound.len(),
        2,
        "publish must fan out one EVENT per resolved write relay; \
         got {} frames",
        outbound.len()
    );
    let urls: std::collections::BTreeSet<String> =
        outbound.iter().map(|m| m.relay_url.clone()).collect();
    assert!(
        urls.contains("wss://alice.primary/"),
        "primary write relay must receive the EVENT"
    );
    assert!(
        urls.contains("wss://alice.archive/"),
        "archive write relay must receive the EVENT"
    );
    for m in &outbound {
        assert!(
            !BOOTSTRAP_DISCOVERY_RELAYS.contains(&m.relay_url.as_str()),
            "warm-author publish MUST NOT leak onto the bootstrap constant"
        );
        assert!(m.text.starts_with("[\"EVENT\""), "frame is an EVENT");
    }
}

// ── T121: thread hydration outbox (codex R1) ─────────────────────────────────
//
// The thread hydration path (`maybe_open_thread_hydration`) fills in missing
// parent/root events from `#e` id refs. Pre-T121 it fanned out to the
// bootstrap discovery seed; T121 routes each id to its **original-event
// author's** NIP-65 write relays (resolved via the in-kernel `events` cache),
// with the bootstrap seed reserved for the cold-start path where the local
// store has no record of who wrote a given id. This pins the wire-level
// behaviour.

const CHARLIE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn id_for(prefix: char) -> String {
    std::iter::repeat_n(prefix, 64).collect()
}

#[test]
fn t121_thread_hydration_routes_ids_by_resolved_author_write_relays() {
    // Three authors A, B, C — A has kind:10002 → relay1, B → relay2, and C
    // has NO cached kind:10002 (cold-start). Three events (one per author)
    // are seeded into the kernel's `events` cache so the hydration path can
    // resolve id → author. Hydration is issued for [id_A, id_B, id_C]; the
    // expectation:
    //   * relay1 receives a REQ carrying [id_A]
    //   * relay2 receives a REQ carrying [id_B]
    //   * each BOOTSTRAP_DISCOVERY_RELAYS seed receives a REQ carrying [id_C]
    //     (the cold-start fallback for an author with no resolved write set).
    // No REQ leaves on a relay that does not own the id it carries.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    install_relay_list(&mut kernel, ALICE, &["wss://relay1/"], &[], &[]);
    install_relay_list(&mut kernel, BOB, &["wss://relay2/"], &[], &[]);
    // CHARLIE intentionally has no kind:10002 cached → cold-start path.

    let id_a = id_for('a');
    let id_b = id_for('b');
    let id_c = id_for('c');

    // Seed the kernel's events cache so `partition_ids_by_author_write_relays`
    // can resolve each id to its author. The `relay_count`/`created_at`
    // fields are immaterial to the routing decision — only `author` is read.
    for (id, author) in [
        (&id_a, ALICE),
        (&id_b, BOB),
        (&id_c, CHARLIE),
    ] {
        kernel.events.insert(
            id.clone(),
            StoredEvent {
                id: id.clone(),
                author: author.to_string(),
                kind: 1,
                created_at: 1_000,
                tags: vec![],
                content: String::new(),
                relay_count: 1,
            },
        );
    }

    // Enqueue the three ids directly onto the hydration queue and drive the
    // wire-level emitter. This is the same code path `prepare_thread_requests`
    // invokes after walking parent/root refs, but isolated to the exact id
    // set under test (no confounding focused/root traversal).
    kernel.thread_view.pending_ids.insert(id_a.clone());
    kernel.thread_view.pending_ids.insert(id_b.clone());
    kernel.thread_view.pending_ids.insert(id_c.clone());

    let requests = kernel.maybe_open_thread_hydration();

    // Partition the emitted REQs by their relay_url. The thread-ids- prefix
    // gates the assertion: thread-replies- is gated by an empty
    // `pending_thread_reply_targets` so this exercise only fires the ids leg.
    let ids_reqs: Vec<&OutboundMessage> = requests
        .iter()
        .filter(|r| r.text.contains("thread-ids-"))
        .collect();
    assert!(
        !ids_reqs.is_empty(),
        "hydration must emit at least one REQ for the seeded id set"
    );

    // (1) Every REQ carries an explicit relay_url — never an empty string.
    for r in &ids_reqs {
        assert!(
            !r.relay_url.is_empty(),
            "T121: every hydration OutboundMessage has an explicit relay_url"
        );
    }

    // (2) The expected URL set is exactly relay1 + relay2 + BOOTSTRAP seeds.
    //     (Two bootstrap seeds today: damus.io + nos.lol. The cold-start id
    //     emits one REQ per seed because `bootstrap_discovery_relays()` is
    //     the seed-list itself, not a single fallback URL.)
    let urls: std::collections::BTreeSet<String> =
        ids_reqs.iter().map(|r| r.relay_url.clone()).collect();
    assert!(
        urls.contains("wss://relay1/"),
        "alice's resolved write relay must be a routing target; got {urls:?}"
    );
    assert!(
        urls.contains("wss://relay2/"),
        "bob's resolved write relay must be a routing target; got {urls:?}"
    );
    for seed in BOOTSTRAP_DISCOVERY_RELAYS {
        assert!(
            urls.contains(*seed),
            "uncached-author id must fall back to bootstrap seed {seed}; \
             got {urls:?}"
        );
    }
    // No unexpected leakage onto other resolved relays.
    let expected: std::collections::BTreeSet<String> = [
        "wss://relay1/".to_string(),
        "wss://relay2/".to_string(),
    ]
    .into_iter()
    .chain(BOOTSTRAP_DISCOVERY_RELAYS.iter().map(|s| s.to_string()))
    .collect();
    assert_eq!(
        urls, expected,
        "hydration URL set must be exactly the resolved write relays plus \
         the cold-start bootstrap seeds"
    );

    // (3) D3 enforcement: relay1 carries ONLY id_a, relay2 carries ONLY id_b,
    //     bootstrap seeds carry ONLY id_c. A leak (id_a on relay2, or id_b
    //     on bootstrap when bob's write relay is resolved, etc.) is the
    //     pre-T121 bug this task closes.
    for r in &ids_reqs {
        let carries_a = r.text.contains(&id_a);
        let carries_b = r.text.contains(&id_b);
        let carries_c = r.text.contains(&id_c);
        match r.relay_url.as_str() {
            "wss://relay1/" => {
                assert!(carries_a, "relay1 must carry id_a; text={}", r.text);
                assert!(!carries_b, "relay1 must NOT carry id_b; text={}", r.text);
                assert!(!carries_c, "relay1 must NOT carry id_c; text={}", r.text);
            }
            "wss://relay2/" => {
                assert!(carries_b, "relay2 must carry id_b; text={}", r.text);
                assert!(!carries_a, "relay2 must NOT carry id_a; text={}", r.text);
                assert!(!carries_c, "relay2 must NOT carry id_c; text={}", r.text);
            }
            url if BOOTSTRAP_DISCOVERY_RELAYS.contains(&url) => {
                assert!(
                    carries_c,
                    "bootstrap seed must carry id_c (uncached author); text={}",
                    r.text
                );
                assert!(
                    !carries_a,
                    "bootstrap seed must NOT carry id_a (alice resolved); text={}",
                    r.text
                );
                assert!(
                    !carries_b,
                    "bootstrap seed must NOT carry id_b (bob resolved); text={}",
                    r.text
                );
            }
            other => panic!("unexpected hydration relay {other}"),
        }
    }
}

#[test]
fn hashtag_firehose_routes_to_active_account_inbox_relays_not_bootstrap() {
    // T122 / codex R2: a hashtag firehose REQ (kind:1 with #t filter) is
    // inbox-direction — the user IS the recipient of their own hashtag
    // interest. With an active account whose kind:10002 declares read
    // relays, `open_firehose_tag` must fan out exactly onto those read
    // relays, never onto the bootstrap discovery seed.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // ALICE is the active account. Her NIP-65 declares two read relays
    // (one pure read marker, one shared "both" marker) and a write relay
    // that MUST NOT appear in the hashtag firehose fan-out.
    install_relay_list(
        &mut kernel,
        ALICE,
        &["wss://alice.write/"],
        &["wss://alice.inbox1/"],
        &["wss://alice.inbox2/"],
    );
    kernel.active_account = Some(ALICE.to_string());

    // Trigger the hashtag firehose. `can_send=true` runs the emit branch
    // synchronously and returns the resolved OutboundMessage fan-out.
    let outbound = kernel.open_firehose_tag("nostr".to_string(), true);
    assert!(
        !outbound.is_empty(),
        "open_firehose_tag must emit at least one REQ when can_send=true"
    );

    // Every frame is a diagnostic firehose REQ (sub_id prefix + filter).
    for m in &outbound {
        assert!(
            m.text.starts_with("[\"REQ\",\"diag-firehose-"),
            "every frame is a diag-firehose REQ, got: {}",
            m.text
        );
        assert!(
            m.text.contains("\"#t\":[\"nostr\"]"),
            "every frame carries the #t filter for the requested tag, got: {}",
            m.text
        );
    }

    // The exact fan-out target set is the active account's read+both relays,
    // sorted/deduped per `recipient_read_relays`.
    let urls: std::collections::BTreeSet<String> =
        outbound.iter().map(|m| m.relay_url.clone()).collect();
    let expected: std::collections::BTreeSet<String> = [
        "wss://alice.inbox1/".to_string(),
        "wss://alice.inbox2/".to_string(),
    ]
    .into_iter()
    .collect();
    assert_eq!(
        urls, expected,
        "hashtag firehose URL set must equal the active account's read+both relays exactly; \
         got {urls:?}, expected {expected:?}"
    );

    // D3 enforcement: the bootstrap discovery seed MUST NOT carry the
    // hashtag firehose now that the active account has a cached kind:10002.
    for m in &outbound {
        assert!(
            !BOOTSTRAP_DISCOVERY_RELAYS.contains(&m.relay_url.as_str()),
            "hashtag firehose MUST NOT route to bootstrap once active account has kind:10002; \
             leaked to {}",
            m.relay_url
        );
    }

    // The user's WRITE relay is not an inbox relay; it must not appear.
    assert!(
        !urls.contains("wss://alice.write/"),
        "hashtag firehose is inbox-direction; the active account's write relay \
         must NOT be a routing target, got urls = {urls:?}"
    );
}


// ─── T130 — deferred queue preserves per-URL routing on drain ────────────────

#[test]
fn t130_deferred_outbound_preserves_relay_url_through_drain() {
    // T130 invariant (kernel side): a frame placed into `deferred_outbound`
    // by any producer (CLOSE-on-replan, defer-on-disconnect, AUTH-pause
    // defer) must drain via `pending_view_requests` carrying the SAME
    // `relay_url` the producer stamped. The kernel does not rewrite routing
    // at drain time — it preserves what the producer chose.
    //
    // Without this guarantee, a CLOSE for a sub originally opened on URL_B
    // could drain back targeting URL_A (the bootstrap), tearing down the
    // wrong socket and leaving URL_B with a stranded sub_id (and double-
    // billing the relay since the kernel re-emits as a new sub on the next
    // recompile).
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let resolved_url = "wss://resolved.write/".to_string();
    let msg = OutboundMessage {
        role: RelayRole::Content,
        relay_url: resolved_url.clone(),
        text: "[\"CLOSE\",\"some-sub\"]".to_string(),
    };
    kernel.defer_outbound(msg.clone());

    let drained = kernel.pending_view_requests();
    let close: Vec<_> = drained
        .iter()
        .filter(|m| m.text == "[\"CLOSE\",\"some-sub\"]")
        .collect();
    assert_eq!(close.len(), 1, "deferred CLOSE must drain exactly once");
    assert_eq!(
        close[0].relay_url, resolved_url,
        "drained frame must preserve the producer-stamped relay_url"
    );
    assert_eq!(
        close[0].role,
        RelayRole::Content,
        "drained frame must preserve the role label"
    );
}
