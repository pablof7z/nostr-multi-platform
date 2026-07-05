//! #3057 ROUND 3 — KeyPackage private-key lifecycle reproduction + fix.
//!
//! Disambiguates WHY B's MLS store lacks the matching KeyPackage private key at
//! welcome time, empirically (no sim needed), then proves the fix.
//!
//! ## What the reproductions establish
//!
//! Within a STABLE store the KeyPackage private-key lifecycle is CORRECT:
//! the private key is persisted on create, survives a store reopen, and a
//! republish ACCUMULATES a new key package without deleting the old one's
//! private key (`keypackage_private_key_survives_store_reopen`,
//! `republished_keypackage_retains_prior_private_key`). So none of the three
//! candidate mechanisms breaks a clean invite against a stable store.
//!
//! The surviving mechanism is a RELAY-side staleness: MDK mints a fresh RANDOM
//! `d` tag per publish, so when B's store is (re)created — a new device, a
//! cleared keyring db-key, a reinstall — B's republished kind:30443 lands under
//! a NEW address and the PRIOR key package stays alive on the relay under its
//! old `d`. An inviter that fetches the stale one builds a Welcome whose
//! init-key private half lives only in B's OLD (gone) store →
//! `process_welcome` fails "No matching key package". The fix pins a STABLE
//! per-identity `d` so republishes REPLACE (NIP-33) — the relay only ever
//! serves the current key package (`stable_d_tag_makes_republish_replace_not_accumulate`).

use std::collections::HashMap;

use mdk_core::prelude::NostrGroupConfigData;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::{Keys, RelayUrl};

use crate::service::{marmot_key_package_d_tag, MarmotService};

/// Model a relay's NIP-33 replaceable index: `(kind, pubkey, d)` → latest
/// event. Publishing a kind:30443 with a `d` already present REPLACES it.
#[derive(Default)]
struct FakeReplaceableRelay {
    by_addr: HashMap<(u16, String, String), nostr::Event>,
}

impl FakeReplaceableRelay {
    fn publish(&mut self, ev: &nostr::Event) {
        let d = ev
            .tags
            .iter()
            .find_map(|t| {
                let s = t.as_slice();
                (s.first().map(String::as_str) == Some("d"))
                    .then(|| s.get(1).cloned().unwrap_or_default())
            })
            .unwrap_or_default();
        self.by_addr
            .insert((ev.kind.as_u16(), ev.pubkey.to_hex(), d), ev.clone());
    }

    /// Every kind:30443 event currently served for `author` (post-replacement).
    fn served_key_packages(&self, author_hex: &str) -> Vec<nostr::Event> {
        self.by_addr
            .iter()
            .filter(|((kind, pk, _), _)| *kind == 30443 && pk == author_hex)
            .map(|(_, ev)| ev.clone())
            .collect()
    }
}

fn file_backed(path: &str, keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_unencrypted(path).expect("file-backed mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

fn in_memory(keys: Keys) -> MarmotService {
    let storage = MdkSqliteStorage::new_in_memory().expect("in-memory mls storage");
    MarmotService::from_storage(storage, keys, Default::default())
}

fn relays() -> Vec<RelayUrl> {
    vec![RelayUrl::parse("wss://test.relay").unwrap()]
}

/// The stable `d`-tag slot id is deterministic per identity, distinct across
/// identities, and a valid 64-hex-char (32-byte) MDK `d` value.
#[test]
fn key_package_d_tag_is_deterministic_and_valid_length() {
    let a = Keys::generate();
    let b = Keys::generate();

    let da1 = marmot_key_package_d_tag(&a.public_key());
    let da2 = marmot_key_package_d_tag(&a.public_key());
    let db = marmot_key_package_d_tag(&b.public_key());

    assert_eq!(da1, da2, "same identity must yield the same stable slot id");
    assert_ne!(da1, db, "different identities must yield different slot ids");
    assert_eq!(da1.len(), 64, "MDK requires a 64-hex-char (32-byte) d tag");
    assert!(
        da1.chars().all(|c| c.is_ascii_hexdigit()),
        "d tag must be hex"
    );
}

/// CANDIDATE 3 probe — store-lifecycle persistence.
///
/// B publishes a KeyPackage in a file-backed store, the service is dropped
/// (store reopened fresh), then A invites B against the PUBLISHED KeyPackage
/// and B tries to `process_welcome`. If the private key did not survive the
/// store reopen, this fails with "no matching key package".
#[test]
fn keypackage_private_key_survives_store_reopen() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("mls-kp.sqlite");
    let db_path_str = db_path.to_str().unwrap();

    // ── Session 1: B publishes a KeyPackage, then the service is dropped. ──
    let bob_kp_event = {
        let bob = file_backed(db_path_str, bob_keys.clone());
        let kp = bob.publish_key_package(relays()).expect("bob key package");
        kp.event_30443
        // bob dropped here → store closed
    };

    // ── A invites B against the PUBLISHED KeyPackage. ──
    let alice = in_memory(alice_keys.clone());
    let config = NostrGroupConfigData::new(
        "Persist Test".to_string(),
        "persist".to_string(),
        None,
        None,
        None,
        relays(),
        vec![alice_keys.public_key()],
    );
    let (_group, pending) = alice
        .create_group(vec![bob_kp_event.clone()], config)
        .expect("alice creates group against bob's published KP");
    let rumor = pending.welcome_rumors[0].clone();
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), rumor)
        .expect("alice gift-wraps welcome");
    pending.commit().expect("alice merges create commit");

    // ── Session 2: reopen B's SAME store, process the Welcome. ──
    let bob = file_backed(db_path_str, bob_keys.clone());
    let result = bob.unwrap_and_process_welcome(&gift);

    assert!(
        result.is_ok(),
        "B must still hold the KeyPackage private key after a store reopen; \
         process_welcome failed: {:?}",
        result.err()
    );
}

/// CANDIDATE 1/2 probe — republish does NOT orphan the prior private key.
///
/// B publishes KP1, then republishes KP2 in the SAME store. A invites B against
/// the FIRST (older) key package. B must still process it — republish
/// accumulates key material, it does not delete the prior private key, and MDK
/// marks key packages last-resort (reusable). Rules out "republish/rotation
/// deletes the old key" and "single-use consumption" as the failure within a
/// stable store.
#[test]
fn republished_keypackage_retains_prior_private_key() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let bob = in_memory(bob_keys.clone());
    let kp1 = bob.publish_key_package(relays()).expect("bob kp1");
    let _kp2 = bob
        .publish_key_package(relays())
        .expect("bob kp2 (republish)");

    // A invites B against KP1 (the older key package).
    let alice = in_memory(alice_keys.clone());
    let config = NostrGroupConfigData::new(
        "Republish Test".into(),
        "republish".into(),
        None,
        None,
        None,
        relays(),
        vec![alice_keys.public_key()],
    );
    let (_g, pending) = alice
        .create_group(vec![kp1.event_30443.clone()], config)
        .expect("alice creates group against bob kp1");
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), pending.welcome_rumors[0].clone())
        .expect("gift-wrap");
    pending.commit().expect("commit");

    assert!(
        bob.unwrap_and_process_welcome(&gift).is_ok(),
        "B must still process a Welcome built against KP1 after republishing KP2 \
         (republish accumulates; it must not orphan the prior private key)"
    );
}

/// THE #3057 ROUND-3 FIX — a stable `d` makes republishes REPLACE, so the relay
/// never serves a stale key package whose private half B no longer holds.
///
/// Reproduces the production mechanism end-to-end through a NIP-33 replaceable
/// relay model:
///   1. B store instance 1 publishes a key package.
///   2. B's store is (re)created (fresh instance, same identity — models a new
///      device / cleared keyring db-key / reinstall). B republishes.
///   3. Both publishes hit the relay.
///
/// WITH the fix (stable `d`), the relay replaces instance-1's key package with
/// instance-2's, so it serves EXACTLY ONE — the one matching B's live store —
/// and any inviter can only build a processable Welcome. The test also proves
/// the mechanism it prevents: a Welcome built against instance-1's (now stale)
/// key package is UNprocessable by instance-2 (the production black hole).
#[test]
fn stable_d_tag_makes_republish_replace_not_accumulate() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let bob_hex = bob_keys.public_key().to_hex();

    let mut relay = FakeReplaceableRelay::default();

    // ── B store instance 1 publishes a key package, then is discarded. ──
    let stale_kp = {
        let bob1 = in_memory(bob_keys.clone());
        let kp = bob1.publish_key_package(relays()).expect("bob1 kp");
        relay.publish(&kp.event_30443);
        kp.event_30443
        // bob1 dropped → its private key material is gone
    };

    // ── B store instance 2 (fresh, same identity) republishes. ──
    let bob2 = in_memory(bob_keys.clone());
    let fresh = bob2.publish_key_package(relays()).expect("bob2 kp");
    relay.publish(&fresh.event_30443);

    // THE FIX: both publishes share the stable `d`, so the relay REPLACED the
    // stale key package — it serves exactly one, and it is the fresh one.
    let served = relay.served_key_packages(&bob_hex);
    assert_eq!(
        served.len(),
        1,
        "a stable per-identity `d` must make republishes REPLACE on the relay; \
         got {} served key packages (stale accumulation is the #3057 bug)",
        served.len()
    );
    assert_eq!(
        served[0].id, fresh.event_30443.id,
        "the single served key package must be the CURRENT one (matching B's live store)"
    );
    assert_ne!(
        stale_kp.id, fresh.event_30443.id,
        "sanity: the two publishes are distinct events…"
    );
    assert_eq!(
        stale_kp
            .tags
            .iter()
            .find_map(|t| (t.as_slice().first().map(String::as_str) == Some("d"))
                .then(|| t.as_slice().get(1).cloned().unwrap_or_default())),
        fresh
            .event_30443
            .tags
            .iter()
            .find_map(|t| (t.as_slice().first().map(String::as_str) == Some("d"))
                .then(|| t.as_slice().get(1).cloned().unwrap_or_default())),
        "…but they MUST share the same stable `d` slot so the relay replaces"
    );

    // Mechanism proof: an inviter fetching the served (fresh) key package builds
    // a Welcome B's live store CAN process…
    let alice = in_memory(alice_keys.clone());
    let cfg = NostrGroupConfigData::new(
        "Fix".into(),
        "fix".into(),
        None,
        None,
        None,
        relays(),
        vec![alice_keys.public_key()],
    );
    let (_g, pending) = alice
        .create_group(vec![served[0].clone()], cfg)
        .expect("alice invites against served kp");
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), pending.welcome_rumors[0].clone())
        .expect("gift-wrap");
    pending.commit().expect("commit");
    assert!(
        bob2.unwrap_and_process_welcome(&gift).is_ok(),
        "B must process a Welcome built against the served (current) key package"
    );

    // …whereas a Welcome built against the STALE key package (what a naive
    // random-`d` accumulation would still serve) is exactly the black hole the
    // fix removes from the relay.
    let alice2 = in_memory(Keys::generate());
    let cfg2 = NostrGroupConfigData::new(
        "Stale".into(),
        "stale".into(),
        None,
        None,
        None,
        relays(),
        vec![alice2.public_key()],
    );
    let (_g2, pending2) = alice2
        .create_group(vec![stale_kp.clone()], cfg2)
        .expect("alice2 invites against STALE kp");
    let stale_gift = alice2
        .wrap_welcome(&bob_keys.public_key(), pending2.welcome_rumors[0].clone())
        .expect("gift-wrap stale");
    pending2.commit().expect("commit");
    assert!(
        bob2.unwrap_and_process_welcome(&stale_gift).is_err(),
        "a Welcome built against the STALE key package MUST be unprocessable by \
         B's live store — this is the #3057 failure the stable-`d` fix keeps off \
         the relay"
    );
}