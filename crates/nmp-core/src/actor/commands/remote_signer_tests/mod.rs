#![cfg(test)]
//! Stage 3 of NIP-46 wiring: actor-side `RemoteSignerHandle` plumbing.
//!
//! These tests drive the new command handlers + dispatch arms with a stub
//! `RemoteSignerHandle` impl — Stage 4 (broker) ships real NIP-46 transport,
//! but the actor MUST treat the trait as a first-class signer regardless of
//! the impl behind it. D0 stays clean: the stub lives in `nmp-core`'s test
//! tree, NOT in `nmp-signers`.
//!
//! ## Sub-modules
//! - `account_tests`   — signer registration, handshake progress, sign routing
//! - `projection_tests`— typed sidecar projections (bunker_handshake, signer_state)
//! - `dispatch_tests`  — end-to-end actor dispatch (runs the `run_actor` loop)
//! - `nip44_tests`     — NIP-44 round-trip + error surface via RemoteSignerHandle

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use nmp_signer_iface::SignerOp;
use nostr::nips::nip19::FromBech32;
use nostr::{EventBuilder, Keys, SecretKey, Timestamp};

use super::*;
use crate::actor::commands::identity::{sign_active_nonblocking, IdentityRuntime};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::remote_signer::RemoteSignerHandle;
use crate::substrate::{SignedEvent, UnsignedEvent};

mod helpers_tests;
pub(super) use helpers_tests::{fresh, stub_signer};

mod account_tests;
mod dispatch_tests;
mod nip44_tests;
mod projection_tests;

/// nsec from `commands::tests` — known-good test key.
pub(super) const TEST_NSEC: &str =
    "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// Stub `RemoteSignerHandle` for Stage 3 plumbing tests. Holds a `Keys` and
/// signs synchronously via `SignerOp::ok(...)`. Production NIP-46 signers
/// live in `nmp-signers`; D0 still forbids that import here so we cannot
/// reach for the real impl — a stub is the correct shape for actor-side
/// plumbing tests.
#[derive(Debug)]
pub(super) struct StubRemoteSigner {
    keys: Keys,
    pk: String,
    sign_count: Arc<AtomicU32>,
}

impl StubRemoteSigner {
    pub(super) fn new(keys: Keys) -> Self {
        let pk = keys.public_key().to_hex();
        Self {
            keys,
            pk,
            sign_count: Arc::new(AtomicU32::new(0)),
        }
    }

    pub(super) fn sign_count_handle(&self) -> Arc<AtomicU32> {
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

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
        // Real NIP-44 v2 against the stub's own keys (ADR-0026). The stub must
        // behave like a production signer for actor-side plumbing tests; an
        // error stub would be a landmine for any future test exercising the
        // seal path. D0 still holds — `nostr::nips::nip44` is a leaf crypto
        // crate, not `nmp-signers`.
        let recipient = match nostr::PublicKey::from_hex(recipient_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                return SignerOp::err(nmp_signer_iface::SignerError::Backend(format!(
                    "stub: invalid recipient pubkey: {e}"
                )))
            }
        };
        SignerOp::Ready(
            nostr::nips::nip44::encrypt(
                self.keys.secret_key(),
                &recipient,
                plaintext,
                nostr::nips::nip44::Version::V2,
            )
            .map_err(|e| {
                nmp_signer_iface::SignerError::Backend(format!("stub nip44 encrypt: {e}"))
            }),
        )
    }

    fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> SignerOp<String> {
        let sender = match nostr::PublicKey::from_hex(sender_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                return SignerOp::err(nmp_signer_iface::SignerError::Backend(format!(
                    "stub: invalid sender pubkey: {e}"
                )))
            }
        };
        SignerOp::Ready(
            nostr::nips::nip44::decrypt(self.keys.secret_key(), &sender, ciphertext).map_err(
                |e| nmp_signer_iface::SignerError::Backend(format!("stub nip44 decrypt: {e}")),
            ),
        )
    }

    fn deliver_response(&self, _response_json: &str) {
        // Stub: no-op. NIP-46 inbound routing is the broker's job (Stage 4).
    }
}

