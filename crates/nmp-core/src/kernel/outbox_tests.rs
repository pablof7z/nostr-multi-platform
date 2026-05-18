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
    // Two followed authors with DISTINCT write relays. The follow-feed REQ
    // MUST emit one REQ per resolved relay, each carrying only the authors
    // that relay serves — never on a hardcoded `RelayRole::Content` URL.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    install_relay_list(&mut kernel, ALICE, &["wss://alice.relay/"], &[], &[]);
    install_relay_list(
        &mut kernel,
        BOB,
        &["wss://bob.write/"],
        &[],
        &["wss://shared.relay/"],
    );

    // Force the timeline to open: the seed-contacts gate is satisfied when
    // all seed accounts have contributed, or the contacts deadline elapses.
    // The simplest path here is to populate timeline_authors directly and
    // call maybe_open_timeline via the open-time gate. We use the
    // contacts-deadline-elapsed path: set the deadline to the past, then
    // populate seed_contacts so should_open_timeline returns true.
    kernel.seed_contacts.insert(
        ALICE.to_string(),
        vec![ALICE.to_string(), BOB.to_string()],
    );
    kernel.seed_contacts.insert(BOB.to_string(), vec![]);
    // Seed-account count is 3 (pablof7z/fiatjaf/jb55); the elapsed deadline
    // path is the test-friendly gate.
    kernel.contacts_deadline = Some(Instant::now() - Duration::from_secs(60));

    let requests = kernel.maybe_open_timeline();
    // Strip out pending_profile_claim_requests passes (the function tail
    // calls into it). Only seed-timeline REQs.
    let timeline_reqs: Vec<_> = requests
        .iter()
        .filter(|r| r.text.contains("seed-timeline"))
        .collect();
    assert!(
        !timeline_reqs.is_empty(),
        "maybe_open_timeline must emit REQs after the contacts deadline"
    );

    // (1) Every timeline REQ carries a resolved relay_url — never a routing
    // default we'd be hard-coding into the wire.
    for r in &timeline_reqs {
        assert!(
            !r.relay_url.is_empty(),
            "T105: every OutboundMessage has an explicit relay_url"
        );
    }

    // (2) Alice and Bob's resolved write relays both appear in the URL set;
    // the shared relay also appears once.
    let urls: std::collections::BTreeSet<String> =
        timeline_reqs.iter().map(|r| r.relay_url.clone()).collect();
    assert!(
        urls.contains("wss://alice.relay/"),
        "alice's write relay must be a routing target, got {urls:?}"
    );
    assert!(
        urls.contains("wss://bob.write/"),
        "bob's write relay must be a routing target"
    );
    assert!(
        urls.contains("wss://shared.relay/"),
        "bob's both-marker relay must be a routing target"
    );

    // (3) D3 enforcement: a REQ targeting "wss://alice.relay/" MUST carry
    // alice but NOT bob (and vice versa). The shared relay may carry bob
    // (bob's "both" marker), not alice. Note: `maybe_open_timeline` also
    // adds the built-in seed_accounts (pablof7z/fiatjaf/jb55) which lack
    // cached kind:10002 → they land on the BOOTSTRAP_DISCOVERY_RELAYS seeds.
    // Those seed REQs should NOT carry alice or bob (the resolved authors).
    for r in &timeline_reqs {
        let carries_alice = r.text.contains(ALICE);
        let carries_bob = r.text.contains(BOB);
        match r.relay_url.as_str() {
            "wss://alice.relay/" => {
                assert!(carries_alice, "alice's relay must carry alice");
                assert!(!carries_bob, "alice's relay must NOT carry bob");
            }
            "wss://bob.write/" | "wss://shared.relay/" => {
                assert!(carries_bob, "bob's relay must carry bob");
                assert!(!carries_alice, "bob's relay must NOT carry alice");
            }
            url if BOOTSTRAP_DISCOVERY_RELAYS.contains(&url) => {
                // Bootstrap-routed sub for the seed_accounts cohort (no
                // cached NIP-65). MUST NOT carry our resolved authors — if
                // it does we've leaked the planner-resolved set onto the
                // discovery seed (D3 violation).
                assert!(
                    !carries_alice,
                    "bootstrap seed must not carry alice (her writes resolved)"
                );
                assert!(
                    !carries_bob,
                    "bootstrap seed must not carry bob (his writes resolved)"
                );
            }
            other => panic!("unexpected resolved relay {other}"),
        }
    }
}

#[test]
fn cold_start_routes_to_bootstrap_then_replans_after_nip65_arrives() {
    // Cold start: no cached kind:10002 for ALICE. The first follow-feed
    // emission must route to the bootstrap discovery seed — but the moment
    // an ingest_relay_list arrives for an already-timeline author, the
    // recompilation trigger fires and the NEXT emission targets the
    // resolved write relay.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel
        .seed_contacts
        .insert(ALICE.to_string(), vec![ALICE.to_string()]);
    kernel.contacts_deadline = Some(Instant::now() - Duration::from_secs(60));

    let first = kernel.maybe_open_timeline();
    let first_timeline: Vec<_> = first
        .iter()
        .filter(|r| r.text.contains("seed-timeline"))
        .collect();
    assert!(!first_timeline.is_empty(), "first emission must fire");
    // Every cold-start REQ targets a bootstrap seed.
    for r in &first_timeline {
        assert!(
            BOOTSTRAP_DISCOVERY_RELAYS.contains(&r.relay_url.as_str()),
            "cold-start emission MUST route to bootstrap, got {}",
            r.relay_url
        );
    }

    // ── Recompilation trigger: alice publishes a kind:10002 declaring
    // her write relays. The kernel must mark the timeline for re-planning.
    use crate::store::InsertOutcome;
    let nip65 = vec![
        vec![
            "r".to_string(),
            "wss://alice.write/".to_string(),
            "write".to_string(),
        ],
    ];
    let outcome = kernel
        .inject_replaceable_event(
            "1111111111111111111111111111111111111111111111111111111111111111",
            ALICE,
            2000,
            10002,
            nip65,
            "wss://bootstrap/",
            2_000_000,
        )
        .expect("inject must succeed");
    assert!(matches!(
        outcome,
        InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }
    ));
    assert!(
        !kernel.timeline_requested,
        "kind:10002 arrival for a timeline author must mark the timeline \
         for re-planning (A1 recompilation trigger)"
    );

    // ── Next emission re-plans onto the resolved relay. We additionally
    // expect the prior bootstrap-routed sub to have been CLOSEd; the
    // CLOSEs land in `deferred_outbound` and drain on `pending_view_requests`.
    let second = kernel.maybe_open_timeline();
    let second_timeline: Vec<_> = second
        .iter()
        .filter(|r| r.text.contains("seed-timeline"))
        .collect();
    assert!(!second_timeline.is_empty(), "re-plan must emit");
    // The resolved relay MUST appear as the routing target.
    assert!(
        second_timeline
            .iter()
            .any(|r| r.relay_url == "wss://alice.write/"),
        "post-NIP65 emission must route to alice's resolved write relay; \
         saw urls = {:?}",
        second_timeline
            .iter()
            .map(|r| r.relay_url.clone())
            .collect::<Vec<_>>()
    );

    // The CLOSE frames for the prior bootstrap-routed seed-timeline subs
    // sit in deferred_outbound; the next `pending_view_requests` drains them.
    let drained = kernel.pending_view_requests();
    let closes: Vec<_> = drained
        .iter()
        .filter(|r| r.text.starts_with("[\"CLOSE\""))
        .filter(|r| r.text.contains("seed-timeline-"))
        .collect();
    assert!(
        !closes.is_empty(),
        "re-plan must CLOSE the prior bootstrap-routed seed-timeline subs \
         so they're not double-billed against the new resolved subs"
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
    kernel.pending_thread_ids.insert(id_a.clone());
    kernel.pending_thread_ids.insert(id_b.clone());
    kernel.pending_thread_ids.insert(id_c.clone());

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
