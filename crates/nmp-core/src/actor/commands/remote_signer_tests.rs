//! Stage 3 of NIP-46 wiring: actor-side `RemoteSignerHandle` plumbing.
//!
//! These tests drive the new command handlers + dispatch arms with a stub
//! `RemoteSignerHandle` impl — Stage 4 (broker) ships real NIP-46 transport,
//! but the actor MUST treat the trait as a first-class signer regardless of
//! the impl behind it. D0 stays clean: the stub lives in `nmp-core`'s test
//! tree, NOT in `nmp-signers`.

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use nmp_signer_iface::SignerOp;
use nostr::nips::nip19::FromBech32;
use nostr::{EventBuilder, Keys, SecretKey, Timestamp};

use super::*;
use crate::actor::commands::identity::{sign_active, IdentityRuntime};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::remote_signer::RemoteSignerHandle;
use crate::substrate::{SignedEvent, UnsignedEvent};

/// nsec from `commands::tests` — known-good test key.
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// Stub `RemoteSignerHandle` for Stage 3 plumbing tests. Holds a `Keys` and
/// signs synchronously via `SignerOp::ok(...)`. Production NIP-46 signers
/// live in `nmp-signers`; D0 still forbids that import here so we cannot
/// reach for the real impl — a stub is the correct shape for actor-side
/// plumbing tests.
#[derive(Debug)]
struct StubRemoteSigner {
    keys: Keys,
    pk: String,
    sign_count: Arc<AtomicU32>,
}

impl StubRemoteSigner {
    fn new(keys: Keys) -> Self {
        let pk = keys.public_key().to_hex();
        Self {
            keys,
            pk,
            sign_count: Arc::new(AtomicU32::new(0)),
        }
    }

    fn sign_count_handle(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.sign_count)
    }
}

impl RemoteSignerHandle for StubRemoteSigner {
    fn pubkey_hex(&self) -> String {
        self.pk.clone()
    }

    fn signer_kind(&self) -> &'static str {
        "nip46"
    }

    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        self.sign_count.fetch_add(1, Ordering::Relaxed);
        let kind = nostr::Kind::from_u16(unsigned.kind as u16);
        let tags = unsigned
            .tags
            .iter()
            .filter_map(|t| nostr::Tag::parse(t).ok())
            .collect::<Vec<_>>();
        let built = EventBuilder::new(kind, &unsigned.content)
            .tags(tags)
            .custom_created_at(Timestamp::from(unsigned.created_at))
            .sign_with_keys(&self.keys);
        match built {
            Ok(event) => SignerOp::ok(SignedEvent {
                id: event.id.to_hex(),
                sig: event.sig.to_string(),
                unsigned: UnsignedEvent {
                    pubkey: event.pubkey.to_hex(),
                    kind: event.kind.as_u16() as u32,
                    tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                    content: event.content.clone(),
                    created_at: event.created_at.as_secs(),
                },
            }),
            Err(e) => SignerOp::err(nmp_signer_iface::SignerError::Backend(format!(
                "stub sign failed: {e}"
            ))),
        }
    }

    fn deliver_rpc_response(&self, _response_json: &str) {
        // Stub: no-op. NIP-46 inbound routing is the broker's job (Stage 4).
    }
}

fn fresh() -> (IdentityRuntime, Kernel) {
    (IdentityRuntime::new(), Kernel::new(DEFAULT_VISIBLE_LIMIT))
}

fn stub_signer() -> (Box<StubRemoteSigner>, Arc<AtomicU32>) {
    let sk = SecretKey::from_bech32(TEST_NSEC).expect("valid nsec");
    let keys = Keys::new(sk);
    let stub = StubRemoteSigner::new(keys);
    let count = stub.sign_count_handle();
    (Box::new(stub), count)
}

// ──────────────────────────────────────────────────────────────────────────
// Command-handler tests (the dispatch arms forward straight into these).
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn add_remote_signer_projects_nip46_account_summary() {
    let (mut id, mut kernel) = fresh();
    let (handle, _count) = stub_signer();
    let expected_pk = handle.pubkey_hex();
    add_remote_signer(&mut id, &mut kernel, handle, false);

    let (accounts, active) = kernel.account_snapshot();
    assert!(
        accounts.iter().any(|a| a.signer_kind == "nip46"),
        "expected a nip46 account row, got {accounts:?}"
    );
    let row = accounts
        .iter()
        .find(|a| a.id == expected_pk)
        .expect("row by pubkey hex");
    assert_eq!(row.signer_kind, "nip46");
    assert_eq!(row.status, "active");
    assert!(row.npub.starts_with("npub1"));
    assert_eq!(active, Some(&expected_pk));
}

#[test]
fn remove_remote_signer_drops_account_from_snapshot() {
    let (mut id, mut kernel) = fresh();
    let (handle, _count) = stub_signer();
    let pk = handle.pubkey_hex();
    add_remote_signer(&mut id, &mut kernel, handle, false);
    assert!(!kernel.account_snapshot().0.is_empty());

    let _ = remove_remote_signer(&mut id, &mut kernel, &pk);
    let (accounts, active) = kernel.account_snapshot();
    assert!(
        accounts.iter().all(|a| a.id != pk),
        "account survived remove_remote_signer: {accounts:?}"
    );
    assert!(active.is_none(), "active should be cleared, got {active:?}");
}

#[test]
fn bunker_handshake_progress_writes_then_clears() {
    let (_id, mut kernel) = fresh();
    bunker_handshake_progress(
        &mut kernel,
        "awaiting_pubkey".to_string(),
        Some("connected, waiting for get_public_key".to_string()),
    );
    let progress = kernel.bunker_handshake_snapshot().expect("set");
    assert_eq!(progress.stage, "awaiting_pubkey");
    assert!(progress.message.is_some());

    // `"idle"` collapses to `None`.
    bunker_handshake_progress(&mut kernel, "idle".to_string(), None);
    assert!(kernel.bunker_handshake_snapshot().is_none());
}

#[test]
fn sign_active_routes_through_remote_signer_when_active() {
    let (mut id, mut kernel) = fresh();
    let (handle, count) = stub_signer();
    let expected_pk = handle.pubkey_hex();
    add_remote_signer(&mut id, &mut kernel, handle, false);
    assert_eq!(count.load(Ordering::Relaxed), 0);

    // Drive a publish through the actor path: it must call `sign_active`,
    // which the stub records via `sign_count`.
    let unsigned = UnsignedEvent {
        pubkey: "ignored-by-signer".into(),
        kind: 1,
        tags: Vec::new(),
        content: "stage-3 hello".into(),
        created_at: 1_700_000_000,
    };
    let signed = sign_active(&id, &unsigned).expect("stub sign ok");
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert_eq!(signed.unsigned.pubkey, expected_pk);
    assert_eq!(signed.unsigned.kind, 1);
    assert_eq!(signed.unsigned.content, "stage-3 hello");
}

#[test]
fn publish_unsigned_event_with_active_remote_uses_stub_signer() {
    // End-to-end: AddRemoteSigner → PublishUnsignedEvent goes through the
    // stub. Mirrors `publish_unsigned_event_signs_and_publishes_arbitrary_kind`
    // from `commands::tests` but with a remote handle behind the active slot.
    //
    // T-publish-resolver-indexer (codex f81f735): seed kind:10002 for the
    // remote signer's pubkey so the resolver has NIP-65 write relays.
    let (mut id, mut kernel) = fresh();
    let (handle, count) = stub_signer();
    let expected_pk = handle.pubkey_hex();
    add_remote_signer(&mut id, &mut kernel, handle, false);
    // Seed kind:10002 so the fail-closed resolver finds write relays.
    kernel.seed_kind10002_for_test(
        &expected_pk,
        &["wss://remote-write-r1.test", "wss://remote-write-r2.test"],
    );

    let unsigned = UnsignedEvent {
        pubkey: "ignored-by-signer".into(),
        kind: 30023,
        tags: vec![vec!["d".into(), "stage-3-article".into()]],
        content: "# hello bunker".into(),
        created_at: 1_700_000_000,
    };
    let outbound = publish_unsigned_event(&id, &mut kernel, unsigned, &mut Vec::new());
    assert_eq!(
        count.load(Ordering::Relaxed),
        1,
        "remote signer was invoked"
    );
    assert!(!outbound.is_empty(), "publish produced outbound frames");
    assert!(outbound[0].text.contains("\"kind\":30023"));
    assert!(outbound[0]
        .text
        .contains(&format!("\"pubkey\":\"{expected_pk}\"")));
    let q = kernel.publish_queue_snapshot();
    assert_eq!(q.last().unwrap().status, "accepted_locally");
}

#[test]
fn snapshot_carries_bunker_handshake_value() {
    let (_id, mut kernel) = fresh();
    bunker_handshake_progress(
        &mut kernel,
        "connecting".to_string(),
        Some("dialing wss://r.example".to_string()),
    );
    let json = kernel.make_update(true);
    assert!(json.contains("\"bunker_handshake\""));
    assert!(json.contains("\"stage\":\"connecting\""));
}

// ──────────────────────────────────────────────────────────────────────────
// End-to-end dispatch test — drives the new `ActorCommand` variants through
// the spawned `run_actor` loop so the dispatch arms are exercised (not just
// the command-handler functions they wrap).
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn dispatch_add_remote_signer_then_progress_surfaces_on_snapshot() {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::actor::{run_actor, ActorCommand};

    let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
    let (upd_tx, upd_rx) = mpsc::channel::<String>();
    thread::spawn(move || run_actor(cmd_rx, upd_tx));

    cmd_tx
        .send(ActorCommand::Start {
            visible_limit: 50,
            emit_hz: 30,
        })
        .unwrap();

    let (handle, _count) = stub_signer();
    let pk = handle.pubkey_hex();
    cmd_tx
        .send(ActorCommand::AddRemoteSigner { handle })
        .unwrap();
    cmd_tx
        .send(ActorCommand::BunkerHandshakeProgress {
            stage: "ready".to_string(),
            message: None,
        })
        .unwrap();

    // Let the actor drain both commands and emit at least one snapshot.
    thread::sleep(Duration::from_millis(300));
    let _ = cmd_tx.send(ActorCommand::Shutdown);

    let mut last_frame = String::new();
    while let Ok(frame) = upd_rx.try_recv() {
        last_frame = frame;
    }
    assert!(!last_frame.is_empty(), "actor produced no snapshot frames");
    assert!(
        last_frame.contains(&pk),
        "snapshot missing remote-signer pubkey: {last_frame}"
    );
    assert!(
        last_frame.contains("\"signer_kind\":\"nip46\""),
        "snapshot missing nip46 signer_kind: {last_frame}"
    );
    assert!(
        last_frame.contains("\"stage\":\"ready\""),
        "snapshot missing handshake stage=ready: {last_frame}"
    );
}

#[test]
fn dispatch_remove_remote_signer_drops_account_via_actor() {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use crate::actor::{run_actor, ActorCommand};

    let (cmd_tx, cmd_rx) = mpsc::channel::<ActorCommand>();
    let (upd_tx, upd_rx) = mpsc::channel::<String>();
    thread::spawn(move || run_actor(cmd_rx, upd_tx));

    cmd_tx
        .send(ActorCommand::Start {
            visible_limit: 50,
            emit_hz: 30,
        })
        .unwrap();
    let (handle, _count) = stub_signer();
    let pk = handle.pubkey_hex();
    cmd_tx
        .send(ActorCommand::AddRemoteSigner { handle })
        .unwrap();
    cmd_tx
        .send(ActorCommand::RemoveRemoteSigner {
            identity_id: pk.clone(),
        })
        .unwrap();

    thread::sleep(Duration::from_millis(300));
    let _ = cmd_tx.send(ActorCommand::Shutdown);

    let mut last_frame = String::new();
    while let Ok(frame) = upd_rx.try_recv() {
        last_frame = frame;
    }
    assert!(!last_frame.is_empty(), "actor produced no snapshot frames");
    // After removal the account row should be gone. The pubkey can survive
    // elsewhere on the snapshot (the AddRemoteSigner path retargets the
    // timeline, which leaves the pubkey in the `selected_author` view); we
    // only care that the `accounts` and `active_account` fields are cleared.
    assert!(
        last_frame.contains("\"accounts\":[]"),
        "accounts must be empty after RemoveRemoteSigner: {last_frame}"
    );
    assert!(
        last_frame.contains("\"active_account\":null"),
        "active_account must be cleared after RemoveRemoteSigner: {last_frame}"
    );
    // And nothing labelled signer_kind=nip46 should survive in the snapshot.
    assert!(
        !last_frame.contains("\"signer_kind\":\"nip46\""),
        "snapshot still has nip46 row after remove: {last_frame}"
    );
    let _ = pk; // tied to test setup; kept for symmetry with the add test.
}
