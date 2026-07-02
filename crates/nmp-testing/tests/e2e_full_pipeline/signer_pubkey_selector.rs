//! Tests 7 + 8 — `PublishRaw` signer_pubkey selector
//!
//! Pin the v0.2.5 `PublishRaw` `signer_pubkey` selector end-to-end through the
//! actor dispatch arm: `signer_pubkey` names an app-managed signer slot.
//!
//! The kernel snapshot does not surface the *author* of a freshly-signed
//! arbitrary-kind event (the `in_flight` / publish_queue projection carries
//! event_id / kind / content but not the signer pubkey, and no harness here
//! captures the outbound EVENT). So the deterministic, harness-free observable
//! for "which roster slot the selector resolved" is the sign-step outcome,
//! surfaced as `last_error_toast`. Both tests below are written so the assertion
//! FLIPS against the pre-v0.2.5 hardcoded-`None` code — i.e. they discriminate
//! the fix from the bug rather than passing vacuously.

use nmp_core::actor::{IdentityCommand, LifecycleCommand, PublishCommand};

/// Drain snapshot frames up to `deadline_secs`, returning true iff a
/// `last_error_toast` containing `needle` is observed.
fn wait_for_error_toast(
    rx: &std::sync::mpsc::Receiver<Vec<u8>>,
    needle: &str,
    deadline_secs: u64,
) -> bool {
    use nmp_core::decode_snapshot_envelope;
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(deadline_secs);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => {
                // PR-B: `last_error_toast` is a typed Tier-3 envelope field.
                if let Ok(envelope) = decode_snapshot_envelope(&frame) {
                    if let Some(toast) = envelope.last_error_toast {
                        if toast.contains(needle) {
                            return true;
                        }
                    }
                }
            }
            Err(_) => continue,
        }
    }
    false
}

// Test 7 — an UNREGISTERED selector fails closed at the sign step.
//
// Discriminates the `Auto` dispatch branch: alice is the *active* account, so
// the pre-v0.2.5 hardcoded `None` would have ignored the selector and signed
// cleanly with alice — NO toast. Honouring `Some(<unregistered>)` instead routes
// to `sign_with_account_nonblocking`, which returns
// Err("no signer for account {pk} — add it first"), folded into
// `last_error_toast` by `fail_publish`. The presence of that toast is therefore
// a clean fix-vs-bug discriminator.
#[test]
fn publish_raw_signer_pubkey_unregistered_fails_closed() {
    use nmp_core::publish::PublishTarget;
    use nmp_core::testing::{spawn_actor, ActorCommand};

    const ALICE_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    let unregistered_pubkey_hex = "d".repeat(64);

    let (tx, rx) = spawn_actor();
    tx.send(ActorCommand::Lifecycle(LifecycleCommand::Start {
        visible_limit: 100,
        emit_hz: 0,
        // The Auto resolver needs at least one configured relay or it
        // short-circuits to NoTargets before the sign step runs.
        initial_relays: vec![("wss://relay.test".to_string(), "both".to_string())],
    }))
    .expect("send Start");

    // Active account: alice. Under the legacy hardcoded `None`, alice would have
    // signed and no error toast would ever appear.
    tx.send(ActorCommand::Identity(IdentityCommand::AddSigner {
        source: nmp_core::SignerSource::LocalNsec(zeroize::Zeroizing::new(ALICE_NSEC.to_string())),
        make_active: true,
    }))
    .expect("send AddSigner alice");

    tx.send(ActorCommand::Publish(PublishCommand::RawEvent {
        kind: 30023,
        tags: Vec::new(),
        content: "should fail — unknown signer".to_string(),
        target: PublishTarget::Auto,
        signer_pubkey: Some(unregistered_pubkey_hex.clone()),
        correlation_id: None,
    }))
    .expect("send PublishRawEvent (unregistered signer)");
    tx.send(ActorCommand::Lifecycle(
        LifecycleCommand::MarkChangedSinceEmit,
    ))
    .expect("send MarkChangedSinceEmit");

    assert!(
        wait_for_error_toast(&rx, "no signer for account", 5),
        "an unregistered signer_pubkey must surface a 'no signer for account' toast — \
         proving the Auto dispatch branch honoured the selector (the legacy hardcoded \
         None would have signed with the active account and raised no toast)"
    );

    tx.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown))
        .ok();
}

// Test 8 — a REGISTERED agent key signs even with NO active account.
//
// Discriminates the `Explicit` dispatch branch: bob is registered as an
// app-managed signer slot, and there is NO active account. Under the pre-v0.2.5
// hardcoded `None`, `signer_pubkey.is_none() && active.is_none()` is true →
// `toast_no_account` raises "no active account — sign in first". Honouring
// `Some(bob)` instead skips that guard and signs with bob — NO such toast.
// Manual override exercises the explicit dispatch call site.
#[test]
fn publish_raw_signer_pubkey_signs_with_registered_agent_key_without_active_account() {
    use nmp_core::publish::{PublishRouteClass, PublishTarget};
    use nmp_core::testing::{spawn_actor, ActorCommand};
    use nostr::nips::nip19::ToBech32;
    use nostr::Keys;

    // Bob is the agent / per-podcast key: a hidden signer slot, never the active
    // account, and the ONLY signer in the roster (no active account at all).
    let bob_keys = Keys::generate();
    let bob_nsec = bob_keys
        .secret_key()
        .to_bech32()
        .expect("bob secret key must bech32-encode as an nsec");
    let bob_pubkey_hex = bob_keys.public_key().to_hex();

    let (tx, rx) = spawn_actor();
    tx.send(ActorCommand::Lifecycle(LifecycleCommand::Start {
        visible_limit: 100,
        emit_hz: 0,
        initial_relays: Vec::new(),
    }))
    .expect("send Start");

    // Agent key only: bob. Crucially, no active sign-in follows — there is no
    // active account, so the legacy `None` path would fail closed with
    // "no active account".
    tx.send(ActorCommand::Identity(IdentityCommand::AddSigner {
        source: nmp_core::SignerSource::AppManagedLocalNsec(zeroize::Zeroizing::new(bob_nsec)),
        make_active: false,
    }))
    .expect("send AddSigner bob");

    // `Explicit` target exercises the second changed dispatch call site
    // (`publish_unsigned_event_to_relays`). A stub relay URL is a valid target;
    // we only assert on the sign step, not on relay delivery.
    tx.send(ActorCommand::Publish(PublishCommand::RawEvent {
        kind: 30023,
        tags: Vec::new(),
        content: "agent-authored note".to_string(),
        target: PublishTarget::explicit(
            vec!["wss://relay.test".to_string()],
            PublishRouteClass::ManualOverride,
        ),
        signer_pubkey: Some(bob_pubkey_hex.clone()),
        correlation_id: None,
    }))
    .expect("send PublishRawEvent (registered agent signer, no active account)");
    tx.send(ActorCommand::Lifecycle(
        LifecycleCommand::MarkChangedSinceEmit,
    ))
    .expect("send MarkChangedSinceEmit");

    // The selector must let bob sign WITHOUT an active account: the
    // "no active account" guard must NOT fire. (Under the legacy hardcoded
    // `None` this toast WOULD appear — so its absence discriminates the fix.)
    assert!(
        !wait_for_error_toast(&rx, "no active account", 2),
        "a registered app-managed agent key must sign a PublishRaw \
         even with no active account — the 'no active account' guard must not fire when \
         signer_pubkey selects that key"
    );

    tx.send(ActorCommand::Lifecycle(LifecycleCommand::Shutdown))
        .ok();
}
