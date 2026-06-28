//! PR-B (#991/#979) typed-decode round-trip proof for
//! [`feature_snapshot_from_flatbuffer`].
//!
//! Drives a REAL kernel actor (`nmp_core::testing::spawn_actor` — the same
//! actor loop production uses), signs in a local key, and decodes the emitted
//! FlatBuffers frames through the production typed-first path. Field-level
//! assertions pin the decoded [`FeatureSnapshot`] against the known signing
//! key — proving the kernel's typed-sidecar encoders and this shell's
//! decoders agree on a frame built by the real encoder
//! (`encode_snapshot_with_envelope`), with no JSON payload on the wire.

use super::feature_snapshot_from_flatbuffer;
use nmp_core::actor::{IdentityCommand, LifecycleCommand};
use nmp_core::testing::{spawn_actor, ActorCommand};
use std::time::{Duration, Instant};

/// A fixed nsec used only in tests (same key as nmp-testing's e2e pipeline).
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
/// The hex pubkey of `TEST_NSEC`.
const TEST_PUBKEY_HEX: &str = "7e7e9c42a91bfef19fa929e5fda1b72e0ebc1a4c1141673e2794234d86addf4e";
/// The npub of `TEST_NSEC`.
const TEST_NPUB: &str = "npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg";

#[test]
fn feature_snapshot_round_trips_real_kernel_frame() {
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
                let decoded = feature_snapshot_from_flatbuffer(&frame);
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
         feature_snapshot_from_flatbuffer within 5 s",
    );

    // --- identity cluster: accounts + active_account (typed sidecars) ---
    assert_eq!(
        snapshot.active_account, TEST_PUBKEY_HEX,
        "active_account must round-trip the signed-in pubkey"
    );
    let account = snapshot
        .accounts
        .iter()
        .find(|a| a.id == TEST_PUBKEY_HEX)
        .expect("the signed-in account must appear in the accounts sidecar");
    assert_eq!(account.npub, TEST_NPUB, "npub must round-trip");
    assert!(
        account.active,
        "the signed-in account must be flagged active"
    );
    assert!(
        !account.signer.is_empty(),
        "the signer label/kind must round-trip non-empty"
    );

    // --- configured_relays (typed sidecar) ---
    let relay = snapshot
        .configured_relays
        .iter()
        .find(|r| r.url == "wss://relay.test")
        .expect("the Start-configured relay must appear in configured_relays");
    assert_eq!(relay.role, "both", "relay role must round-trip");

    // --- settings_hub (typed sidecar, always emitted) ---
    assert_eq!(snapshot.settings_hub.title, "Settings");
    assert!(
        !snapshot.settings_hub.subtitle.is_empty(),
        "settings_hub subtitle must carry the relay-count summary"
    );

    tx.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown))
        .ok();
}
