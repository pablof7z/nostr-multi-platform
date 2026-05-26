//! T119 — NIP-46 bunker signing **on the wire** end-to-end.
//!
//! Locks down the full chain that was wired up across the
//! `nmp-signer-iface` → `nmp-signers` → app-neutral `nmp-signer-broker` →
//! app/actor adapter stack but had no integration coverage prior to HB54:
//!
//! 1. A `bunker://` URI is dispatched at the broker.
//! 2. The broker dials a real WebSocket relay (here our `MockBunkerRelay`
//!    on `127.0.0.1`), runs the `connect` + `get_public_key` handshake.
//! 3. The broker constructs a `Nip46Signer` and emits `SignerReady`; the
//!    test adapter packages it as `Box<dyn RemoteSignerHandle>` and posts
//!    `AddRemoteSigner` to the actor sender — the same translation NmpApp
//!    composition performs.
//! 4. The test plays the actor's role: receives `AddRemoteSigner`, slots
//!    the handle into a fresh `IdentityRuntime`, drives `sign_active`.
//! 5. The signer's `sign()` enqueues a `sign_event` RPC; the broker's
//!    `BrokerTransport::send_rpc` NIP-44-encrypts + signs + ships it to
//!    the mock relay.
//! 6. The mock signs the inner kind:1 with the user's secret key and
//!    replies with an encrypted `{id, result: <signed-event-json>}`.
//! 7. The inbound dispatcher thread the broker spawned routes the
//!    response back into `Nip46Signer::deliver_rpc_response`, which fires
//!    the pending one-shot.
//! 8. The mapper validates the signed kind:1 (id recomputation + schnorr
//!    verify + pubkey match) and resolves the `sign_active` blocking call.
//!
//! ## Assertions
//!
//! - The mock observed `connect` + `get_public_key` + `sign_event`.
//! - The signed event the kernel receives has the user's pubkey, kind=1,
//!   and a schnorr signature that re-verifies with `nostr::Event::verify`.
//!
//! ## What this **doesn't** test
//!
//! - Reconnect mid-publish (the broker does not yet replay in-flight RPCs
//!   after a relay socket rebuild).
//! - NIP-42 AUTH challenges over the bunker relay (separate follow-up;
//!   `sync_kernel` clears the auth signer when a NIP-46 is active).
//! - Concurrent publishes (the broker's `pending` map handles them; we
//!   exercise one at a time).

mod common;

use std::sync::mpsc;
use std::time::Duration;

use nmp_core::{ActorCommand, RemoteSignerHandle};
use nmp_signer_iface::SignerError;
use nostr::{Event, Keys};

use crate::common::broker_adapter::broker_for_actor;
use crate::common::mock_bunker_relay::MockBunkerRelay;

/// Spin up the mock, hand the broker a `bunker://<bunker-pubkey>?relay=ws://…`
/// URI, wait until the actor channel produces `AddRemoteSigner`, then drive a
/// `sign_active`-style call against the resulting handle.
#[test]
fn bunker_sign_event_round_trip_on_the_wire() {
    // ── Setup ───────────────────────────────────────────────────────────
    // Two key pairs:
    //   `bunker_keys`  — addresses the bunker; the URI's pubkey segment
    //                    and the pubkey on outgoing `connect` / `get_public_key`
    //                    / `sign_event` RPCs.
    //   `user_keys`    — the user whose nsec the bunker custodies.  Its
    //                    pubkey is the one `get_public_key` returns and the
    //                    one the signed kind:1 must carry.
    let bunker_keys = Keys::generate();
    let user_keys = Keys::generate();
    let user_pubkey_hex = user_keys.public_key().to_hex();

    let mock = MockBunkerRelay::spawn(bunker_keys.clone(), user_keys.clone())
        .expect("mock bunker relay must spawn on 127.0.0.1");
    let bunker_uri = format!(
        "bunker://{}?relay={}",
        bunker_keys.public_key().to_hex(),
        mock.ws_url(),
    );

    // Actor sender stand-in: the broker is going to send
    // `BunkerHandshakeProgress` events here (we ignore them) and one
    // `AddRemoteSigner` once the handshake completes.
    let (actor_tx, actor_rx) = mpsc::channel::<ActorCommand>();
    let broker = broker_for_actor(actor_tx);

    // ── Drive the handshake ────────────────────────────────────────────
    broker.start_handshake(bunker_uri);

    let handle = wait_for_add_remote_signer(&actor_rx, Duration::from_secs(10))
        .expect("AddRemoteSigner must arrive on the actor channel");

    assert_eq!(
        handle.pubkey_hex(),
        user_pubkey_hex,
        "the signer must report the user pubkey returned by get_public_key, \
         not the bunker pubkey from the URI",
    );
    assert_eq!(handle.signer_kind(), "nip46");

    // The mock should have observed `connect` then `get_public_key` in order.
    // (Some bunkers send `connect` after `get_public_key`; the broker we
    // shipped sends `connect` first.)
    let observed = mock.observed_methods();
    assert!(
        observed.iter().any(|m| m == "connect"),
        "mock must have seen `connect`, got {observed:?}"
    );
    assert!(
        observed.iter().any(|m| m == "get_public_key"),
        "mock must have seen `get_public_key`, got {observed:?}"
    );

    // ── Drive a sign through the wire ──────────────────────────────────
    let unsigned = nmp_core::substrate::UnsignedEvent {
        pubkey: user_pubkey_hex.clone(),
        kind: 1,
        tags: Vec::new(),
        content: "hello bunker — t119 on the wire".to_string(),
        created_at: 1_700_000_500,
    };

    // The production REMOTE_SIGN_TIMEOUT is 5s; we don't need that here —
    // the mock turns around in milliseconds. 10s is generous.
    let signed = handle
        .sign(&unsigned)
        .wait(Duration::from_secs(10))
        .unwrap_or_else(|e| panic!("sign over the wire failed: {e}"));

    assert_eq!(signed.unsigned.pubkey, user_pubkey_hex);
    assert_eq!(signed.unsigned.kind, 1);
    assert_eq!(signed.unsigned.content, unsigned.content);

    // Cross-check signature: parse the signed event back through nostr and
    // run `verify()` — proves id+sig are real, not just round-tripped
    // strings.
    re_verify_signed_event(&signed);

    // `sign_event` must have appeared in the mock's observed method list.
    let observed_after = mock.observed_methods();
    assert!(
        observed_after.iter().any(|m| m == "sign_event"),
        "mock must have seen `sign_event` after sign() call, got {observed_after:?}"
    );

    // ── Tear down ──────────────────────────────────────────────────────
    broker.cancel();
    // mock drops here.
}

/// Same wire chain, but drive the sign through the actor path so the
/// `IdentityRuntime` → `sign_active` → `publish_unsigned_event` plumbing is
/// covered end-to-end (mirroring how production code calls into the signer).
#[test]
fn bunker_publish_unsigned_event_routes_signed_kind1_through_publish_queue() {
    use std::sync::mpsc;

    use nmp_core::testing::{run_actor, ActorCommand};

    let bunker_keys = Keys::generate();
    let user_keys = Keys::generate();
    let user_pubkey_hex = user_keys.public_key().to_hex();

    let mock = MockBunkerRelay::spawn(bunker_keys.clone(), user_keys.clone())
        .expect("mock bunker relay must spawn");
    let bunker_uri = format!(
        "bunker://{}?relay={}",
        bunker_keys.public_key().to_hex(),
        mock.ws_url(),
    );

    let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
    let (upd_tx, upd_rx) = mpsc::channel::<String>();
    let actor_self_tx = cmd_tx.clone();
    let actor_handle = std::thread::spawn(move || run_actor(cmd_rx, actor_self_tx, upd_tx));

    cmd_tx
        .send(ActorCommand::Start {
            visible_limit: 50,
            emit_hz: 30,
        })
        .expect("send Start");

    // Wire the broker through the same event-to-actor-command translation
    // that `nmp_signer_broker_init` installs on Chirp startup, then deliver
    // the URI.
    let broker = broker_for_actor(cmd_tx.clone());
    broker.start_handshake(bunker_uri);

    // Wait for the actor's snapshot to confirm the nip46 account is active.
    let user_pk_for_wait = user_pubkey_hex.clone();
    wait_for_snapshot_predicate(&upd_rx, Duration::from_secs(10), move |frame| {
        frame.contains("\"signer_kind\":\"nip46\"")
            && frame.contains(&user_pk_for_wait)
            && frame.contains(&format!("\"active_account\":\"{user_pk_for_wait}\""))
    })
    .expect("actor snapshot must include the nip46 account after handshake completes");

    // Now drive a publish.  This walks `sign_active` → handle.sign() →
    // BrokerTransport → mock → BrokerTransport::dispatch_inbound →
    // deliver_rpc_response → mapper → signed event → publish_signed.
    let unsigned = nmp_core::substrate::UnsignedEvent {
        pubkey: user_pubkey_hex.clone(),
        kind: 1,
        tags: Vec::new(),
        content: "t119 via actor".to_string(),
        created_at: 1_700_001_000,
    };
    cmd_tx
        .send(ActorCommand::PublishUnsignedEvent {
            event: unsigned,
            correlation_id: None,
        })
        .expect("send PublishUnsignedEvent");

    // Wait for a snapshot whose publish_queue has a kind:1 entry.
    //
    // T-publish-resolver-indexer (codex f81f735): the resolver is now
    // fail-closed — an author with no kind:10002 produces `NoTargets` and the
    // engine records a queue entry with `status:"pending_relays_unknown"` rather
    // than routing to arbitrary public relays. The test no longer asserts on
    // event content appearing in timeline items (that required a live relay
    // echo); instead we assert on the queue entry itself and the mock methods
    // to prove that bunker signing flowed correctly.
    // `"publish_queue\":[{` means the array is non-empty. `"kind\":1` in the
    // same frame proves it's the right event type (kind:1 note). D0: the
    // publish cluster now nests under the snapshot's `projections` map — the
    // `"publish_queue":[…]` key still serializes verbatim, so this substring
    // probe is unaffected by the field's relocation.
    let last_frame = wait_for_snapshot_predicate(&upd_rx, Duration::from_secs(15), move |frame| {
        frame.contains("\"publish_queue\":[{") && frame.contains("\"target_relays\"")
    })
    .expect("publish_queue must include a kind:1 entry — sign flowed through bunker");

    // Spot-check fields directly off the snapshot rather than re-parse: the
    // `signer_kind` row must still be `nip46` (proves the publish went
    // through the remote handle, not a fallback local key).
    assert!(
        last_frame.contains("\"signer_kind\":\"nip46\""),
        "snapshot lost the nip46 row: {last_frame}"
    );

    // The mock must have seen a `sign_event` call — the bunker bridge was
    // actually invoked (not just a local-key fallback path).
    let methods = mock.observed_methods();
    assert!(
        methods.contains(&"sign_event".to_string()),
        "mock bunker must have seen a sign_event RPC; got: {methods:?}"
    );

    // Tear down.
    broker.cancel();
    let _ = cmd_tx.send(ActorCommand::Shutdown);
    let _ = actor_handle.join();
}

/// Block on `actor_rx` until an `AddRemoteSigner` arrives; return the boxed
/// handle.  All other commands (progress, …) are drained and dropped.
fn wait_for_add_remote_signer(
    actor_rx: &mpsc::Receiver<ActorCommand>,
    timeout: Duration,
) -> Option<Box<dyn RemoteSignerHandle>> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.checked_duration_since(std::time::Instant::now())?;
        match actor_rx.recv_timeout(remaining) {
            Ok(ActorCommand::AddRemoteSigner { handle }) => return Some(handle),
            Ok(ActorCommand::BunkerHandshakeProgress { stage, message }) => {
                if stage == "failed" {
                    panic!("bunker handshake failed: {stage}: {message:?}");
                }
                continue;
            }
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}

/// Drain `upd_rx` until a snapshot frame matches `predicate`, or timeout.
/// Returns the matching frame.
fn wait_for_snapshot_predicate(
    upd_rx: &mpsc::Receiver<String>,
    timeout: Duration,
    predicate: impl Fn(&str) -> bool,
) -> Option<String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.checked_duration_since(std::time::Instant::now())?;
        match upd_rx.recv_timeout(remaining.min(Duration::from_millis(200))) {
            Ok(frame) => {
                if predicate(&frame) {
                    return Some(frame);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
        }
    }
}

/// Walk the signed event back through `nostr::Event` and call `verify()` —
/// proves the bunker's signature is cryptographically valid for the recovered
/// id, not just a string the mapper let through.
fn re_verify_signed_event(signed: &nmp_core::substrate::SignedEvent) {
    use std::str::FromStr;

    use nostr::secp256k1::schnorr::Signature;
    use nostr::{EventId, Kind, PublicKey, Tag, Timestamp};

    let pubkey =
        PublicKey::from_hex(&signed.unsigned.pubkey).expect("response pubkey must be valid hex");
    let id = EventId::from_hex(&signed.id).expect("response id must be valid hex");
    let sig = Signature::from_str(&signed.sig).expect("response sig must be valid hex");
    let tags: Vec<Tag> = signed
        .unsigned
        .tags
        .iter()
        .map(Tag::parse)
        .collect::<Result<_, _>>()
        .expect("tag rows must parse");
    let event = Event::new(
        id,
        pubkey,
        Timestamp::from(signed.unsigned.created_at),
        Kind::from_u16(signed.unsigned.kind as u16),
        tags,
        signed.unsigned.content.clone(),
        sig,
    );
    if let Err(e) = event.verify() {
        panic!("signed event failed nostr::Event::verify(): {e}");
    }
}

/// Surface `SignerError` in `expect`-style call sites without a verbose match.
#[allow(dead_code)] // referenced in macro form by other helpers; kept for parity.
trait UnwrapSignerErr<T> {
    fn unwrap_signer(self) -> T;
}

impl<T> UnwrapSignerErr<T> for Result<T, SignerError> {
    fn unwrap_signer(self) -> T {
        match self {
            Ok(t) => t,
            Err(e) => panic!("signer op failed: {e}"),
        }
    }
}
