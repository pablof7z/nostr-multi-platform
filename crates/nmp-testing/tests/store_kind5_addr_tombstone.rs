//! §2.7a Kind:5 `a`-tag delete arriving before the target event.
//!
//! Also tests that foreign kind:5 pre-tombstone does NOT block the rightful
//! event owner (regression test for mem.rs foreign tombstone author check).
//!
//! See `docs/design/lmdb/tests/insert.md` §2.7a.

use nmp_core::store::{InsertOutcome, TombstoneOrigin};
use nmp_testing::for_each_backend;
use nmp_testing::store_harness::{StoreHarness, ALICE_HEX, BOB_HEX};

for_each_backend!(e_tag_self_delete_before_target_suppresses_reinsert, |h: &mut StoreHarness| {
    // Insert kind:5 by Alice referencing a target that doesn't exist yet.
    let target_id_hex = format!("{:0>64}", "aabbcc");
    let kind5 = h.make_event_with_tags(ALICE_HEX, 5, 2_000, vec![
        vec!["e".to_string(), target_id_hex.clone()],
    ]);
    let o5 = h.insert_raw(kind5, "wss://t/", 2_000_000);
    assert!(matches!(o5, InsertOutcome::Inserted { .. }), "{o5:?}");

    // Now the target (owned by Alice) arrives — should be Tombstoned.
    let target = h.make_event_with_id(&target_id_hex, ALICE_HEX, 1, 1_000);
    let target_id = target.id_bytes();
    let o = h.insert_raw(target, "wss://t/", 1_000_000);
    assert!(
        matches!(o, InsertOutcome::Tombstoned { origin: TombstoneOrigin::Kind5, .. }),
        "expected Tombstoned (self-delete pre-tombstone), got {o:?}"
    );
    h.assert_absent(&target_id);
});

for_each_backend!(foreign_e_tag_pre_tombstone_does_not_block_owner, |h: &mut StoreHarness| {
    // Bob inserts a kind:5 referencing Alice's event (foreign delete — should be ignored).
    let target_id_hex = format!("{:0>64}", "ccddee");
    let kind5_bob = h.make_event_with_tags(BOB_HEX, 5, 2_000, vec![
        vec!["e".to_string(), target_id_hex.clone()],
    ]);
    h.insert_raw(kind5_bob, "wss://t/", 2_000_000);

    // Alice's event arrives — Bob's pre-tombstone must NOT block it.
    let target = h.make_event_with_id(&target_id_hex, ALICE_HEX, 1, 1_000);
    let target_id = target.id_bytes();
    let o = h.insert_raw(target, "wss://t/", 1_000_000);
    assert!(
        matches!(o, InsertOutcome::Inserted { .. }),
        "foreign pre-tombstone must not block the rightful owner, got {o:?}"
    );
    h.assert_present(&target_id);
});

for_each_backend!(a_tag_delete_before_event_suppresses_reinsert, |h: &mut StoreHarness| {
    // Kind:5 with `a`-tag arrives before the parameterized replaceable.
    let d_tag = "my-article";
    let addr = format!("30023:{ALICE_HEX}:{d_tag}");
    let kind5 = h.make_event_with_tags(ALICE_HEX, 5, 2_000, vec![
        vec!["a".to_string(), addr],
    ]);
    let o5 = h.insert_raw(kind5, "wss://t/", 2_000_000);
    assert!(
        matches!(o5, InsertOutcome::Inserted { .. } | InsertOutcome::Duplicate { .. }),
        "{o5:?}"
    );

    // The parameterized replaceable arrives (same author, older timestamp).
    let article = h.make_event_with_tags(ALICE_HEX, 30_023, 1_000, vec![
        vec!["d".to_string(), d_tag.to_string()],
    ]);
    let article_id = article.id_bytes();
    let o = h.insert_raw(article, "wss://t/", 1_000_000);
    assert!(
        matches!(o, InsertOutcome::Tombstoned { origin: TombstoneOrigin::Kind5, .. }),
        "expected Tombstoned from a-tag pre-tombstone, got {o:?}"
    );
    h.assert_absent(&article_id);
});

for_each_backend!(duplicate_replaceable_returns_duplicate_not_superseded, |h: &mut StoreHarness| {
    // Regression: inserting the exact same replaceable event twice should be Duplicate,
    // not Superseded. The second insert should merge provenance.
    let ev = h.make_event(ALICE_HEX, 0, 1_000);
    let id = ev.id_bytes();
    let ev2 = ev.clone();

    let o1 = h.insert_raw(ev, "wss://a/", 1_000_000);
    assert!(matches!(o1, InsertOutcome::Inserted { .. }), "{o1:?}");

    let o2 = h.insert_raw(ev2, "wss://b/", 2_000_000);
    assert!(
        matches!(o2, InsertOutcome::Duplicate { sources_after: 2, .. }),
        "identical replaceable reinsert should be Duplicate, got {o2:?}"
    );
    h.assert_present(&id);
});
