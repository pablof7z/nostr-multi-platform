//! Shared fixtures for the `SignEventForAccount` / `SignEventForReturn`
//! dispatch-arm tests: a known-good local nsec, an unsigned draft-event
//! builder, a bunker stub whose `sign()` returns `SignerOp::Pending` so a
//! test can drive the broker round-trip by hand, a fresh `IdentityRuntime`
//! builder, and a continuation-capture helper shared by every backend /
//! scenario submodule.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use nmp_signer_iface::{SignerError, SignerOp};
use nostr::nips::nip19::FromBech32;
use nostr::{EventBuilder, Keys, SecretKey, Timestamp};

use crate::actor::commands::{self, IdentityRuntime};
use crate::actor::SignContinuation;
use nmp_signer_iface::RemoteSignerHandle;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

/// Known-good test nsec (shared with `remote_signer_tests`).
pub(super) const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

pub(super) fn test_keys() -> Keys {
    Keys::new(SecretKey::from_bech32(TEST_NSEC).expect("valid nsec"))
}

/// An unsigned kind:24242-shaped draft (any kind works; this mirrors a Blossom
/// auth event). `created_at` is already stamped (the dispatch arm does not
/// re-stamp — the caller owns D7 before constructing the command).
pub(super) fn draft_unsigned(pubkey_hint: &str) -> UnsignedEvent {
    UnsignedEvent {
        pubkey: pubkey_hint.to_string(),
        kind: 24242,
        tags: vec![
            vec!["t".to_string(), "upload".to_string()],
            vec!["x".to_string(), "ab".repeat(32)],
            vec!["expiration".to_string(), "1700000300".to_string()],
        ],
        content: "Upload blob".to_string(),
        created_at: 1_700_000_000,
    }
}

/// A remote-signer stub whose `sign` returns `SignerOp::Pending` — the broker
/// round-trip is driven by the test through the returned [`Sender`]. This is
/// the bunker shape the dispatch arm must park (the existing
/// `remote_signer_tests::StubRemoteSigner` resolves `Ready`, so the park path
/// is never exercised there).
#[derive(Debug)]
pub(super) struct PendingRemoteSigner {
    keys: Keys,
    pk: String,
    pub(super) sign_count: Arc<AtomicU32>,
    /// Self-described per-op budget (ADR-0050 §D4). Defaults to the NIP-46 5s
    /// budget; the named-roster-key deadline test overrides it to a 90s
    /// NIP-55-style budget to prove the parked deadline reflects the SIGNING
    /// account's budget rather than the active account's.
    op_timeout: std::time::Duration,
    /// Each `sign()` stashes the receiver end here so the dispatch arm can park
    /// it; the test holds the matching sender to resolve the broker round-trip.
    last_sender: Mutex<Option<Sender<Result<SignedEvent, SignerError>>>>,
}

impl PendingRemoteSigner {
    pub(super) fn new(keys: Keys) -> Self {
        Self::with_op_timeout(keys, nmp_signer_iface::PENDING_SIGN_TIMEOUT)
    }

    pub(super) fn with_op_timeout(keys: Keys, op_timeout: std::time::Duration) -> Self {
        let pk = keys.public_key().to_hex();
        Self {
            keys,
            pk,
            sign_count: Arc::new(AtomicU32::new(0)),
            op_timeout,
            last_sender: Mutex::new(None),
        }
    }

    /// Build the `SignedEvent` the broker would return for `unsigned`.
    pub(super) fn signed_for(&self, unsigned: &UnsignedEvent) -> SignedEvent {
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

    fn op_timeout(&self) -> std::time::Duration {
        self.op_timeout
    }

    fn sign(&self, _unsigned: &UnsignedEvent) -> SignerOp<SignedEvent> {
        self.sign_count.fetch_add(1, Ordering::Relaxed);
        let (tx, rx): (
            Sender<Result<SignedEvent, SignerError>>,
            Receiver<Result<SignedEvent, SignerError>>,
        ) = channel();
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

pub(super) fn fresh_identity() -> IdentityRuntime {
    IdentityRuntime::new(
        commands::new_bunker_handshake_slot(),
        commands::new_signer_state_slot(),
    )
}

/// Captured continuation outcome: `Some(Ok(signed))` / `Some(Err(reason))` once
/// the continuation ran, `None` while it has not.
pub(super) type CapturedOutcome = Arc<Mutex<Option<Result<SignedEvent, String>>>>;

pub(super) fn capture_continuation() -> (CapturedOutcome, SignContinuation) {
    let captured: CapturedOutcome = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&captured);
    let continuation = SignContinuation::new(move |outcome| {
        *slot.lock().unwrap() = Some(outcome);
    });
    (captured, continuation)
}
