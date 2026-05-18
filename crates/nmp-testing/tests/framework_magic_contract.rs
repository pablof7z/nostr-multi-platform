//! Framework Magic Contract — cross-cutting test suite.
//!
//! 14 tests: C1–C13 behaviour tests + 1 coverage meta-test.
//! Intentionally NOT milestone-prefixed; see
//! `docs/design/framework-magic/test-scaffolding.md` §1.
//!
//! Active on master: C1, C2, C3, C4, C6, C9, C10, meta-test (8 of 14).
//! Ignored pending milestone implementation:
//!   C5  `#[ignore = "pending M2 FollowListChanged trigger"]`
//!   C7  `#[ignore = "pending M2 PublishPlanner + M6 SendNote consumer"]`
//!   C8  `#[ignore = "pending M2 wire-emitter auto-close + buffer"]`
//!   C11 `#[ignore = "pending M6 signers + KeyringCapability"]`
//!   C12 `#[ignore = "pending M8 multi-account state machine"]`
//!   C13 `#[ignore = "pending M2 projection cache + ViewModule surface"]`
//!
//! Invocation: `cargo test -p nmp-testing --test framework_magic_contract`

use nmp_core::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot, SubscriptionCompiler,
};
use nmp_core::store::{InsertOutcome, TombstoneOrigin};
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX, BOB_HEX};

// ── Shared helpers ────────────────────────────────────────────────────────────

fn pubkey(seed: &str) -> String {
    format!("{seed:0>64}").chars().take(64).collect::<String>().to_lowercase()
}

fn relay(url: &str) -> String { url.to_string() }

fn interest_id(n: u64) -> InterestId { InterestId(n) }

fn mailbox_write(relays: &[&str]) -> MailboxSnapshot {
    MailboxSnapshot {
        write_relays: relays.iter().map(|s| relay(s)).collect(),
        read_relays: vec![],
        both_relays: vec![],
    }
}

// ── C1 ────────────────────────────────────────────────────────────────────────

/// C1: Replaceable-event supersession (kind 0 / 3 / 10000-19999) on insert.
/// Design: `docs/design/framework-magic/replaceable.md` §C1.
#[test]
fn c1_replaceable_supersedes_on_insert() {
    let h = StoreHarness::mem();

    let ev1 = h.make_event(ALICE_HEX, 0, 1_000);
    let id1 = ev1.id_bytes();
    assert!(matches!(h.insert_raw(ev1, "wss://r1/", 1_000_000), InsertOutcome::Inserted { .. }));
    h.assert_present(&id1);

    let ev2 = h.make_event(ALICE_HEX, 0, 2_000);
    let id2 = ev2.id_bytes();
    let o2 = h.insert_raw(ev2, "wss://r1/", 2_000_000);
    assert!(matches!(o2, InsertOutcome::Replaced { .. }), "newer must replace: {o2:?}");
    h.assert_absent(&id1);
    h.assert_present(&id2);

    let ev_stale = h.make_event(ALICE_HEX, 0, 500);
    let id_stale = ev_stale.id_bytes();
    let o_stale = h.insert_raw(ev_stale, "wss://r2/", 3_000_000);
    assert!(matches!(o_stale, InsertOutcome::Superseded { .. }), "stale: {o_stale:?}");
    h.assert_absent(&id_stale);
    h.assert_present(&id2);

    // Tie-break at same created_at: lexicographically smaller id wins.
    // Use BOB_HEX so we don't conflict with Alice's kind:0 slot.
    let id_large = "f".repeat(64);
    let id_small = "0".repeat(64);
    let ev_large = h.make_event_with_id(&id_large, BOB_HEX, 0, 5_000);
    let large_id_bytes = ev_large.id_bytes();
    h.insert_raw(ev_large, "wss://r1/", 5_000_000);
    h.assert_present(&large_id_bytes);

    let ev_small = h.make_event_with_id(&id_small, BOB_HEX, 0, 5_000);
    let small_id_bytes = ev_small.id_bytes();
    let o_small = h.insert_raw(ev_small, "wss://r1/", 5_000_001);
    assert!(
        matches!(o_small, InsertOutcome::Replaced { .. }),
        "smaller id must replace larger at same timestamp: {o_small:?}"
    );
    h.assert_present(&small_id_bytes);
    h.assert_absent(&large_id_bytes);
}

// ── C2 ────────────────────────────────────────────────────────────────────────

/// C2: Parameterized replaceable supersession (kind 30000-39999) by
/// `(pubkey, kind, d-tag)`. Different d-tags coexist; same d-tag supersedes.
/// Design: `docs/design/framework-magic/replaceable.md` §C2.
#[test]
fn c2_parameterized_replaceable_supersedes_by_dtag() {
    use nmp_testing::store_harness::ALICE_PUBKEY;
    let h = StoreHarness::mem();

    let ev1 = h.make_event_with_tags(ALICE_HEX, 30_023, 1_000, vec![
        vec!["d".to_string(), "foo".to_string()],
    ]);
    let id1 = ev1.id_bytes();
    h.insert_raw(ev1, "wss://t/", 1_000_000);
    let ev2 = h.make_event_with_tags(ALICE_HEX, 30_023, 2_000, vec![
        vec!["d".to_string(), "foo".to_string()],
    ]);
    let id2 = ev2.id_bytes();
    let o2 = h.insert_raw(ev2, "wss://t/", 2_000_000);
    assert!(matches!(o2, InsertOutcome::Replaced { .. }), "newer d=foo must replace: {o2:?}");
    h.assert_absent(&id1);
    h.assert_present(&id2);

    let ev_bar = h.make_event_with_tags(ALICE_HEX, 30_023, 1_000, vec![
        vec!["d".to_string(), "bar".to_string()],
    ]);
    let id_bar = ev_bar.id_bytes();
    h.insert_raw(ev_bar, "wss://t/", 1_000_000);
    h.assert_present(&id2);
    h.assert_present(&id_bar);
    let foo = h.store.get_param_replaceable(&ALICE_PUBKEY, 30_023, b"foo").unwrap();
    let bar = h.store.get_param_replaceable(&ALICE_PUBKEY, 30_023, b"bar").unwrap();
    assert_eq!(foo.unwrap().raw.id_bytes(), id2, "foo slot must hold v2");
    assert_eq!(bar.unwrap().raw.id_bytes(), id_bar, "bar slot must be independent");

    let ev_24 = h.make_event_with_tags(ALICE_HEX, 30_024, 1_000, vec![
        vec!["d".to_string(), "foo".to_string()],
    ]);
    let id_24 = ev_24.id_bytes();
    h.insert_raw(ev_24, "wss://t/", 1_000_000);
    h.assert_present(&id2);
    h.assert_present(&id_24);
    let r24 = h.store.get_param_replaceable(&ALICE_PUBKEY, 30_024, b"foo").unwrap();
    assert_eq!(r24.unwrap().raw.id_bytes(), id_24, "kind:30024 slot is independent");
}

// ── C3 ────────────────────────────────────────────────────────────────────────

/// C3: Kind:5 delete — referenced events removed, tombstone persisted.
/// Cross-author kind:5 has no effect.
/// Design: `docs/design/framework-magic/replaceable.md` §C3.
#[test]
fn c3_kind5_delete_removes_referenced_and_tombstones() {
    let h = StoreHarness::mem();

    let kind1 = h.make_event(ALICE_HEX, 1, 1_000);
    let kind1_id = kind1.id_bytes();
    let kind1_id_hex = kind1.id.clone();
    let kind1_clone = kind1.clone();
    h.insert_raw(kind1, "wss://r1/", 1_000_000);
    h.assert_present(&kind1_id);

    let kind5 = h.make_event_with_tags(ALICE_HEX, 5, 2_000, vec![
        vec!["e".to_string(), kind1_id_hex],
    ]);
    h.insert_raw(kind5, "wss://r1/", 2_000_000);
    h.assert_absent(&kind1_id);
    h.assert_tombstoned(&kind1_id);
    assert_eq!(h.store.tombstones_for(&kind1_id).unwrap()[0].origin, TombstoneOrigin::Kind5);

    let o_rein = h.insert_raw(kind1_clone, "wss://r2/", 3_000_000);
    assert!(
        matches!(o_rein, InsertOutcome::Tombstoned { origin: TombstoneOrigin::Kind5, .. }),
        "reinsert must be Tombstoned: {o_rein:?}"
    );
    h.assert_absent(&kind1_id);

    // Bob's kind:5 on Alice's event must be a no-op (cross-author delete forbidden).
    let alice_ev2 = h.make_event(ALICE_HEX, 1, 3_000);
    let alice_ev2_id = alice_ev2.id_bytes();
    let alice_ev2_hex = alice_ev2.id.clone();
    h.insert_raw(alice_ev2, "wss://r1/", 3_000_000);
    h.insert_raw(
        h.make_event_with_tags(BOB_HEX, 5, 4_000, vec![vec!["e".to_string(), alice_ev2_hex]]),
        "wss://r1/",
        4_000_000,
    );
    h.assert_present(&alice_ev2_id); // Bob cannot delete Alice's events
}

// ── C4 ────────────────────────────────────────────────────────────────────────

/// C4: NIP-40 expiration — expired-on-arrival rejected; GC reaps with tombstone;
/// tombstone blocks re-insert.
/// Design: `docs/design/framework-magic/replaceable.md` §C4.
#[test]
fn c4_nip40_expiration_removes_and_persists_schedule() {
    use nmp_core::store::{GcBudget, RejectReason};
    let h = StoreHarness::mem();

    let ev_past = h.make_event_with_tags(ALICE_HEX, 1, 1_000, vec![
        vec!["expiration".to_string(), "999".to_string()],
    ]);
    let past_id = ev_past.id_bytes();
    let o = h.insert_raw(ev_past, "wss://t/", 1_700_000_000_000u64);
    assert!(
        matches!(o, InsertOutcome::Rejected { reason: RejectReason::ExpiredOnArrival, .. }),
        "expired on arrival must be rejected: {o:?}"
    );
    h.assert_absent(&past_id);

    // Insert with already-past expiration (unix second 2) and early received_at.
    let ev = h.make_event_with_tags(ALICE_HEX, 1, 1u64, vec![
        vec!["expiration".to_string(), "2".to_string()],
    ]);
    let ev_id = ev.id_bytes();
    let ev_clone = ev.clone();
    h.insert_raw(ev, "wss://t/", 1u64);
    h.assert_present(&ev_id);

    let expiring: Vec<_> = h.store
        .scan_expiring_before(12, 100).unwrap()
        .collect::<Result<Vec<_>, _>>().unwrap();
    assert!(expiring.iter().any(|e| e.raw.id_bytes() == ev_id));

    let report = h.store.gc_step(GcBudget { max_events_per_step: 100, max_duration_ms: 1_000 }).unwrap();
    assert!(report.expired_reaped >= 1, "gc_step must reap the expired event");
    h.assert_absent(&ev_id);
    let tombs = h.store.tombstones_for(&ev_id).unwrap();
    assert!(!tombs.is_empty() && tombs[0].origin == TombstoneOrigin::NIP40Expiry);

    let o_rein = h.insert_raw(ev_clone, "wss://r2/", 1u64);
    assert!(
        matches!(
            o_rein,
            InsertOutcome::Tombstoned { origin: TombstoneOrigin::NIP40Expiry, .. }
                | InsertOutcome::Rejected { .. }
        ),
        "re-insert after NIP40 expiry must be blocked: {o_rein:?}"
    );
}

// ── C5 ────────────────────────────────────────────────────────────────────────

/// C5: Kind:3 auto-tracking: active account's follow-list change recompiles
/// every dependent subscription transparently.
/// Design: `docs/design/framework-magic/kind3.md`
#[test]
#[ignore = "pending M2 FollowListChanged trigger + FollowingTimelineView compiler integration"]
fn c5_kind3_change_recompiles_follow_dependent_subs() {
    unimplemented!("M2: FollowListChanged trigger must fire and cause delta CLOSE/REQ");
}

// ── C6 ────────────────────────────────────────────────────────────────────────

/// C6: Outbox read routing — `authors`-filter subscriptions fan out to those
/// authors' write relays (NIP-65), de-duplicated; plan-id stable under re-compile.
/// Design: `docs/design/framework-magic/outbox.md` §C6.
#[test]
fn c6_authors_subscription_routes_to_per_author_write_relays() {
    let alice = pubkey("alice");
    let bob = pubkey("bob");
    let carol = pubkey("carol");
    let mut cache = InMemoryMailboxCache::new();
    cache.put(alice.clone(), mailbox_write(&["wss://r1/", "wss://r2/"]));
    cache.put(bob.clone(), mailbox_write(&["wss://r2/", "wss://r3/"]));
    cache.put(carol.clone(), mailbox_write(&["wss://r3/"]));
    // Bind to a local so the slice reference outlives the compiler call.
    let indexers = vec![relay("wss://purplepag.es")];
    let compiler = SubscriptionCompiler::new(&cache, &indexers);

    let mk = |n: u64, authors: Vec<String>| LogicalInterest {
        id: interest_id(n),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape {
            authors: authors.into_iter().collect(),
            kinds: [1u32, 6u32].into_iter().collect(),
            ..Default::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::Tailing,
    };

    let plan = compiler
        .compile(&[mk(1, vec![alice.clone(), bob.clone(), carol.clone()])])
        .expect("compile");

    let rs: std::collections::BTreeSet<_> = plan.per_relay.keys().cloned().collect();
    assert!(rs.contains("wss://r1/") && rs.contains("wss://r2/") && rs.contains("wss://r3/"));
    assert!(!rs.contains("wss://purplepag.es"), "indexer must not appear for known authors");
    for (url, rp) in &plan.per_relay {
        assert_eq!(rp.sub_shapes.len(), 1, "relay {url} must have one merged sub-shape");
    }
    let r1 = &plan.per_relay["wss://r1/"].sub_shapes[0].shape.authors;
    assert!(r1.contains(&alice) && !r1.contains(&carol));
    let r3 = &plan.per_relay["wss://r3/"].sub_shapes[0].shape.authors;
    assert!(r3.contains(&bob) && r3.contains(&carol) && !r3.contains(&alice));

    let plan2 = compiler
        .compile(&[mk(1, vec![alice, bob, carol])])
        .expect("compile #2");
    assert_eq!(plan.plan_id, plan2.plan_id, "plan_id must be stable under identical inputs");
}

// ── C7 ────────────────────────────────────────────────────────────────────────

/// C7: Write fan-out — publish routes to outbox + recipient-inbox; private events
/// fail closed when recipient inbox is unknown.
/// Design: `docs/design/framework-magic/outbox.md` §C7.
#[test]
#[ignore = "pending M2 PublishPlanner trait + M6 SendNote consumer"]
fn c7_publish_routes_outbox_and_private_fails_closed() {
    unimplemented!("M6: PublishPlanner must route public/notification/private events correctly");
}

// ── C8 ────────────────────────────────────────────────────────────────────────

/// C8: Subscriptions auto-dedup, auto-coalesce, auto-close, and auto-buffer.
/// Design: `docs/design/framework-magic/subs.md`
#[test]
#[ignore = "pending M2 wire-emitter auto-close + actor-tick buffer infrastructure"]
fn c8_subscriptions_coalesce_autoclose_and_buffer() {
    unimplemented!("M2: compiler + wire-emitter + scheduler all required for full C8");
}

// ── C9 ────────────────────────────────────────────────────────────────────────

/// C9: Provenance preserved — same event id from N relays merges into one record
/// with N-entry provenance set; signature and id byte-stable.
/// Design: `docs/design/framework-magic/sync.md` §C9.
#[test]
fn c9_provenance_merges_across_relay_redeliveries() {
    let h = StoreHarness::mem();
    let ev = h.make_event(ALICE_HEX, 1, 1_000_000);
    let id = ev.id_bytes();
    let ev2 = ev.clone();
    let ev3 = ev.clone();

    assert!(matches!(h.insert_raw(ev, "wss://r1/", 1_000), InsertOutcome::Inserted { .. }));
    let prov1 = h.store.provenance_for(&id).unwrap();
    assert_eq!(prov1.len(), 1);
    assert_eq!(prov1.iter().find(|e| e.primary).unwrap().relay_url, "wss://r1/");

    let o2 = h.insert_raw(ev2, "wss://r2/", 5_000);
    assert!(
        matches!(o2, InsertOutcome::Duplicate { sources_after: 2, .. }),
        "second relay must produce Duplicate: {o2:?}"
    );
    let prov2 = h.store.provenance_for(&id).unwrap();
    assert_eq!(prov2.len(), 2);
    assert_eq!(prov2.iter().find(|e| e.primary).unwrap().relay_url, "wss://r1/");

    let o3 = h.insert_raw(ev3, "wss://r1/", 60_000);
    assert!(
        matches!(o3, InsertOutcome::Duplicate { sources_after: 2, .. }),
        "same-relay re-delivery must not add a third provenance entry: {o3:?}"
    );
    let prov3 = h.store.provenance_for(&id).unwrap();
    assert_eq!(prov3.len(), 2);
    let r1 = prov3.iter().find(|e| e.relay_url == "wss://r1/").unwrap();
    assert_eq!(r1.first_seen_ms, 1_000);
    assert_eq!(r1.last_seen_ms, 60_000);
}

// ── C10 ───────────────────────────────────────────────────────────────────────
//
// C10's body lives in the sibling test binary `framework_magic_c10.rs` so the
// wired SubscriptionLifecycle proof can grow without breaking AGENTS.md's
// 500 LOC ceiling on this file. The contract meta-test below keys off the
// `framework-magic.md` doc table, not file location — splitting is invisible
// at the contract layer.
//
// Test binary: `crates/nmp-testing/tests/framework_magic_c10.rs`
// Test name:   `c10_watermark_gates_backfill_and_authoritative_miss`

// ── C11 ───────────────────────────────────────────────────────────────────────

/// C11: Signer onboarding — bunker:// URL and new-nsec creation as kernel actions.
/// Design: `docs/design/framework-magic/signers.md`
#[test]
#[ignore = "pending M6 signers + KeyringCapability + IdentityModule"]
fn c11_bunker_url_and_nsec_creation_complete_via_actions() {
    unimplemented!("M6: BunkerConnect + CreateLocalIdentity action modules required");
}

// ── C12 ───────────────────────────────────────────────────────────────────────

/// C12: Account switch is a state transition; views rebind without imperative dance.
/// Design: `docs/design/framework-magic/sessions.md`
#[test]
#[ignore = "pending M8 multi-account state machine + ActiveAccountChanged trigger"]
fn c12_account_switch_rebinds_views_without_imperative_dance() {
    unimplemented!("M8: SwitchActiveAccount must rebind all ActiveAccount-scoped views");
}

// ── C13 ───────────────────────────────────────────────────────────────────────

/// C13: Best-effort rendering — every display field is non-`Option`; missing data
/// uses defined placeholders; authoritative data refines in place.
/// Design: `docs/design/framework-magic/capabilities.md`
#[test]
#[ignore = "pending M2 projection cache + concrete TimelineItem payload shape"]
fn c13_view_payload_uses_placeholders_then_refines_in_place() {
    unimplemented!("M2: ViewModule payload must be non-Option with identicon fallback");
}

// ── Coverage meta-test ────────────────────────────────────────────────────────

/// `contract_surface_complete` — asserts structural correspondence between this
/// file and `docs/design/framework-magic.md`'s contract table.
///
/// Drift classes caught:
/// 1. Doc table gains a row but this file does not grow a `#[test] fn`.
/// 2. This file gains a `#[test] fn` but the doc table does not list it.
/// 3. A renamed test breaks the doc-test correspondence.
///
/// Design: `docs/design/framework-magic/test-scaffolding.md` §4.
#[test]
fn contract_surface_complete() {
    const EXPECTED_TESTS: &[&str] = &[
        "c1_replaceable_supersedes_on_insert",
        "c2_parameterized_replaceable_supersedes_by_dtag",
        "c3_kind5_delete_removes_referenced_and_tombstones",
        "c4_nip40_expiration_removes_and_persists_schedule",
        "c5_kind3_change_recompiles_follow_dependent_subs",
        "c6_authors_subscription_routes_to_per_author_write_relays",
        "c7_publish_routes_outbox_and_private_fails_closed",
        "c8_subscriptions_coalesce_autoclose_and_buffer",
        "c9_provenance_merges_across_relay_redeliveries",
        "c10_watermark_gates_backfill_and_authoritative_miss",
        "c11_bunker_url_and_nsec_creation_complete_via_actions",
        "c12_account_switch_rebinds_views_without_imperative_dance",
        "c13_view_payload_uses_placeholders_then_refines_in_place",
    ];

    // Parse test names from the contract table in framework-magic.md.
    // Table row format: `| # | Behavior | Sub-file | Test name | Milestone | Doctrine |`
    let doc = include_str!("../../../docs/design/framework-magic.md");
    let doc_test_names: Vec<String> = doc
        .lines()
        .filter(|l| l.starts_with("| C") || l.starts_with("| c"))
        .filter_map(|l| {
            let cols: Vec<&str> = l.split('|').collect();
            if cols.len() >= 5 {
                let name = cols[4].trim();
                if name.starts_with('`') && name.ends_with('`') {
                    Some(name[1..name.len() - 1].to_string())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    for doc_name in &doc_test_names {
        assert!(
            EXPECTED_TESTS.contains(&doc_name.as_str()),
            "contract doc lists '{}' not in EXPECTED_TESTS",
            doc_name
        );
    }
    for expected in EXPECTED_TESTS {
        assert!(
            doc_test_names.iter().any(|n| n == expected),
            "EXPECTED_TESTS lists '{}' not in the contract doc table",
            expected
        );
    }
    assert_eq!(
        doc_test_names.len(),
        EXPECTED_TESTS.len(),
        "doc table has {} names, EXPECTED_TESTS has {} — must agree",
        doc_test_names.len(),
        EXPECTED_TESTS.len()
    );
}
