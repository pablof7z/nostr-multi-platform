//! PR-B (#991/#979) typed-decode round-trip proof for [`decode_snapshot_typed`].
//!
//! Drives a REAL kernel actor (`nmp_core::testing::spawn_actor` — the same
//! actor loop production uses), signs in a local key, and decodes the emitted
//! FlatBuffers frames through the production typed-first path. Field-level
//! assertions pin the decoded [`crate::snapshot::Snapshot`] against the known
//! signing key — proving the kernel's Tier-3 envelope + typed-sidecar encoders
//! and this shell's decoders agree on a frame built by the real encoder
//! (`encode_snapshot_with_envelope`), with no JSON payload on the wire.

use super::decode_snapshot_typed;
use nmp_core::actor::{IdentityCommand, LifecycleCommand};
use nmp_core::testing::{spawn_actor, ActorCommand};
use std::time::{Duration, Instant};

/// A fixed nsec used only in tests (same key as nmp-testing's e2e pipeline).
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
/// The hex pubkey of `TEST_NSEC`.
const TEST_PUBKEY_HEX: &str = "7e7e9c42a91bfef19fa929e5fda1b72e0ebc1a4c1141673e2794234d86addf4e";

#[test]
fn decode_snapshot_typed_round_trips_real_kernel_frame() {
    let (tx, rx) = spawn_actor();
    tx.send(ActorCommand::Lifecycle(LifecycleCommand::Start {
        visible_limit: 100,
        emit_hz: 0,
        initial_relays: vec![("wss://relay.test".to_string(), "both".to_string())],
    }))
    .expect("send Start");
    tx.send(ActorCommand::Identity(IdentityCommand::AddSigner {
        source: nmp_core::SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        make_active: true,
    }))
    .expect("send AddSigner");
    tx.send(ActorCommand::Lifecycle(
        LifecycleCommand::MarkChangedSinceEmit,
    ))
    .expect("send MarkChangedSinceEmit");

    // Drain real frames until the typed `accounts` sidecar surfaces the
    // signed-in account through the production decode path.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut snapshot = None;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => {
                let Some(decoded) =
                    decode_snapshot_typed(&frame, &mut nmp_core::refs::RefProfileStore::new())
                else {
                    panic!("every live kernel frame must decode through the typed path");
                };
                if !decoded.accounts.is_empty() {
                    snapshot = Some(decoded);
                    break;
                }
            }
            Err(_) => continue,
        }
    }
    let snapshot = snapshot.expect(
        "a real kernel frame must surface the signed-in account through \
         decode_snapshot_typed within 5 s",
    );

    // --- Tier-3 envelope fields ---
    assert!(
        snapshot.rev > 0,
        "rev must round-trip off the typed envelope"
    );
    assert!(
        snapshot.running,
        "running must round-trip off the typed envelope"
    );
    // relay_statuses carries one row per RelayRole (role-aggregate rows may
    // have an empty relay_url when no relay fills that role); the
    // Start-configured relay must surface on at least one row.
    assert!(
        snapshot
            .relay_statuses
            .iter()
            .any(|r| r.relay_url == "wss://relay.test"),
        "the Start-configured relay must appear in relay_statuses: {:?}",
        snapshot.relay_statuses
    );

    // --- identity cluster: accounts + active_account (typed sidecars) ---
    assert_eq!(
        snapshot.active_account.as_deref(),
        Some(TEST_PUBKEY_HEX),
        "active_account must round-trip the signed-in pubkey"
    );
    let account = snapshot
        .accounts
        .iter()
        .find(|a| a.pubkey == TEST_PUBKEY_HEX)
        .expect("the signed-in account must appear in the accounts sidecar");
    assert!(
        account.is_active,
        "the signed-in account must be flagged active"
    );

    // --- profile card (typed sidecar; placeholder until a kind:0 arrives) ---
    assert_eq!(
        snapshot.profile.pubkey, TEST_PUBKEY_HEX,
        "the active account's profile card must round-trip its pubkey"
    );
    // D1 (#606): no `has_profile` render-gate. Before any kind:0 arrives the
    // card is an honest placeholder — its display fields are simply empty/None.
    assert!(
        snapshot.profile.display_name.is_none(),
        "no kind:0 was ingested — display_name must be None (honest placeholder)"
    );

    tx.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown))
        .ok();
}
