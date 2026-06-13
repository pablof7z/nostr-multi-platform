//! V-06 / issue #960 — NIP-42 AUTH through the ADR-0050 async signer port.
//!
//! Before this fix the kernel signed the kind:22242 AUTH event with a
//! *synchronous* `AuthSignerFn`, and `sync_kernel` cleared that signer whenever
//! a remote (NIP-46 bunker) account was active — so bunker users could never
//! pass a NIP-42 AUTH gate. The fix routes AUTH signing through the SAME async
//! sign seam every other write uses (`sign_*_nonblocking` → park → drain → the
//! obligation re-enters the kernel).
//!
//! These tests pin the remote (parked) path end-to-end:
//!
//! 1. A remote account is active; the kernel binds the AUTH *pubkey* (no
//!    synchronous signer). An inbound AUTH challenge enqueues a
//!    `PendingAuthSign` and the relay stays `ChallengeReceived` — nothing is
//!    dispatched synchronously, no panic, no signer-cleared bail.
//! 2. The actor parks the pending sign through the signer port; a simulated
//!    broker response resolves the op, the `AuthObligation` re-enters
//!    `Kernel::dispatch_signed_auth`, the relay advances to `Authenticating`,
//!    and the outbound `["AUTH", <signed>]` frame is emitted.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use nmp_signer_iface::{SignerError, SignerOp};
use nostr::nips::nip19::FromBech32;
use nostr::{EventBuilder, Keys, SecretKey, Timestamp};
use serde_json::json;

use super::commands::{self, sign_with_account_nonblocking, IdentityRuntime};
use super::pending_sign::{resolve_parked_op, AuthObligation, ParkedOp, ParkedOpSink};
use crate::kernel::{Kernel, RelayFrame};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::remote_signer::RemoteSignerHandle;
use crate::subs::RelayAuthState;
use crate::substrate::{SignedEvent, UnsignedEvent};

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
const RELAY_URL: &str = "wss://auth.example";
const CHALLENGE: &str = "challenge-12345";

fn test_keys() -> Keys {
    Keys::new(SecretKey::from_bech32(TEST_NSEC).expect("valid nsec"))
}

/// A remote signer whose `sign` parks (returns `SignerOp::Pending`).
#[derive(Debug)]
struct PendingRemoteSigner {
    keys: Keys,
    pk: String,
    sign_count: Arc<AtomicU32>,
    /// Keeps the parked sender alive so the op stays `Pending` (a dropped sender
    /// would close the channel and resolve the poll to a disconnect error).
    last_sender: Mutex<Option<Sender<Result<SignedEvent, SignerError>>>>,
}

impl PendingRemoteSigner {
    fn new(keys: Keys) -> Self {
        let pk = keys.public_key().to_hex();
        Self {
            keys,
            pk,
            sign_count: Arc::new(AtomicU32::new(0)),
            last_sender: Mutex::new(None),
        }
    }

    fn signed_for(&self, unsigned: &UnsignedEvent) -> SignedEvent {
        let kind = nostr::Kind::from_u16(unsigned.kind as u16);
        let tags = unsigned
            .tags
            .iter()
            .filter_map(|t| nostr::Tag::parse(t).ok())
            .collect::<Vec<_>>();
        let event = EventBuilder::new(kind, &unsigned.content)
            .tags(tags)
            .custom_created_at(Timestamp::from(unsigned.created_at))
            .sign_with_keys(&self.keys)
            .expect("stub sign");
        SignedEvent {
            id: event.id.to_hex(),
            sig: event.sig.to_string(),
            unsigned: UnsignedEvent {
                pubkey: event.pubkey.to_hex(),
                kind: event.kind.as_u16() as u32,
                tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                content: event.content.clone(),
                created_at: event.created_at.as_secs(),
            },
        }
    }
}

impl RemoteSignerHandle for PendingRemoteSigner {
    fn pubkey_hex(&self) -> String {
        self.pk.clone()
    }

    fn signer_kind(&self) -> &'static str {
        "nip46"
    }

    fn sign(&self, _unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        self.sign_count.fetch_add(1, Ordering::Relaxed);
        let (tx, rx): (
            Sender<Result<SignedEvent, SignerError>>,
            Receiver<Result<SignedEvent, SignerError>>,
        ) = channel();
        // Hold the sender so the op stays Pending; the round-trip test swaps in a
        // controlled channel before draining (mirrors `sign_event_for_account_tests`).
        *self.last_sender.lock().unwrap() = Some(tx);
        SignerOp::Pending(rx)
    }

    fn nip44_encrypt(&self, _recipient_pubkey: &str, _plaintext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Backend("unused".into()))
    }

    fn nip44_decrypt(&self, _sender_pubkey: &str, _ciphertext: &str) -> SignerOp<String> {
        SignerOp::err(SignerError::Backend("unused".into()))
    }

    fn deliver_response(&self, _response_json: &str) {}
}

fn fresh_identity() -> IdentityRuntime {
    IdentityRuntime::new(
        commands::new_bunker_handshake_slot(),
        commands::new_signer_state_slot(),
    )
}

/// Sign in a remote account, bind its AUTH pubkey on the kernel (no synchronous
/// signer), and feed an inbound AUTH challenge.
fn setup_remote_and_challenge(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
) -> (String, Arc<AtomicU32>) {
    let signer = PendingRemoteSigner::new(test_keys());
    let signer_pk = signer.pubkey_hex();
    let sign_count = Arc::clone(&signer.sign_count);
    commands::add_signer(
        identity,
        kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(signer)),
        true,
        false,
    );

    // sync_kernel runs inside add_signer; it must NOT have cleared auth and must
    // have bound the remote AUTH pubkey (the fix). Feed the AUTH challenge
    // through the production ingest path (`handle_message`).
    let frame = RelayFrame::Text(json!(["AUTH", CHALLENGE]).to_string());
    let out = kernel.handle_message(RelayRole::Content, RELAY_URL, frame);
    assert!(
        out.is_empty(),
        "a remote-account AUTH challenge must NOT dispatch synchronously — it parks"
    );
    (signer_pk, sign_count)
}

#[test]
fn remote_auth_challenge_parks_and_does_not_clear_or_bail() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer_pk, _count) = setup_remote_and_challenge(&mut identity, &mut kernel);

    let pending = kernel.take_pending_auth_signs();
    assert_eq!(pending.len(), 1, "exactly one pending auth-sign enqueued");
    assert_eq!(pending[0].role, RelayRole::Content);
    assert_eq!(pending[0].relay_url, RELAY_URL);
    assert_eq!(pending[0].unsigned.kind, 22242);
    assert_eq!(pending[0].unsigned.pubkey, signer_pk);
    assert_eq!(pending[0].challenge, CHALLENGE);
    assert_eq!(
        kernel.relay_auth_state_for_test(RelayRole::Content),
        Some(RelayAuthState::ChallengeReceived),
        "relay stays ChallengeReceived until the signed frame resolves"
    );
}

#[test]
fn remote_auth_round_trip_authenticates_via_signer_port() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer_pk, sign_count) = setup_remote_and_challenge(&mut identity, &mut kernel);

    let req = kernel
        .take_pending_auth_signs()
        .into_iter()
        .next()
        .expect("one pending auth-sign");

    // Actor side: route the pending sign through the SAME async port every other
    // write uses. A remote signer parks (Pending).
    let mut op = sign_with_account_nonblocking(&identity, &signer_pk, &req.unsigned)
        .expect("remote sign op");
    assert!(op.poll().is_none(), "remote signer parks before broker responds");
    assert_eq!(sign_count.load(Ordering::Relaxed), 1);
    let deadline = identity.sign_deadline_for(Some(&signer_pk));
    let mut parked = ParkedOp::auth(op, req.role, req.relay_url.clone(), req.challenge.clone(), deadline);

    // Drain BEFORE the broker responds: still pending, no obligation.
    let outcome = resolve_parked_op(&mut parked, &mut kernel);
    assert!(outcome.keep, "still pending — keep the parked op");
    assert!(outcome.auth.is_none());
    assert_eq!(
        kernel.relay_auth_state_for_test(RelayRole::Content),
        Some(RelayAuthState::ChallengeReceived)
    );

    // Simulated broker response: build the signed AUTH event and push it into a
    // controlled channel swapped into the parked op (mirrors a later idle tick).
    let responder = PendingRemoteSigner::new(test_keys());
    let signed = responder.signed_for(&req.unsigned);
    {
        let ParkedOpSink::Auth { op, .. } = &mut parked.sink else {
            panic!("expected an Auth sink");
        };
        let (tx, rx): (
            Sender<Result<SignedEvent, SignerError>>,
            Receiver<Result<SignedEvent, SignerError>>,
        ) = channel();
        *op = SignerOp::Pending(rx);
        tx.send(Ok(signed.clone())).unwrap();
    }

    let outcome = resolve_parked_op(&mut parked, &mut kernel);
    assert!(!outcome.keep, "resolved — drop the parked op");
    assert!(outcome.changed, "kernel state changed");
    let obligation = outcome.auth.expect("an auth obligation re-entered the loop");
    let frames = match obligation {
        AuthObligation::Dispatch {
            role,
            relay_url,
            challenge,
            signed,
        } => kernel.dispatch_signed_auth(role, &relay_url, &challenge, signed),
        AuthObligation::Failed { reason, .. } => panic!("unexpected sign failure: {reason}"),
    };

    assert_eq!(
        kernel.relay_auth_state_for_test(RelayRole::Content),
        Some(RelayAuthState::Authenticating),
        "relay must advance to Authenticating once the signed AUTH dispatches"
    );
    assert_eq!(frames.len(), 1, "one outbound AUTH frame");
    assert_eq!(frames[0].relay_url, RELAY_URL);
    assert!(frames[0].text.contains("\"AUTH\""), "outbound is a CLIENT-AUTH frame");
    assert!(frames[0].text.contains(&signed.id), "outbound carries the signed event id");
}
