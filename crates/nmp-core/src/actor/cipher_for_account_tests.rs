//! ADR-0050 §D1 — `Nip44EncryptForAccount` / `Nip44DecryptForAccount` port +
//! §D4 per-account deadline regression tests.
//!
//! Oracle 2 (encrypt+decrypt round-trip through the port): a local account
//! encrypts a plaintext to a peer and the peer decrypts it back — both verbs
//! routed through the actor dispatch arm and the identity-runtime NIP-44
//! helpers (which run `nostr::nips::nip44` INSIDE the runtime, D13). A stub
//! remote signer drives the `Pending` → parked → drained path so the cipher
//! `CipherContinuation` sink is exercised.
//!
//! Oracle 3 (§D4): a named NIP-55-style roster key with a 90s budget parks with
//! ITS budget, not the active (5s NIP-46-style) account's — the bug ADR-0050 D4
//! fixes at `dispatch.rs:606` / `signer_port_dispatch.rs:71`.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmp_signer_iface::{SignerError, SignerOp};
use nostr::nips::nip19::{FromBech32, ToBech32};
use nostr::{Keys, SecretKey};

use super::commands::{self, IdentityRuntime};
use super::dispatch::{dispatch_command, ActorContext};
use super::pending_sign::{resolve_parked_op, ParkedOp, ParkedOpSink};
use super::{ActorCommand, ActorMail, CipherContinuation, CommandSender};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::remote_signer::RemoteSignerHandle;
use crate::substrate::{SignedEvent, UnsignedEvent};

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

fn fresh_identity() -> IdentityRuntime {
    IdentityRuntime::new(
        commands::new_bunker_handshake_slot(),
        commands::new_signer_state_slot(),
    )
}

/// Captured cipher outcome: `Some(Ok(text))` / `Some(Err(reason))` once the
/// continuation ran, `None` while it has not.
type CapturedCipher = Arc<Mutex<Option<Result<String, String>>>>;

fn capture_cipher() -> (CapturedCipher, CipherContinuation) {
    let captured: CapturedCipher = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&captured);
    let continuation = CipherContinuation::new(move |outcome| {
        *slot.lock().unwrap() = Some(outcome);
    });
    (captured, continuation)
}

/// A remote-signer stub whose NIP-44 verbs return `SignerOp::Pending` — the
/// "broker" round-trip is driven by the test through the stashed sender. Sign
/// is unused here. `op_timeout` is configurable so the §D4 budget test can give
/// it a 90s deadline like a NIP-55 signer.
#[derive(Debug)]
struct PendingCipherSigner {
    pk: String,
    op_timeout: Duration,
    last_sender: Mutex<Option<Sender<Result<String, SignerError>>>>,
}

impl PendingCipherSigner {
    fn new(pk: String, op_timeout: Duration) -> Self {
        Self {
            pk,
            op_timeout,
            last_sender: Mutex::new(None),
        }
    }

    fn pending_string_op(&self) -> SignerOp<String> {
        let (tx, rx): (
            Sender<Result<String, SignerError>>,
            Receiver<Result<String, SignerError>>,
        ) = channel();
        *self.last_sender.lock().unwrap() = Some(tx);
        SignerOp::Pending(rx)
    }
}

impl RemoteSignerHandle for PendingCipherSigner {
    fn pubkey_hex(&self) -> String {
        self.pk.clone()
    }
    fn signer_kind(&self) -> &'static str {
        "nip46"
    }
    fn op_timeout(&self) -> Duration {
        self.op_timeout
    }
    fn sign(&self, _unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        SignerOp::err(SignerError::Backend("unused".into()))
    }
    fn nip44_encrypt(&self, _recipient_pubkey: &str, _plaintext: &str) -> SignerOp<String> {
        self.pending_string_op()
    }
    fn nip44_decrypt(&self, _sender_pubkey: &str, _ciphertext: &str) -> SignerOp<String> {
        self.pending_string_op()
    }
    fn deliver_response(&self, _response_json: &str) {}
}

/// A remote-signer stub that records every `deliver_response` it receives — the
/// §D3b fan-out target. `op_timeout` and the cipher/sign verbs are unused here.
#[derive(Debug)]
struct RecordingSigner {
    pk: String,
    delivered: Arc<Mutex<Vec<String>>>,
}

impl RemoteSignerHandle for RecordingSigner {
    fn pubkey_hex(&self) -> String {
        self.pk.clone()
    }
    fn signer_kind(&self) -> &'static str {
        "nip46"
    }
    fn sign(&self, _unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        SignerOp::err(SignerError::Backend("unused".into()))
    }
    fn nip44_encrypt(&self, _r: &str, _p: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Backend("unused".into()))
    }
    fn nip44_decrypt(&self, _s: &str, _c: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Backend("unused".into()))
    }
    fn deliver_response(&self, response_json: &str) {
        self.delivered.lock().unwrap().push(response_json.to_string());
    }
}

/// Drive a single `dispatch_command(cmd, ctx)` and return the parked-op queue.
fn dispatch_one(
    cmd: ActorCommand,
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
) -> Vec<ParkedOp> {
    use crate::relay::CanonicalRelayUrl;
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;

    let (update_tx, _update_rx) = channel::<crate::update_envelope::UpdateFrameBytes>();
    let (command_inbox_tx, _command_rx) = channel::<ActorMail>();
    let command_tx = CommandSender::new(command_inbox_tx);
    let lifecycle_observer = commands::new_observer_slot();
    let mls_local_nsec = Arc::new(Mutex::new(None));
    let active_local_keys = Arc::new(Mutex::new(None));
    let pool = nmp_network::pool::Pool::new(
        nmp_network::pool::PoolConfig::default(),
        channel::<nmp_network::pool::PoolEvent>().0,
    );
    let mut relay_controls: HashMap<CanonicalRelayUrl, super::RelayControl> = HashMap::new();
    let mut slot_to_url: HashMap<u32, CanonicalRelayUrl> = HashMap::new();
    let mut connected_relays = HashSet::new();
    let mut connected_urls = HashSet::new();
    let mut last_emit = Instant::now();
    let mut next_relay_generation = 1u64;
    let mut running = true;
    let mut emit_hz = 4u32;
    let mut startup_sent = false;
    let mut parked_ops: Vec<ParkedOp> = Vec::new();
    let capability_callback: crate::capability_socket::CapabilityCallbackSlot =
        Arc::new(Mutex::new(None));
    let (capability_work_inner_tx, _capability_work_rx) = channel::<ActorMail>();
    let capability_work_tx = crate::actor::capability_worker::spawn_capability_worker(
        Arc::clone(&capability_callback),
        CommandSender::new(capability_work_inner_tx),
    );
    let coverage_hook = Arc::new(Mutex::new(None::<crate::subs::PlanCoverageHook>));
    let req_frame_interceptor = Arc::new(Mutex::new(None));
    let host_op_handler = Arc::new(Mutex::new(None));
    let ingest_dispatcher_slot = Arc::new(std::sync::RwLock::new(
        crate::substrate::EventIngestDispatcher::default(),
    ));
    let dm_inbox_relays_slot =
        Arc::new(Mutex::new(crate::substrate::empty_dm_inbox_relay_lookup()));
    let blocked_relays_slot = Arc::new(Mutex::new(crate::substrate::empty_blocked_relay_lookup()));
    let bootstrap_self_kinds_slot = Arc::new(Mutex::new(None));
    let routing_trace_slot = Arc::new(Mutex::new(None));
    let event_store_slot = Arc::new(Mutex::new(None));
    let routing_substrate_slot = Arc::new(Mutex::new(None));
    let publish_resolver_slot = Arc::new(Mutex::new(None));
    let active_account_slot = Arc::new(Mutex::new(None));
    let raw_event_forward_observer_ids =
        crate::actor::raw_event_forwarder::new_raw_event_forward_observer_id_slot();
    let raw_event_forward_policy_slot = Arc::new(Mutex::new(None));
    let raw_event_observers = commands::new_raw_event_observer_slot();

    let mut ctx = ActorContext {
        kernel,
        identity,
        relay_controls: &mut relay_controls,
        slot_to_url: &mut slot_to_url,
        pool: &pool,
        connected_relays: &mut connected_relays,
        connected_urls: &mut connected_urls,
        update_tx: &update_tx,
        last_emit: &mut last_emit,
        next_relay_generation: &mut next_relay_generation,
        running: &mut running,
        emit_hz: &mut emit_hz,
        startup_sent: &mut startup_sent,
        relays_ready: false,
        lifecycle_observer: &lifecycle_observer,
        mls_local_nsec: &mls_local_nsec,
        active_local_keys: &active_local_keys,
        capability_callback: &capability_callback,
        parked_ops: &mut parked_ops,
        command_tx_self: &command_tx,
        capability_work_tx: &capability_work_tx,
        coverage_hook_slot: &coverage_hook,
        req_frame_interceptor_slot: &req_frame_interceptor,
        host_op_handler: &host_op_handler,
        ingest_dispatcher_slot: &ingest_dispatcher_slot,
        dm_inbox_relays_slot: &dm_inbox_relays_slot,
        blocked_relays_slot: &blocked_relays_slot,
        bootstrap_self_kinds_slot: &bootstrap_self_kinds_slot,
        routing_trace_slot: &routing_trace_slot,
        event_store_slot: &event_store_slot,
        routing_substrate_slot: &routing_substrate_slot,
        publish_resolver_slot: &publish_resolver_slot,
        active_account_slot: &active_account_slot,
        raw_event_forward_observer_ids: &raw_event_forward_observer_ids,
        raw_event_forward_policy_slot: &raw_event_forward_policy_slot,
        raw_event_observers_handle: &raw_event_observers,
    };
    dispatch_command(cmd, &mut ctx);
    parked_ops
}

// ── Oracle 2 — encrypt+decrypt round-trip through the port (local account) ──

/// A local account encrypts a plaintext to a peer through
/// `Nip44EncryptForAccount`; the continuation resolves INLINE with ciphertext
/// (local op is `Ready`). The peer then decrypts that ciphertext through
/// `Nip44DecryptForAccount` and recovers the original plaintext. Both verbs run
/// `nostr::nips::nip44` inside the identity runtime (D13).
#[test]
fn local_account_nip44_encrypt_decrypt_round_trips_through_the_port() {
    // Alice is the active local account; Bob is the peer.
    let alice = Keys::new(SecretKey::from_bech32(TEST_NSEC).expect("valid nsec"));
    let bob = Keys::generate();
    let alice_pk = alice.public_key().to_hex();
    let bob_pk = bob.public_key().to_hex();
    let alice_nsec = alice.secret_key().to_bech32().expect("nsec");
    let bob_nsec = bob.secret_key().to_bech32().expect("nsec");

    let mut id_alice = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    commands::add_signer(
        &mut id_alice,
        &mut kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(alice_nsec)),
        true,
        false,
    );

    // Alice encrypts "hello bob" to Bob through the port.
    let (enc_captured, enc_cont) = capture_cipher();
    let parked = dispatch_one(
        ActorCommand::Nip44EncryptForAccount {
            peer_pubkey: bob_pk.clone(),
            plaintext: "hello bob".to_string(),
            signer_pubkey: None,
            continuation: enc_cont,
        },
        &mut id_alice,
        &mut kernel,
    );
    assert!(parked.is_empty(), "a local encrypt resolves Ready — no park");
    let ciphertext = enc_captured
        .lock()
        .unwrap()
        .take()
        .expect("encrypt continuation ran inline")
        .expect("local encrypt succeeds");
    assert!(!ciphertext.is_empty(), "ciphertext is produced");
    assert_ne!(ciphertext, "hello bob", "ciphertext is not the plaintext");

    // Bob (active local) decrypts the ciphertext FROM Alice through the port.
    let mut id_bob = fresh_identity();
    commands::add_signer(
        &mut id_bob,
        &mut kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(bob_nsec)),
        true,
        false,
    );
    let (dec_captured, dec_cont) = capture_cipher();
    let parked = dispatch_one(
        ActorCommand::Nip44DecryptForAccount {
            peer_pubkey: alice_pk,
            ciphertext,
            signer_pubkey: None,
            continuation: dec_cont,
        },
        &mut id_bob,
        &mut kernel,
    );
    assert!(parked.is_empty(), "a local decrypt resolves Ready — no park");
    let plaintext = dec_captured
        .lock()
        .unwrap()
        .take()
        .expect("decrypt continuation ran inline")
        .expect("local decrypt succeeds");
    assert_eq!(plaintext, "hello bob", "round-trip recovers the plaintext");
}

/// A remote (bunker) account encrypt parks under the `CipherContinuation` sink;
/// the drain invokes the SAME continuation once the broker turns the request
/// around. Proves the §D1 cipher park/drain path end-to-end.
#[test]
fn bunker_account_nip44_encrypt_parks_then_drain_invokes_continuation() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let stub_pk = Keys::generate().public_key().to_hex();
    let stub = PendingCipherSigner::new(stub_pk.clone(), Duration::from_secs(5));
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(stub)),
        true,
        false,
    );

    let (captured, continuation) = capture_cipher();
    let mut parked = dispatch_one(
        ActorCommand::Nip44EncryptForAccount {
            peer_pubkey: Keys::generate().public_key().to_hex(),
            plaintext: "secret".to_string(),
            signer_pubkey: None,
            continuation,
        },
        &mut identity,
        &mut kernel,
    );
    assert_eq!(parked.len(), 1, "a bunker encrypt parks a Pending op");
    assert!(
        captured.lock().unwrap().is_none(),
        "continuation must NOT run while the broker round-trip is pending"
    );

    // The broker turns the request around: replace the parked op with one whose
    // sender we control, then resolve it (mirrors a later idle-tick delivery).
    {
        let ParkedOpSink::CipherContinuation { op, .. } = &mut parked[0].sink else {
            panic!("expected a CipherContinuation sink");
        };
        let (tx, rx): (
            Sender<Result<String, SignerError>>,
            Receiver<Result<String, SignerError>>,
        ) = channel();
        *op = SignerOp::Pending(rx);
        tx.send(Ok("ciphertext-from-bunker".to_string())).unwrap();
    }
    let drained = resolve_parked_op(&mut parked[0], &mut kernel);
    assert!(!drained.keep, "a resolved cipher op is dropped");
    assert!(
        drained.publish.is_none(),
        "a CipherContinuation sink yields no publish obligation"
    );
    let got = captured
        .lock()
        .unwrap()
        .take()
        .expect("continuation runs from the drain once the broker responds");
    assert_eq!(got.expect("bunker encrypt succeeds"), "ciphertext-from-bunker");
}

// ── Oracle 3 — §D4 named-account budget regression ──────────────────────────

/// A named roster key with a 90s budget must park with ITS deadline, not the
/// active account's (5s). Regression for ADR-0050 D4 (`signer_port_dispatch.rs:71`,
/// which previously called `active_sign_deadline()`).
#[test]
fn named_roster_key_keeps_its_own_budget_not_the_active_accounts() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Active account: a 5s-budget bunker (NIP-46-style).
    let active = PendingCipherSigner::new(
        Keys::generate().public_key().to_hex(),
        Duration::from_secs(5),
    );
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(active)),
        true,
        false,
    );

    // A SECOND, non-active roster key: a 90s-budget signer (NIP-55-style).
    let named_pk = Keys::generate().public_key().to_hex();
    let named = PendingCipherSigner::new(named_pk.clone(), Duration::from_secs(90));
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(named)),
        false, // do NOT make active — the 5s account stays active.
        false,
    );

    // Encrypt with the NAMED key while the 5s account is active.
    let (_captured, continuation) = capture_cipher();
    let before = std::time::Instant::now();
    let parked = dispatch_one(
        ActorCommand::Nip44EncryptForAccount {
            peer_pubkey: Keys::generate().public_key().to_hex(),
            plaintext: "x".to_string(),
            signer_pubkey: Some(named_pk),
            continuation,
        },
        &mut identity,
        &mut kernel,
    );
    assert_eq!(parked.len(), 1, "named bunker encrypt parks");

    // The parked deadline must reflect the NAMED key's 90s budget, NOT the
    // active account's 5s. Allow a generous slack for dispatch latency.
    let deadline = parked[0].deadline;
    let budget = deadline.saturating_duration_since(before);
    assert!(
        budget > Duration::from_secs(60),
        "named 90s key must keep its own budget (got {budget:?}); the D4 bug would \
         have parked it with the active account's 5s budget"
    );
}

// ── §D3b — DeliverSignerResponse fans out to remote handles ─────────────────

/// The `DeliverSignerResponse` dispatch arm fans the response JSON out to every
/// registered remote handle (the §D3b mailbox-completion path). This is what
/// resolves a parked op on the actor thread: the command is delivered by the
/// single waking inbox (§D3a — proven separately by `inbox::tests`), and the
/// parked-op drain runs unconditionally the same loop iteration. Here we assert
/// the fan-out reaches the handle with the exact body.
#[test]
fn deliver_signer_response_fans_out_to_every_remote_handle() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let delivered_a = Arc::new(Mutex::new(Vec::new()));
    let delivered_b = Arc::new(Mutex::new(Vec::new()));
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(RecordingSigner {
            pk: Keys::generate().public_key().to_hex(),
            delivered: Arc::clone(&delivered_a),
        })),
        true,
        false,
    );
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(RecordingSigner {
            pk: Keys::generate().public_key().to_hex(),
            delivered: Arc::clone(&delivered_b),
        })),
        false,
        false,
    );

    let body = r#"{"id":"req-1","result":"signed-event-json"}"#.to_string();
    let parked = dispatch_one(
        ActorCommand::DeliverSignerResponse {
            response_json: body.clone(),
        },
        &mut identity,
        &mut kernel,
    );
    assert!(parked.is_empty(), "DeliverSignerResponse parks nothing itself");

    // Both registered remote handles received the exact body (each drops a
    // non-matching correlation id internally — the trait contract).
    assert_eq!(delivered_a.lock().unwrap().as_slice(), &[body.clone()]);
    assert_eq!(delivered_b.lock().unwrap().as_slice(), &[body]);
}
