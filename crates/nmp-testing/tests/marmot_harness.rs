//! Shared harness utilities for Marmot exit-gate integration tests.
//!
//! Imported by each `marmot_*.rs` file via `mod marmot_harness;` (a Rust
//! inline `#[path]` module — each test binary sees its own copy, which is the
//! standard pattern for shared test helpers in `tests/`).
//!
//! ## Storage strategy
//!
//! The advisor recommended uniform `MdkSqliteStorage::new_unencrypted` +
//! `tempfile::TempDir` rather than `new_in_memory`, so that `std::fs::copy`
//! can snapshot the SQLite file at a specific epoch for the post-compromise
//! test.  All helpers here follow that convention; `marmot_post_compromise.rs`
//! uses `snapshot_storage` directly.

use std::path::{Path, PathBuf};

use mdk_core::MdkConfig;
use mdk_core::prelude::NostrGroupConfigData;
use mdk_sqlite_storage::MdkSqliteStorage;
use nostr::{Keys, RelayUrl};
use nmp_marmot::service::MarmotService;
use tempfile::TempDir;

// ─── Storage helpers ──────────────────────────────────────────────────────────

/// Ephemeral directory that lives for the duration of one test.
pub struct TestDir(pub TempDir);

impl TestDir {
    pub fn new() -> Self {
        TestDir(TempDir::new().expect("tempdir"))
    }

    /// Path for a named SQLite file inside this dir.
    pub fn db_path(&self, name: &str) -> PathBuf {
        self.0.path().join(format!("{name}.sqlite"))
    }
}

/// Build an unencrypted (test-only) `MdkSqliteStorage` at `path`.
pub fn unencrypted_storage(path: &Path) -> MdkSqliteStorage {
    MdkSqliteStorage::new_unencrypted(
        path.to_str().expect("utf8 path"),
    )
    .expect("unencrypted sqlite storage")
}

/// Copy the SQLite file at `src` to `dst` — used by the post-compromise test
/// to snapshot Bob's MLS state at epoch N before he rotates to epoch N+1.
///
/// `#[allow(dead_code)]`: this harness is included via `#[path]` in every
/// `marmot_*` test binary; only `marmot_post_compromise` calls this helper, so
/// the other binaries would otherwise warn.
#[allow(dead_code)]
pub fn snapshot_storage(src: &Path, dst: &Path) {
    std::fs::copy(src, dst).expect("snapshot sqlite copy");
}

// ─── MarmotService construction ───────────────────────────────────────────────

/// Build a `MarmotService` backed by an unencrypted SQLite file at `db_path`.
/// `max_past_epochs` defaults to 5 (MDK default).
pub fn service_at(db_path: &Path, keys: Keys) -> MarmotService {
    let storage = unencrypted_storage(db_path);
    MarmotService::from_storage(storage, keys, MdkConfig::default())
}

// ─── Common relay list ────────────────────────────────────────────────────────

pub fn test_relays() -> Vec<RelayUrl> {
    vec![RelayUrl::parse("wss://test.relay").expect("relay url")]
}

// ─── Group config helper ──────────────────────────────────────────────────────

pub fn group_config(name: &str, admin_key: &Keys) -> NostrGroupConfigData {
    NostrGroupConfigData::new(
        name.to_string(),
        "marmot exit-gate test".to_string(),
        None,
        None,
        None,
        test_relays(),
        vec![admin_key.public_key()],
    )
}

// ─── Full setup: Alice creates a group with Bob ───────────────────────────────

/// Alice publishes a key package; Alice creates a group inviting Bob; the
/// Welcome is gift-wrapped and delivered to Bob; Bob unwraps + accepts + does
/// the mandatory post-join self_update; Alice processes Bob's commit.
///
/// Returns the `group_id` with both pre-built services at the same epoch and
/// the group fully active on both sides.
///
/// `#[allow(dead_code)]`: this harness is included via `#[path]` in every
/// `marmot_*` test binary; not every binary builds a two-member group, so the
/// others would otherwise warn.
#[allow(dead_code)]
pub fn setup_two_member_group(
    alice: &MarmotService,
    alice_keys: &Keys,
    bob: &MarmotService,
    bob_keys: &Keys,
    group_name: &str,
) -> mdk_core::prelude::GroupId {
    // Bob publishes key package.
    let bob_kp = bob
        .publish_key_package(test_relays())
        .expect("bob publish kp");
    alice
        .validate_peer_key_package(&bob_kp.event_30443)
        .expect("alice validates bob kp");

    // Alice creates the group.
    let (group, pending) = alice
        .create_group(
            vec![bob_kp.event_30443.clone()],
            group_config(group_name, alice_keys),
        )
        .expect("alice creates group");
    let group_id = group.mls_group_id.clone();
    assert_eq!(pending.welcome_rumors.len(), 1, "one welcome for Bob");
    let bob_rumor = pending.welcome_rumors[0].clone();

    // Gift-wrap the welcome.
    let gift = alice
        .wrap_welcome(&bob_keys.public_key(), bob_rumor, None)
        .expect("alice gift-wraps welcome");

    // Merge the create commit.
    pending.commit().expect("alice merges create commit");

    // Bob unwraps + processes + accepts.
    let (bob_welcome, sender) = bob
        .unwrap_and_process_welcome(&gift)
        .expect("bob unwraps welcome");
    assert_eq!(sender, alice_keys.public_key());
    bob.accept_welcome(&bob_welcome).expect("bob accepts welcome");

    // Post-join self_update (MIP-02 mandatory).
    post_join_self_update(bob, alice, &group_id);

    group_id
}

/// Bob does a post-join self_update; Alice processes the resulting commit.
/// After this call both services are at the same epoch.
pub fn post_join_self_update(
    joiner: &MarmotService,
    peer: &MarmotService,
    group_id: &mdk_core::prelude::GroupId,
) {
    use mdk_core::prelude::MessageProcessingResult;
    let su = joiner
        .self_update(group_id)
        .expect("post-join self_update");
    let commit_event = su.evolution_event.clone();
    su.commit().expect("merge post-join self_update");
    match peer
        .process_message(&commit_event)
        .expect("peer processes self_update commit")
    {
        MessageProcessingResult::Commit { .. } => {}
        other => panic!("expected Commit from self_update, got {other:?}"),
    }
}
