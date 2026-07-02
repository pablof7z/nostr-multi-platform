//! Test 1 — cold_open_profile_view_full_pipeline
//!
//! Scenario:
//!   1. Boot the kernel actor.
//!   2. Sign in as alice (establishes an active account with a local key signer).
//!   3. Dispatch PublishProfile with display_name = "Alice".
//!      — `publish_profile` signs the kind:0 locally, then
//!        `record_local_publish_intent` routes it (since #1193, ADR-0070 Rev 2
//!        single-mechanism) through `verify_and_persist` + `ingest_profile`
//!        into the canonical `profiles` cache — the EXACT relay-ingest sequence.
//!        The later relay echo dedups to `Duplicate` (no-op).
//!   4. Force a snapshot emit (MarkChangedSinceEmit).
//!   5. Drain the update channel; assert the typed `profile` sidecar's
//!      display_name == "Alice".
//!
//! `profile` (not `claimed_profiles`) is the correct projection key here:
//! the active account's own profile card, present in every snapshot (D1),
//! populated immediately by the store-first local-publish path.

use nmp_core::actor::{IdentityCommand, LifecycleCommand, PublishCommand};

#[test]
fn cold_open_profile_view_full_pipeline() {
    use crate::e2e_profile_actor;
    use nmp_core::decode_snapshot_typed_projections;
    use nmp_core::testing::ActorCommand;
    use nmp_core::typed_projections::{decode_profile, PROFILE_SCHEMA_ID};
    use std::time::Duration;

    const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

    let (tx, rx) = e2e_profile_actor::spawn_actor_with_nip01_profile_cache();
    tx.send(ActorCommand::Lifecycle(LifecycleCommand::Start {
        visible_limit: 100,
        emit_hz: 0,
        // A test relay so the publish engine has a target for PublishProfile.
        // Without at least one configured relay the engine's Auto resolver finds
        // nothing, returns Err, and record_local_publish_intent is never called —
        // leaving the profile projection in the "Waiting for kind:0" placeholder.
        initial_relays: vec![("wss://relay.test".to_string(), "both".to_string())],
    }))
    .expect("send Start");

    tx.send(ActorCommand::Identity(IdentityCommand::AddSigner {
        source: nmp_core::SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        make_active: true,
    }))
    .expect("send AddSigner");

    // Step 2: Publish alice's profile.
    // Actor dispatch: PublishProfile → publish_profile() → sign locally →
    // publish_signed_with_correlation → record_local_publish_intent →
    // verify_and_persist + ingest_profile → profiles[alice_pk] =
    // Profile { display: "Alice", ... }
    let mut fields = serde_json::Map::new();
    fields.insert(
        "display_name".to_string(),
        serde_json::Value::String("Alice".to_string()),
    );
    tx.send(ActorCommand::Publish(PublishCommand::Profile {
        fields,
        correlation_id: None,
    }))
    .expect("send PublishProfile");

    // Step 3: Force emit so we don't wait for the ticker.
    tx.send(ActorCommand::Lifecycle(
        LifecycleCommand::MarkChangedSinceEmit,
    ))
    .expect("send MarkChangedSinceEmit");

    // Drain snapshots until the typed `profile` sidecar carries
    // display_name == "Alice" (PR-B: the JSON payload no longer exists).
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut found = false;
    let mut last_profile = None;
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => {
                let typed =
                    decode_snapshot_typed_projections(&frame).expect("decode frame sidecar");
                let profile = typed
                    .iter()
                    .find(|t| t.key == PROFILE_SCHEMA_ID)
                    .and_then(|t| decode_profile(&t.payload).ok());
                if let Some(profile) = profile {
                    if profile.display_name.as_deref() == Some("Alice") {
                        found = true;
                        break;
                    }
                    last_profile = Some(profile);
                }
            }
            Err(_) => continue,
        }
    }

    assert!(
        found,
        "the typed profile sidecar's display_name must equal 'Alice' after PublishProfile; \
         last profile model: {:?}",
        last_profile
    );

    tx.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown))
        .ok();
}
