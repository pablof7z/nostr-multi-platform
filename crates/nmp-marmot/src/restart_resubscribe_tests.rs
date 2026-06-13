//! Regression test for the post-restart live-receive fix.
//!
//! Bug: after app restart, already-joined Marmot groups receive no new live
//! kind:445 messages. `register_with_keys` re-pushes the giftwrap inbox
//! interest but never re-registers the per-group kind:445 message feeds for
//! groups loaded from the persisted MDK store. The in-memory `group_relays`
//! cache starts empty on every launch; `subscribe_group_messages` is only
//! called from `cache_group_relays`, whose only callers are in-session ops
//! (create/join). After restart, both the in-memory relay cache and the relay
//! subscriptions are absent.
//!
//! Fix: `MarmotProjection::resubscribe_all_groups` enumerates persisted
//! groups and routes each through `cache_group_relays` (the existing
//! choke-point), seeding the in-memory cache and pushing interests.
//! `register_with_keys` calls it right after the giftwrap inbox push.
//!
//! ## Test strategy
//!
//! A file-backed `MdkSqliteStorage::new_unencrypted` is used so that
//! "dropping session 1 and opening session 2" models a real restart: the
//! second session builds a fresh `MarmotService` against the SAME file path
//! and has no in-memory state from session 1.
//!
//! After `resubscribe_all_groups` the in-memory `group_relays` cache must
//! contain the persisted relay URLs for every previously-joined group. This
//! is the closest observable proxy for "interests were pushed" that does not
//! require a live kernel — the cache is consulted by every outbound publish
//! and by `subscribe_group_messages` itself.

use mdk_core::prelude::{GroupId, NostrGroupConfigData};
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::{Keys, RelayUrl};

use crate::projection::state::MarmotProjection;
use crate::service::MarmotService;

fn file_backed_service(path: &str, keys: Keys) -> MarmotService {
    let storage =
        MdkSqliteStorage::new_unencrypted(path).expect("file-backed mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

fn group_relay() -> RelayUrl {
    RelayUrl::parse("wss://group.test.relay").unwrap()
}

fn in_memory_service(keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

/// Session 1: create a group with persisted (file-backed) storage and explicit
/// relay URLs. The group relay must round-trip through MDK's `get_relays` API.
///
/// This test is intentionally split from the restart test below so it can be
/// run independently to validate the `MarmotService::group_relays` read seam.
#[test]
fn group_relays_read_seam_round_trips_after_create() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("mls.sqlite");
    let db_path_str = db_path.to_str().unwrap();

    let alice = file_backed_service(db_path_str, alice_keys.clone());

    // Bob is in-memory — only needed to mint a KeyPackage.
    let bob = in_memory_service(bob_keys.clone());
    let bob_kp = bob
        .publish_key_package(vec![group_relay()])
        .expect("bob key package");

    let config = NostrGroupConfigData::new(
        "Relay Round-Trip Test".to_string(),
        "test".to_string(),
        None,
        None,
        None,
        vec![group_relay()],
        vec![alice_keys.public_key()],
    );
    let (group, pending) = alice
        .create_group(vec![bob_kp.event_30443], config)
        .expect("alice creates group");
    pending.commit().expect("merge create commit");

    let group_id = &group.mls_group_id;

    // The `MarmotService::group_relays` read seam must return the configured relay.
    let relays = alice
        .group_relays(group_id)
        .expect("group_relays read");
    assert!(
        !relays.is_empty(),
        "group_relays must be non-empty after create_group with explicit relays"
    );
    assert!(
        relays.contains(&group_relay()),
        "the configured relay URL must round-trip through MDK get_relays"
    );
}

/// Two-session restart scenario (the load-bearing regression proof).
///
/// Session 1: create a group, persist to file, verify `get_relays` non-empty,
///   then DROP the session (simulates app restart — all in-memory state gone).
///
/// Session 2: open the SAME file, build a fresh `MarmotProjection`, call
///   `resubscribe_all_groups`, then assert the in-memory `group_relays` cache
///   for the restarted projection contains the persisted relay URL.
///
/// The assertion proves that the choke-point `cache_group_relays` was called
/// with the persisted relays, which is the same code path that calls
/// `subscribe_group_messages` (and thus `app.push_interest`) in production.
/// End-to-end interest delivery requires a live kernel; asserting the cache
/// contents is the equivalent unit-level oracle.
#[test]
fn resubscribe_all_groups_seeds_relay_cache_after_restart() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("mls-restart.sqlite");
    let db_path_str = db_path.to_str().unwrap();

    // ── Session 1 ────────────────────────────────────────────────────────────
    let group_id_hex: String;
    {
        let alice = file_backed_service(db_path_str, alice_keys.clone());
        let bob = in_memory_service(bob_keys.clone());

        let bob_kp = bob
            .publish_key_package(vec![group_relay()])
            .expect("session1 bob key package");

        let config = NostrGroupConfigData::new(
            "Restart Test Group".to_string(),
            "restart".to_string(),
            None,
            None,
            None,
            vec![group_relay()],
            vec![alice_keys.public_key()],
        );
        let (group, pending) = alice
            .create_group(vec![bob_kp.event_30443], config)
            .expect("session1 alice creates group");
        pending.commit().expect("session1 merge create commit");

        let group_id = &group.mls_group_id;
        group_id_hex = hex_encode(group_id.as_slice());

        // Verify the relay persisted in session 1 before we drop it.
        let relays = alice
            .group_relays(group_id)
            .expect("session1 group_relays");
        assert!(
            !relays.is_empty(),
            "session 1: group_relays must be non-empty before restart"
        );
        assert!(
            relays.contains(&group_relay()),
            "session 1: configured relay must persist"
        );

        // Drop alice (session 1 ends here — all in-memory state is gone).
    }

    // ── Session 2 (restart) ──────────────────────────────────────────────────
    {
        // Open the SAME SQLite file — fresh MarmotService, no in-memory state.
        let alice2 = file_backed_service(db_path_str, alice_keys.clone());
        // `null` app — no live kernel; `subscribe_group_messages` will no-op
        // on the `app()` guard (`None` for null pointer). We assert the cache
        // instead (the observable proxy that does not require a live kernel).
        let proj2 = MarmotProjection::new(alice2, false);

        // Before resubscribe: the in-memory group_relays cache must be EMPTY.
        // (This was the bug — the cache was never seeded on restart.)
        let before = proj2
            .with_inner(|h| h.group_relays(&group_id_hex))
            .unwrap_or_default();
        assert!(
            before.is_empty(),
            "before resubscribe: in-memory group_relays cache must start empty (proving the bug)"
        );

        // Call the fix.
        proj2.resubscribe_all_groups();

        // After resubscribe: the in-memory cache must contain the persisted relay.
        let after = proj2
            .with_inner(|h| h.group_relays(&group_id_hex))
            .unwrap_or_default();
        assert!(
            !after.is_empty(),
            "after resubscribe_all_groups: in-memory group_relays must be non-empty"
        );
        assert!(
            after.contains(&group_relay()),
            "after resubscribe_all_groups: the persisted relay must be in the in-memory cache"
        );
    }
}

/// Verify `resubscribe_all_groups` is a no-op when there are no persisted groups
/// (first launch / fresh install). Must not panic.
#[test]
fn resubscribe_all_groups_is_noop_on_fresh_store() {
    let alice_keys = Keys::generate();
    let service = in_memory_service(alice_keys);
    let proj = MarmotProjection::new(service, false);
    // Must not panic and must complete without error.
    proj.resubscribe_all_groups();
    // Snapshot must still be empty and valid.
    let snap = proj.snapshot(0);
    assert!(snap.groups.is_empty(), "fresh store: groups must be empty after resubscribe");
}

/// Verify idempotency: calling `resubscribe_all_groups` twice must produce the
/// same relay cache state as calling it once (deterministic interest ids mean
/// the second call is a pure no-op at the kernel level).
#[test]
fn resubscribe_all_groups_is_idempotent() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("mls-idempotent.sqlite");
    let db_path_str = db_path.to_str().unwrap();

    // Session 1: create a group.
    let group_id_hex: String;
    {
        let alice = file_backed_service(db_path_str, alice_keys.clone());
        let bob = in_memory_service(bob_keys.clone());
        let bob_kp = bob.publish_key_package(vec![group_relay()]).unwrap();
        let config = NostrGroupConfigData::new(
            "Idempotent Test".to_string(),
            "idempotent".to_string(),
            None,
            None,
            None,
            vec![group_relay()],
            vec![alice_keys.public_key()],
        );
        let (group, pending) = alice.create_group(vec![bob_kp.event_30443], config).unwrap();
        pending.commit().unwrap();
        group_id_hex = hex_encode(group.mls_group_id.as_slice());
    }

    // Session 2: call resubscribe twice; relay set must be the same.
    let alice2 = file_backed_service(db_path_str, alice_keys);
    let proj2 = MarmotProjection::new(alice2, false);

    proj2.resubscribe_all_groups();
    let after_first = proj2
        .with_inner(|h| h.group_relays(&group_id_hex))
        .unwrap_or_default();

    proj2.resubscribe_all_groups();
    let after_second = proj2
        .with_inner(|h| h.group_relays(&group_id_hex))
        .unwrap_or_default();

    assert_eq!(
        after_first, after_second,
        "resubscribe_all_groups must be idempotent: relay cache must be the same after two calls"
    );
    assert!(
        !after_second.is_empty(),
        "relay cache must be populated after idempotent double-call"
    );
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

// ─── Security: Inactive group filter in resubscribe_all_groups ───────────────

/// `resubscribe_all_groups` must NOT re-subscribe a group that the local
/// identity has declined (state = Inactive).  Resuming subscriptions for
/// Inactive groups would leak relay-side metadata and re-open channels the
/// user explicitly closed.
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
        bob.decline_welcome(&welcome)
            .expect("bob declines welcome");

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
        let proj2 = MarmotProjection::new(bob2, false);

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

    // ── Session 1: alice creates two groups, bob joins one and declines the
    //   other; carol is the observer whose file-backed store has both records.
    //
    // We cheat slightly: alice creates one group with carol as sole admin
    // (so carol's copy is Active) and one group where carol declines.
    // Alice is ephemeral (in-memory); carol is file-backed.
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
        carol.decline_welcome(&welcome2).expect("carol declines welcome 2");
    }

    // ── Session 2: open carol's store in a fresh projection, resubscribe,
    //   assert Active group IS seeded and Inactive group is NOT seeded.
    {
        let carol2 = file_backed_service(db_path_str, carol_keys.clone());
        let proj2 = MarmotProjection::new(carol2, false);

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
