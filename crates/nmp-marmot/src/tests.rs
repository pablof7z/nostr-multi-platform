//! `nmp-marmot` in-crate tests.
//!
//! **MDK + NIP-59 round-trip** — publish key package → create group →
//! gift-wrap Welcome → unwrap → join → message round-trip using in-memory
//! storage + explicit keys, driven entirely through the public
//! [`crate::service::MarmotService`] API (the same surface a headless
//! integration-test driver uses).
//!
//! The FULL exit-gate proofs (forward-secrecy, post-compromise, perf) are a
//! separate task in `nmp-testing/tests/marmot_*.rs`; this file proves the
//! crate's public API supports them.

use mdk_core::prelude::{MessageProcessingResult, NostrGroupConfigData};
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::{EventBuilder, Keys, Kind, PublicKey, RelayUrl};

use crate::service::MarmotService;

// ─── MDK + NIP-59 round-trip via the service API ─────────────────────────────

fn in_memory_service(keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

fn test_relays() -> Vec<RelayUrl> {
    vec![RelayUrl::parse("wss://test.relay").unwrap()]
}

/// Full round-trip: Bob publishes a key package; Alice creates a group with
/// Bob's key package; Alice GIFT-WRAPS the Welcome (NIP-59) to Bob; Bob
/// unwraps + processes + accepts; Bob does the mandatory post-join
/// self-update; Alice sends a message; Bob decrypts it. Exercises the exact
/// public API a headless integration-test driver uses, including the real
/// `nmp_nip59` gift-wrap path.
#[test]
fn marmot_full_round_trip_create_giftwrap_join_message() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let alice = in_memory_service(alice_keys.clone());
    let bob = in_memory_service(bob_keys.clone());

    // Bob publishes a KeyPackage (dual kind:30443 + 443).
    let bob_kp = bob
        .publish_key_package(test_relays())
        .expect("bob key package");
    assert_eq!(bob_kp.event_30443.kind, Kind::Custom(30443));
    assert_eq!(bob_kp.event_443.kind, Kind::Custom(443));
    assert!(!bob_kp.d_tag.is_empty());
    alice
        .validate_peer_key_package(&bob_kp.event_30443)
        .expect("alice validates bob kp");

    // Alice creates the group inviting Bob.
    let config = NostrGroupConfigData::new(
        "Marmot Test".to_string(),
        "round-trip".to_string(),
        None,
        None,
        None,
        test_relays(),
        vec![alice_keys.public_key()],
    );
    let (group, pending) = alice
        .create_group(vec![bob_kp.event_30443.clone()], config)
        .expect("alice creates group");
    let group_id = group.mls_group_id.clone();
    assert_eq!(pending.welcome_rumors.len(), 1, "one welcome for Bob");
    let bob_welcome_rumor = pending.welcome_rumors[0].clone();
    assert_eq!(bob_welcome_rumor.kind, Kind::MlsWelcome);

    // Alice NIP-59 gift-wraps the Welcome to Bob (real nmp_nip59 path).
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), bob_welcome_rumor)
        .expect("alice gift-wraps welcome");
    assert_eq!(gift.kind, Kind::GiftWrap);

    // Publish-success path: merge the create commit.
    pending.commit().expect("alice merges create commit");

    // Bob unwraps + processes + accepts the gift-wrapped Welcome.
    let (bob_welcome, sender) = bob
        .unwrap_and_process_welcome(&gift)
        .expect("bob unwraps + processes welcome");
    assert_eq!(sender, alice_keys.public_key(), "seal sender is Alice");
    bob.accept_welcome(&bob_welcome)
        .expect("bob accepts welcome");

    // Membership: Alice's view shows exactly Alice + Bob.
    let members = alice.get_members(&group_id).expect("members");
    assert_eq!(members.len(), 2);
    assert!(members.contains(&alice_keys.public_key()));
    assert!(members.contains(&bob_keys.public_key()));

    // Post-join self-update is mandatory per MIP-02. Bob rotates; Alice
    // processes Bob's commit so both converge on the new epoch.
    let bob_su = bob.self_update(&group_id).expect("bob self_update");
    let bob_commit = bob_su.evolution_event.clone();
    bob_su.commit().expect("bob merges self_update");
    match alice
        .process_message(&bob_commit)
        .expect("alice processes bob commit")
    {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("expected Commit, got {other:?}"),
    }

    // Alice sends an application message; Bob decrypts it.
    let rumor =
        EventBuilder::new(Kind::TextNote, "hello bob").build(alice_keys.public_key());
    let msg_event = alice
        .create_message(&group_id, rumor)
        .expect("alice creates message");
    assert_eq!(msg_event.kind, Kind::MlsGroupMessage);

    match bob
        .process_message(&msg_event)
        .expect("bob processes message")
    {
        MessageProcessingResult::ApplicationMessage(m) => {
            assert_eq!(m.content, "hello bob");
            assert_eq!(m.pubkey, alice_keys.public_key());
        }
        other => panic!("expected ApplicationMessage, got {other:?}"),
    }
}

/// The publish-FAILURE path: clear() unblocks future group ops
/// (mdk-api.md §7.7). After clear, a subsequent self_update must succeed.
#[test]
fn pending_change_clear_unblocks_group_ops() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let alice = in_memory_service(alice_keys.clone());
    let bob = in_memory_service(bob_keys.clone());

    let bob_kp = bob.publish_key_package(test_relays()).unwrap();
    let config = NostrGroupConfigData::new(
        "g".into(),
        "d".into(),
        None,
        None,
        None,
        test_relays(),
        vec![alice_keys.public_key()],
    );
    let (group, pending) = alice
        .create_group(vec![bob_kp.event_30443], config)
        .unwrap();
    let group_id = group.mls_group_id.clone();
    pending.commit().unwrap();

    // Simulate a publish failure on a self_update evolution_event: clear()
    // must reset the pending commit so a later op is not wedged.
    let su = alice.self_update(&group_id).expect("self_update");
    su.clear().expect("clear pending commit on publish failure");

    // A fresh self_update now succeeds (group not wedged).
    let su2 = alice.self_update(&group_id).expect("self_update after clear");
    su2.commit().expect("merge after clear");
}

// ─── Multi-actor lifecycle test scaffolding ──────────────────────────────────

/// One MLS actor: its identity keys + service. Returned by [`new_actor`] so
/// multi-actor lifecycle tests can keep a stable handle on each peer.
struct Actor {
    keys: Keys,
    service: MarmotService,
}

impl Actor {
    fn pubkey(&self) -> PublicKey {
        self.keys.public_key()
    }
}

/// Build a fresh actor with independent in-memory MLS storage. Each call to
/// `MdkSqliteStorage::new_in_memory()` yields a private SQLite handle, so
/// actors never share ratchet state (the round-trip test already relies on
/// this; multi-actor tests below exercise it harder).
fn new_actor() -> Actor {
    let keys = Keys::generate();
    Actor {
        service: in_memory_service(keys.clone()),
        keys,
    }
}

/// A standard group config naming `admins` as the admin set.
fn group_config(admins: Vec<PublicKey>) -> NostrGroupConfigData {
    NostrGroupConfigData::new(
        "Lifecycle Test".to_string(),
        "lifecycle".to_string(),
        None,
        None,
        None,
        test_relays(),
        admins,
    )
}

/// Have `admin` create a group with `joiner` invited. Performs the full
/// create → gift-wrap → unwrap → accept → post-join self-update dance and
/// converges both peers on the post-join epoch. Returns the group id.
fn bootstrap_pair(admin: &Actor, joiner: &Actor) -> mdk_core::prelude::GroupId {
    let joiner_kp = joiner
        .service
        .publish_key_package(test_relays())
        .expect("joiner key package");
    let config = group_config(vec![admin.pubkey()]);
    let (group, pending) = admin
        .service
        .create_group(vec![joiner_kp.event_30443.clone()], config)
        .expect("admin creates group");
    let group_id = group.mls_group_id.clone();

    // Deliver the Welcome to the joiner via the real NIP-59 gift-wrap path.
    let rumor = pending.welcome_rumors[0].clone();
    let gift = admin
        .service
        .wrap_welcome(&joiner.pubkey(), rumor)
        .expect("admin gift-wraps welcome");
    pending.commit().expect("admin merges create commit");

    let (welcome, _) = joiner
        .service
        .unwrap_and_process_welcome(&gift)
        .expect("joiner processes welcome");
    joiner
        .service
        .accept_welcome(&welcome)
        .expect("joiner accepts welcome");

    // MIP-02 mandatory post-join self-update; admin processes the commit so
    // both converge.
    let su = joiner.service.self_update(&group_id).expect("post-join self_update");
    let su_commit = su.evolution_event.clone();
    su.commit().expect("joiner merges self_update");
    match admin
        .service
        .process_message(&su_commit)
        .expect("admin processes joiner self_update")
    {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("expected Commit from post-join self_update, got {other:?}"),
    }
    group_id
}

// ─── add_members: existing group grows; both peers converge ──────────────────

/// Add a third member to an existing two-member group. The admin's view AND
/// the existing member's view must both project the new member count of 3
/// once the kind:445 commit is processed.
#[test]
fn add_members_grows_group_and_both_views_converge() {
    let alice = new_actor();
    let bob = new_actor();
    let carol = new_actor();

    let group_id = bootstrap_pair(&alice, &bob);
    assert_eq!(
        alice.service.get_members(&group_id).unwrap().len(),
        2,
        "alice + bob before invite"
    );

    // Carol publishes a KeyPackage; Alice (admin) adds her.
    let carol_kp = carol
        .service
        .publish_key_package(test_relays())
        .expect("carol key package");
    let pending = alice
        .service
        .add_members(&group_id, std::slice::from_ref(&carol_kp.event_30443))
        .expect("alice adds carol");
    let add_commit = pending.evolution_event.clone();
    let carol_rumor = pending.welcome_rumors[0].clone();
    let carol_gift = alice
        .service
        .wrap_welcome(&carol.pubkey(), carol_rumor)
        .expect("alice gift-wraps carol welcome");
    pending.commit().expect("alice merges add commit");

    // Alice's projection now shows 3 members.
    let alice_members = alice.service.get_members(&group_id).unwrap();
    assert_eq!(alice_members.len(), 3, "alice sees 3 after add");
    assert!(alice_members.contains(&carol.pubkey()));

    // Bob (an existing member) processes the add commit and converges to 3.
    match bob
        .service
        .process_message(&add_commit)
        .expect("bob processes add commit")
    {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("expected Commit, got {other:?}"),
    }
    let bob_members = bob.service.get_members(&group_id).unwrap();
    assert_eq!(bob_members.len(), 3, "bob converges to 3 after add commit");
    assert!(bob_members.contains(&carol.pubkey()));

    // Carol joins via her Welcome and also sees the 3-member group.
    let (carol_welcome, _) = carol
        .service
        .unwrap_and_process_welcome(&carol_gift)
        .expect("carol processes welcome");
    carol
        .service
        .accept_welcome(&carol_welcome)
        .expect("carol accepts welcome");
    assert_eq!(
        carol.service.get_members(&group_id).unwrap().len(),
        3,
        "carol's joined view shows 3 members"
    );
}

// ─── remove_members: group shrinks; remaining peer converges ─────────────────

/// Remove a member from a three-member group. The admin's projection AND a
/// remaining member's projection must both fall to 2.
#[test]
fn remove_members_shrinks_group_and_view_converges() {
    let alice = new_actor();
    let bob = new_actor();
    let carol = new_actor();

    let group_id = bootstrap_pair(&alice, &bob);

    // Grow to 3 (Carol).
    let carol_kp = carol.service.publish_key_package(test_relays()).unwrap();
    let add = alice
        .service
        .add_members(&group_id, &[carol_kp.event_30443])
        .unwrap();
    let add_commit = add.evolution_event.clone();
    add.commit().unwrap();
    bob.service.process_message(&add_commit).unwrap();
    assert_eq!(alice.service.get_members(&group_id).unwrap().len(), 3);

    // Alice removes Carol.
    let removal = alice
        .service
        .remove_members(&group_id, &[carol.pubkey()])
        .expect("alice removes carol");
    let remove_commit = removal.evolution_event.clone();
    removal.commit().expect("alice merges remove commit");

    let alice_members = alice.service.get_members(&group_id).unwrap();
    assert_eq!(alice_members.len(), 2, "alice sees 2 after remove");
    assert!(!alice_members.contains(&carol.pubkey()), "carol removed");

    // Bob processes the remove commit and converges to 2.
    match bob
        .service
        .process_message(&remove_commit)
        .expect("bob processes remove commit")
    {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("expected Commit, got {other:?}"),
    }
    let bob_members = bob.service.get_members(&group_id).unwrap();
    assert_eq!(bob_members.len(), 2, "bob converges to 2 after remove");
    assert!(!bob_members.contains(&carol.pubkey()));
}

// ─── leave_group: SelfRemove proposal; commit() does NOT merge ───────────────

/// `leave_group` is a SelfRemove proposal: a peer commits the epoch, so the
/// leaver's `PendingGroupChange::commit()` is a documented no-op. The admin
/// processes the leaver's proposal as a Proposal (not a Commit).
#[test]
fn leave_group_is_self_remove_and_commit_is_noop() {
    let alice = new_actor();
    let bob = new_actor();
    let group_id = bootstrap_pair(&alice, &bob);
    assert_eq!(alice.service.get_members(&group_id).unwrap().len(), 2);

    // Bob leaves: SelfRemove proposal.
    let leave = bob.service.leave_group(&group_id).expect("bob leaves");
    let leave_event = leave.evolution_event.clone();
    // commit() on a SelfRemove handle must succeed WITHOUT merging an MLS
    // commit (no pending commit was created — a peer commits the epoch).
    leave.commit().expect("bob's SelfRemove commit is a no-op");

    // The admin processes Bob's leave proposal. A bare `leave_group` emits a
    // SelfRemove *Proposal* — a peer (admin) commits the epoch later. If MDK
    // ever starts auto-committing here this assertion catches the regression.
    match alice
        .service
        .process_message(&leave_event)
        .expect("alice processes bob's leave")
    {
        MessageProcessingResult::Proposal(_) => {}
        other => panic!("expected Proposal for a SelfRemove leave, got {other:?}"),
    }
}

// ─── decline_welcome: invitee rejects; group is Inactive, not joined ─────────

/// An invitee who declines a Welcome does NOT join the group. MDK keeps the
/// group record (created `Pending` by `process_welcome`) but `decline_welcome`
/// flips it to `GroupState::Inactive` — verified against mdk-core 0.8.0
/// `welcomes.rs` (`process_welcome` → Pending; `decline_welcome` → Inactive).
/// The invariant a UI relies on: the declined group is never `Active`.
#[test]
fn decline_welcome_leaves_group_inactive_for_invitee() {
    use mdk_core::prelude::group_types::GroupState;

    let alice = new_actor();
    let bob = new_actor();

    let bob_kp = bob.service.publish_key_package(test_relays()).unwrap();
    let (group, pending) = alice
        .service
        .create_group(
            vec![bob_kp.event_30443],
            group_config(vec![alice.pubkey()]),
        )
        .expect("alice creates group");
    let group_id = group.mls_group_id.clone();
    let rumor = pending.welcome_rumors[0].clone();
    let gift = alice
        .service
        .wrap_welcome(&bob.pubkey(), rumor)
        .expect("alice gift-wraps");
    pending.commit().unwrap();

    let (welcome, _) = bob
        .service
        .unwrap_and_process_welcome(&gift)
        .expect("bob processes welcome");
    bob.service
        .decline_welcome(&welcome)
        .expect("bob declines welcome");

    // Bob declined: the group record exists but is Inactive — never Active.
    let bob_group = bob
        .service
        .get_group(&group_id)
        .expect("get_group")
        .expect("declined group record is retained as Inactive");
    assert_eq!(
        bob_group.state,
        GroupState::Inactive,
        "a declined welcome must leave the group Inactive, never Active"
    );
}

// ─── Read projections: get_groups / get_messages / group_leaf_map ────────────

/// The read projections that back the Domain/View modules must reflect the
/// real MLS state: `get_groups` lists the created group, `get_messages`
/// returns delivered application messages, and `group_leaf_map` is keyed by
/// the exact same pubkey set as `get_members`.
#[test]
fn read_projections_reflect_group_state() {
    let alice = new_actor();
    let bob = new_actor();
    let group_id = bootstrap_pair(&alice, &bob);

    // get_groups lists exactly the one created group.
    let groups = alice.service.get_groups().expect("get_groups");
    assert_eq!(groups.len(), 1, "exactly one group");
    assert_eq!(groups[0].mls_group_id, group_id);

    // group_leaf_map's pubkey set equals get_members.
    let members = alice.service.get_members(&group_id).unwrap();
    let leaf_map = alice.service.group_leaf_map(&group_id).expect("leaf map");
    let leaf_pubkeys: std::collections::BTreeSet<PublicKey> =
        leaf_map.values().cloned().collect();
    assert_eq!(
        leaf_pubkeys, members,
        "leaf map pubkeys must match the member set"
    );

    // An application message round-trips into get_messages on the receiver.
    let rumor = EventBuilder::new(Kind::TextNote, "history check")
        .build(alice.pubkey());
    let msg = alice
        .service
        .create_message(&group_id, rumor)
        .expect("alice creates message");
    bob.service
        .process_message(&msg)
        .expect("bob processes message");
    let history = bob.service.get_messages(&group_id).expect("get_messages");
    assert!(
        history.iter().any(|m| m.content == "history check"),
        "delivered message must surface in bob's get_messages projection"
    );
}

// ─── KeyPackage cache: cache_key_package / cached_key_packages ────────────────

/// The KeyPackage cache (populated by the app's raw-event tap) must round
/// trip: cache a peer's signed event, then retrieve it by pubkey and list it.
#[test]
fn key_package_cache_round_trips() {
    let alice = new_actor();
    let bob = new_actor();
    let carol = new_actor();

    let bob_kp = bob.service.publish_key_package(test_relays()).unwrap();
    // Alice caches Bob's signed kind:30443 event.
    alice.service.cache_key_package(bob_kp.event_30443.clone());

    // Retrieval by pubkey returns exactly Bob's event.
    let cached = alice.service.cached_key_packages(&[bob.pubkey()]);
    assert_eq!(cached.len(), 1, "bob's kp is cached");
    assert_eq!(cached[0].pubkey, bob.pubkey());

    // Carol was never cached — she is filtered out, not returned empty-shaped.
    let mixed = alice
        .service
        .cached_key_packages(&[bob.pubkey(), carol.pubkey()]);
    assert_eq!(mixed.len(), 1, "only cached pubkeys returned");

    // cached_kp_pubkeys lists Bob's hex pubkey.
    let listed = alice.service.cached_kp_pubkeys();
    assert!(listed.contains(&bob.pubkey().to_hex()));
    assert!(!listed.contains(&carol.pubkey().to_hex()));

    // Re-caching the same author overwrites silently (newest wins).
    alice.service.cache_key_package(bob_kp.event_443.clone());
    assert_eq!(
        alice.service.cached_key_packages(&[bob.pubkey()]).len(),
        1,
        "re-cache overwrites, never duplicates"
    );
}

// ─── Error paths ─────────────────────────────────────────────────────────────

/// `leave_group` against a group id that was never created must surface a
/// `MarmotError::Mdk` error, not panic.
#[test]
fn leave_nonexistent_group_errors() {
    let alice = new_actor();
    let bogus = mdk_core::prelude::GroupId::from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    // Map to a borrow-free Result first: an Ok holds a `PendingGroupChange`
    // that borrows `alice.service`, which would outlive the binding here.
    let outcome: std::result::Result<(), crate::service::MarmotError> =
        alice.service.leave_group(&bogus).map(|p| {
            drop(p);
        });
    match outcome {
        Err(crate::service::MarmotError::Mdk(_)) => {}
        Err(other) => panic!("expected Mdk error for unknown group, got {other:?}"),
        Ok(()) => panic!("leaving a non-existent group must not succeed"),
    }
}

/// `remove_members` for a pubkey that is not a member must error rather than
/// silently succeed or panic.
#[test]
fn remove_non_member_errors() {
    let alice = new_actor();
    let bob = new_actor();
    let stranger = new_actor();
    let group_id = bootstrap_pair(&alice, &bob);

    let removed_non_member = alice
        .service
        .remove_members(&group_id, &[stranger.pubkey()])
        .map(drop)
        .is_ok();
    assert!(
        !removed_non_member,
        "removing a non-member must not succeed"
    );
    // The group is not wedged: a real op still works afterwards.
    let su = alice
        .service
        .self_update(&group_id)
        .expect("self_update still works after failed remove");
    su.commit().expect("merge self_update");
}

/// `validate_peer_key_package` against a wrong-kind event (a plain text note)
/// must reject it — it is a pre-flight sanity check for kind:30443/443.
#[test]
fn validate_rejects_non_key_package_event() {
    let alice = new_actor();
    let not_a_kp = EventBuilder::new(Kind::TextNote, "definitely not a key package")
        .sign_with_keys(&alice.keys)
        .expect("sign text note");
    assert!(
        alice.service.validate_peer_key_package(&not_a_kp).is_err(),
        "a kind:1 text note is not a valid KeyPackage event"
    );
}

/// `unwrap_and_process_welcome` against an event that is not a NIP-59
/// gift-wrap must surface an error (GiftWrap unwrap failure), not panic.
#[test]
fn unwrap_rejects_non_gift_wrap_event() {
    let alice = new_actor();
    let not_a_gift = EventBuilder::new(Kind::TextNote, "not a gift wrap")
        .sign_with_keys(&alice.keys)
        .expect("sign text note");
    match alice.service.unwrap_and_process_welcome(&not_a_gift) {
        Err(_) => {}
        Ok(_) => panic!("a kind:1 text note must not unwrap as a Welcome"),
    }
}

/// Dropping a `PendingGroupChange` without commit/clear must NOT wedge the
/// group — the `Drop` impl defensively clears the pending commit.
#[test]
fn dropped_pending_change_does_not_wedge_group() {
    let alice = new_actor();
    let bob = new_actor();
    let group_id = bootstrap_pair(&alice, &bob);

    // Create a pending self_update and drop it WITHOUT commit/clear.
    {
        let pending = alice.service.self_update(&group_id).expect("self_update");
        drop(pending); // Drop impl must clear the pending commit.
    }
    // The group is not wedged: a fresh op succeeds.
    let su = alice
        .service
        .self_update(&group_id)
        .expect("self_update after dropped pending");
    su.commit().expect("merge after dropped pending");
}

// ─── V-61: orphaned commit counter ───────────────────────────────────────────

/// V-61: dropping a `PendingGroupChange` without commit/clear must increment
/// `orphaned_commit_count` so the host can observe the divergence.
///
/// The group must not be wedged (the existing behaviour), AND the counter
/// must record the event so a downstream snapshot can surface it.
#[test]
fn dropped_pending_change_increments_orphaned_commit_count() {
    let alice = new_actor();
    let bob = new_actor();
    let group_id = bootstrap_pair(&alice, &bob);

    assert_eq!(
        alice.service.orphaned_commit_count(),
        0,
        "counter starts at zero"
    );

    // Drop without commit/clear.
    drop(alice.service.self_update(&group_id).expect("self_update"));

    assert_eq!(
        alice.service.orphaned_commit_count(),
        1,
        "one drop must increment the counter exactly once"
    );

    // A second unresolved drop accumulates.
    drop(alice.service.self_update(&group_id).expect("self_update 2"));
    assert_eq!(
        alice.service.orphaned_commit_count(),
        2,
        "second unresolved drop must accumulate"
    );

    // A resolved commit does NOT increment.
    alice
        .service
        .self_update(&group_id)
        .expect("self_update 3")
        .commit()
        .expect("commit");
    assert_eq!(
        alice.service.orphaned_commit_count(),
        2,
        "resolved commit must not increment the counter"
    );

    // A resolved clear does NOT increment.
    alice
        .service
        .self_update(&group_id)
        .expect("self_update 4")
        .clear()
        .expect("clear");
    assert_eq!(
        alice.service.orphaned_commit_count(),
        2,
        "resolved clear must not increment the counter"
    );
}

/// V-61: a SelfRemove (`leave_group`) dropped unresolved must NOT increment
/// the counter — SelfRemove never creates a pending commit on the local side.
#[test]
fn dropped_self_remove_does_not_increment_orphaned_count() {
    let alice = new_actor();
    let bob = new_actor();
    let group_id = bootstrap_pair(&alice, &bob);

    assert_eq!(bob.service.orphaned_commit_count(), 0);
    // Bob is a non-admin member; leave_group is SelfRemove for non-admins.
    // A peer commits it — no local pending commit is created.
    let pending = bob.service.leave_group(&group_id).expect("leave_group");
    drop(pending);
    assert_eq!(
        bob.service.orphaned_commit_count(),
        0,
        "SelfRemove drop must not increment orphaned_commit_count"
    );
}

// ─── V-62: keyring_unavailable surfaced in snapshot ──────────────────────────

/// V-62: a `MarmotProjection` created with `keyring_unavailable = true` must
/// surface that flag in every snapshot so the host can warn the user.
/// This test verifies the snapshot wire shape — the host reads it and may
/// block group features or prompt keychain recovery.
#[test]
fn keyring_unavailable_is_surfaced_in_snapshot() {
    use crate::projection::state::MarmotProjection;

    let service = in_memory_service(Keys::generate());
    // Simulate the path where `credential_store::initialize()` returned
    // `Some(true)` (mock store) — i.e. the real Keychain was not available.
    let proj = MarmotProjection::new(service, true);
    let snap = proj.snapshot(0);
    assert!(
        snap.keyring_unavailable,
        "snapshot.keyring_unavailable must be true when initialized with mock store"
    );
}

/// V-62: a `MarmotProjection` created with `keyring_unavailable = false` must
/// NOT set the flag — the real Keychain is in use, no warning needed.
#[test]
fn keyring_available_not_flagged_in_snapshot() {
    use crate::projection::state::MarmotProjection;

    let service = in_memory_service(Keys::generate());
    // Simulate the path where `credential_store::initialize()` returned
    // `Some(false)` (real Apple Keychain).
    let proj = MarmotProjection::new(service, false);
    let snap = proj.snapshot(0);
    assert!(
        !snap.keyring_unavailable,
        "snapshot.keyring_unavailable must be false when real Keychain is in use"
    );
}
