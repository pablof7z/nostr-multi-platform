//! Security tests for the Inactive-group filter in `resubscribe_all_groups`.
//!
//! Left/declined groups (`GroupState::Inactive`) must NOT receive live relay
//! subscriptions on restart, and must NOT appear in the snapshot group list.
//! Resuming subscriptions for an Inactive group would leak relay-side metadata
//! and re-open delivery channels the user explicitly closed.
//!
//! These tests are a sibling of `restart_resubscribe_tests` (same wiring in
//! `lib.rs`); they live in their own file purely to keep each test module
//! under the 500 LOC hard cap (AGENTS.md file-size rule).

use mdk_core::prelude::NostrGroupConfigData;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::{Keys, RelayUrl};

use crate::projection::state::MarmotProjection;
use crate::service::MarmotService;

fn file_backed_service(path: &str, keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_unencrypted(path).expect("file-backed mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

fn in_memory_service(keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

fn group_relay() -> RelayUrl {
    RelayUrl::parse("wss://group.test.relay").unwrap()
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// `resubscribe_all_groups` must NOT re-subscribe a group that the local
/// identity has declined (state = Inactive).
///
/// TDD proof: create a group, have an invitee decline it (leaving the record
/// Inactive), then call `resubscribe_all_groups` from a fresh session and
/// assert the declined group's relay cache remains empty.
#[test]
fn resubscribe_all_groups_skips_inactive_groups() {
    use mdk_core::prelude::group_types::GroupState;

    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("mls-inactive.sqlite");
    let db_path_str = db_path.to_str().unwrap();

    let declined_group_id_hex: String;

    // ── Session 1 ────────────────────────────────────────────────────────────
    // Bob joins via alice's invitation, then immediately declines.  Bob's
    // group record flips to Inactive; we persist this via the file-backed store.
    {
        let alice = in_memory_service(alice_keys.clone());

        // Bob uses file-backed storage so the Inactive record survives restart.
        let bob = file_backed_service(db_path_str, bob_keys.clone());

        let bob_kp = bob
            .publish_key_package(vec![group_relay()])
            .expect("bob key package");

        let config = NostrGroupConfigData::new(
            "Inactive Filter Test".to_string(),
            "inactive".to_string(),
            None,
            None,
            None,
            vec![group_relay()],
            vec![alice_keys.public_key()],
        );
        let (group, pending) = alice
            .create_group(vec![bob_kp.event_30443], config)
            .expect("alice creates group");
        let group_id = group.mls_group_id.clone();
        declined_group_id_hex = hex_encode(group_id.as_slice());

        // Alice gift-wraps the welcome for Bob.
        let rumor = pending.welcome_rumors[0].clone();
        let gift = alice
            .wrap_welcome(&bob_keys.public_key(), rumor)
            .expect("alice gift-wraps welcome");
        pending.commit().expect("alice merges create commit");

        // Bob processes and then declines the welcome.
        let (welcome, _) = bob
            .unwrap_and_process_welcome(&gift)
            .expect("bob processes welcome");
        bob.decline_welcome(&welcome).expect("bob declines welcome");

        // Verify: bob's record for this group is Inactive before we restart.
        let bob_group = bob
            .get_group(&group_id)
            .expect("get_group")
            .expect("declined group record retained");
        assert_eq!(
            bob_group.state,
            GroupState::Inactive,
            "declined group must be Inactive before restart"
        );
        // (bob is dropped here — session 1 ends)
    }

    // ── Session 2 (restart) ──────────────────────────────────────────────────
    // Open the SAME file-backed store.  The Inactive group record is present.
    // `resubscribe_all_groups` must NOT seed the relay cache for it.
    {
        let bob2 = file_backed_service(db_path_str, bob_keys.clone());
        let proj2 = MarmotProjection::new(bob2, None);

        proj2.resubscribe_all_groups();

        // The in-memory group_relays cache for the declined group must be empty.
        let relays = proj2
            .with_inner(|h| h.group_relays(&declined_group_id_hex))
            .unwrap_or_default();
        assert!(
            relays.is_empty(),
            "resubscribe_all_groups must NOT seed the relay cache for an Inactive (declined) group"
        );

        // The snapshot must also NOT include the declined group.
        let snap = proj2.snapshot(0);
        assert!(
            snap.groups.is_empty(),
            "snapshot must not surface Inactive groups; got: {:?}",
            snap.groups.iter().map(|g| &g.id_hex).collect::<Vec<_>>()
        );
    }
}

/// `resubscribe_all_groups` must re-subscribe Active groups and skip Inactive
/// ones when both exist in the same store.  Proves the filter is selective,
/// not a blanket suppress.
#[test]
fn resubscribe_all_groups_active_subscribed_inactive_skipped() {
    let alice_keys = Keys::generate();
    let carol_keys = Keys::generate();

    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("mls-mixed.sqlite");
    let db_path_str = db_path.to_str().unwrap();

    let active_group_id_hex: String;
    let declined_group_id_hex: String;

    // ── Session 1: alice creates two groups; carol accepts one (Active) and
    //   declines the other (Inactive).  Carol is file-backed so both records
    //   survive restart; alice is ephemeral (in-memory).
    {
        let alice = in_memory_service(alice_keys.clone());
        let carol = file_backed_service(db_path_str, carol_keys.clone());

        let carol_kp = carol
            .publish_key_package(vec![group_relay()])
            .expect("carol key package");

        // Group 1: carol will accept (Active).
        let config_active = NostrGroupConfigData::new(
            "Active Group".to_string(),
            "active".to_string(),
            None,
            None,
            None,
            vec![group_relay()],
            vec![alice_keys.public_key()],
        );
        let (g1, p1) = alice
            .create_group(vec![carol_kp.event_30443.clone()], config_active)
            .expect("alice creates active group");
        active_group_id_hex = hex_encode(g1.mls_group_id.as_slice());

        let rumor1 = p1.welcome_rumors[0].clone();
        let gift1 = alice
            .wrap_welcome(&carol_keys.public_key(), rumor1)
            .expect("gift-wrap active");
        p1.commit().expect("active group commit");

        let (welcome1, _) = carol
            .unwrap_and_process_welcome(&gift1)
            .expect("carol processes welcome 1");
        carol
            .accept_welcome(&welcome1)
            .expect("carol accepts welcome 1");
        // Mandatory post-join self-update.
        let update = carol
            .self_update(&g1.mls_group_id)
            .expect("carol self-update");
        update.commit().expect("carol self-update commit");

        // Group 2: alice must publish a fresh key package for carol since
        // MDK one-time-use semantics consume the first one.
        let carol_kp2 = carol
            .publish_key_package(vec![group_relay()])
            .expect("carol key package 2");

        let config_declined = NostrGroupConfigData::new(
            "Declined Group".to_string(),
            "declined".to_string(),
            None,
            None,
            None,
            vec![group_relay()],
            vec![alice_keys.public_key()],
        );
        let (g2, p2) = alice
            .create_group(vec![carol_kp2.event_30443], config_declined)
            .expect("alice creates declined group");
        declined_group_id_hex = hex_encode(g2.mls_group_id.as_slice());

        let rumor2 = p2.welcome_rumors[0].clone();
        let gift2 = alice
            .wrap_welcome(&carol_keys.public_key(), rumor2)
            .expect("gift-wrap declined");
        p2.commit().expect("declined group commit");

        let (welcome2, _) = carol
            .unwrap_and_process_welcome(&gift2)
            .expect("carol processes welcome 2");
        carol
            .decline_welcome(&welcome2)
            .expect("carol declines welcome 2");
    }

    // ── Session 2: open carol's store in a fresh projection, resubscribe,
    //   assert Active group IS seeded and Inactive group is NOT seeded.
    {
        let carol2 = file_backed_service(db_path_str, carol_keys.clone());
        let proj2 = MarmotProjection::new(carol2, None);

        proj2.resubscribe_all_groups();

        let active_relays = proj2
            .with_inner(|h| h.group_relays(&active_group_id_hex))
            .unwrap_or_default();
        assert!(
            !active_relays.is_empty(),
            "Active group relay cache must be seeded after resubscribe"
        );

        let declined_relays = proj2
            .with_inner(|h| h.group_relays(&declined_group_id_hex))
            .unwrap_or_default();
        assert!(
            declined_relays.is_empty(),
            "Inactive (declined) group relay cache must remain empty after resubscribe"
        );

        // Snapshot must include only the Active group.
        let snap = proj2.snapshot(0);
        let snap_ids: Vec<&str> = snap.groups.iter().map(|g| g.id_hex.as_str()).collect();
        assert!(
            snap_ids.contains(&active_group_id_hex.as_str()),
            "Active group must appear in snapshot"
        );
        assert!(
            !snap_ids.contains(&declined_group_id_hex.as_str()),
            "Inactive group must NOT appear in snapshot"
        );
    }
}
