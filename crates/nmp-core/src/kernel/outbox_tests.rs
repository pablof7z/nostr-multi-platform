//! T105 integration tests — outbox-driven REQ + publish fan-out and the
//! kind:10002 recompilation trigger.
//!
//! These tests exercise the live REQ emitters + publish path against a
//! multi-author MemEventStore fixture with distinct kind:10002 write
//! relays per author. They pin the D3 enforcement bullets:
//!
//! 1. **Generic multi-author REQ** fans out to each author's resolved write
//!    relays (NOT the BOOTSTRAP constants) once their kind:10002 is cached.
//! 2. **Publish** fans out to the author's resolved write relays via
//!    `Nip65OutboxResolver`.
//! 3. **Cold-start** with no cached kind:10002 routes the first emission to
//!    the bootstrap discovery seed, then re-plans onto resolved relays after
//!    the relay list arrives (recompilation trigger).

use super::*;
use crate::planner::{InterestLifecycle, InterestScope, InterestShape, LogicalInterest};
use crate::relay::{BOOTSTRAP_DISCOVERY_RELAYS, DEFAULT_VISIBLE_LIMIT};
use crate::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use std::collections::BTreeSet;

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn install_relay_list(kernel: &Kernel, author: &str, write: &[&str], read: &[&str], both: &[&str]) {
    kernel.seed_mailbox_relay_list(
        author,
        read.iter().map(|s| s.to_string()).collect(),
        write.iter().map(|s| s.to_string()).collect(),
        both.iter().map(|s| s.to_string()).collect(),
    );
}

fn open_author_interest(kernel: &mut Kernel, owner: &str, authors: &[&str], kinds: &[u32]) {
    let shape = InterestShape {
        authors: authors.iter().map(|author| (*author).to_string()).collect(),
        kinds: kinds.iter().copied().collect(),
        ..Default::default()
    };
    let key = SubKey::builder("open-interest")
        .with(&shape)
        .with(1u32)
        .finish();
    let identity = SubIdentity::new(SubOwnerKey::new(owner), key, SubScope::Global);
    let interest = LogicalInterest {
        scope: InterestScope::Global,
        shape,
        lifecycle: InterestLifecycle::Tailing,
        ..Default::default()
    };
    let _ = kernel.open_interest_sub(identity, interest);
}

#[test]
fn multi_author_interest_fans_out_per_author_write_relays_not_constants() {
    // Two authors with DISTINCT write relays MUST each get a REQ on their own
    // resolved relay, each carrying only the authors that relay serves — never
    // a hardcoded `RelayRole::Content` URL.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ALICE.to_string());
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);
    install_relay_list(&kernel, ALICE, &["wss://alice.relay/"], &[], &[]);
    install_relay_list(
        &kernel,
        BOB,
        &["wss://bob.write/"],
        &[],
        &["wss://shared.relay/"],
    );

    open_author_interest(&mut kernel, "outbox-multi-author", &[ALICE, BOB], &[1, 6]);

    // The actor idle-loop call: lifecycle compiles + emits the per-relay REQ diff.
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
        "lifecycle drain must emit generic author-interest REQs"
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
    let urls: BTreeSet<String> = reqs.iter().map(|(u, _)| (*u).clone()).collect();
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
    // T105 / T140: NIP-65 arrival for an interested author triggers recompile
    // and re-routes from discovery (no-NIP65 fallback) to the resolved write relay.
    //
    // Setup: an interest names ALICE; alice's kind:10002 is NOT cached initially
    // so the first M2 drain emits a discovery (kind:10002) probe. Once alice's
    // kind:10002 arrives (Nip65Arrived trigger), the second M2 drain emits a
    // REQ for alice's resolved write relay and CLOSEs the prior fallback REQ.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(ALICE.to_string());
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);

    open_author_interest(&mut kernel, "outbox-cold-start", &[ALICE], &[1, 6]);

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

    // Seed kind:10002 for ALICE — Nip65Arrived trigger fires.
    kernel.seed_kind10002_for_test(ALICE, &["wss://alice.write/"]);

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
    use nmp_signer_iface::{SignedEvent, UnsignedEvent};

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
        urls.contains("wss://alice.primary"),
        "primary write relay must receive the EVENT"
    );
    assert!(
        urls.contains("wss://alice.archive"),
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

// V-112 (ADR-0042): t121_thread_hydration_routes_ids_by_resolved_author_write_relays
// deleted — ThreadViewState (including pending_ids / requested_ids) and
// maybe_open_thread_hydration() removed from kernel. Thread hydration is now
// owned by the handle-opened per-app Flat feed.

// M2 (ADR-0042): `hashtag_firehose_routes_to_active_account_inbox_relays_not_bootstrap`
// was deleted with the `open_firehose_tag` kernel method it exercised. Hashtag
// feeds now register as a generic `open_interest` (`{"kinds":[1],"#t":[…]}`,
// scope Global) and route through the `SubscriptionCompiler`'s standard
// inbox-direction (Case D / NIP-65 read-relay) lane — covered by the planner
// compiler partition tests, not a bespoke kernel routing test.

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
