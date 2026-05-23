//! NIP-46 (Nostr Connect / bunker://) signer scaffolding.
//!
//! ## Architecture
//!
//! `Nip46Signer` is the **fully-connected** form: it has a known remote user
//! pubkey (set by the `connect` / `get_public_key` handshake) and routes all
//! sign/encrypt/decrypt calls via a [`Nip46Transport`] trait that the embedder
//! provides.  The trait is what lets the production kernel (with its real relay
//! pool) drive the RPC, while tests can stub it out completely.
//!
//! `Nip46SignerHandle` is the **pre-handshake** form: it carries a parsed
//! `bunker://` URI + local ephemeral keys.  Once the caller has completed the
//! connect / `get_public_key` RPC handshake, `complete(transport, pubkey)` returns
//! the upgraded `Nip46Signer`.
//!
//! ## Design choices
//!
//! - **Transport injection**: The signer does not own the relay pool — per
//!   doctrine D7 (capabilities report; never decide policy), it asks the
//!   kernel to send/receive via a trait, and the kernel applies routing
//!   policy.  This also lets unit tests run without spinning up a relay.
//! - **Cached remote pubkey**: After the first successful handshake we cache
//!   the remote user's pubkey in [`Nip46Payload::cached_remote_user_pubkey_hex`]
//!   so `pubkey()` is synchronous on restore (per applesauce `0867a502`).
//! - **Pending RPCs**: each `sign` / `encrypt` / `decrypt` allocates one
//!   request id (nanoid-equivalent: 11-byte hex) and registers a one-shot
//!   `Sender` in `Nip46Signer::pending`.  The transport delivers responses by
//!   their id; we resolve and drop.

mod handle;
mod mapper;

use std::collections::HashMap;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nostr::{Keys, PublicKey, SecretKey};
use zeroize::Zeroizing;

use crate::bunker::{parse_bunker_uri, BunkerParseError, BunkerUri};
use super::payload::{Nip46Payload, SignerPayload};
use super::traits::{Nip04, Nip44, Signer, SignerBackend, SignerError};
use super::SignerOp;

// `Nip46Rpc` and `Nip46Transport` are defined in the leaf
// [`nmp_signer_iface`] crate so the kernel side can refer to them
// without depending on `nmp-signers` (doctrine **D0**).
pub use nmp_signer_iface::{Nip46Rpc, Nip46Transport};

use mapper::{escape_json, generate_request_id, map_response_to_event};

/// Pending RPC table: request id -> one-shot sender.
type PendingMap = HashMap<String, Sender<Result<String, SignerError>>>;

/// Pre-handshake handle for a NIP-46 connection.
///
/// Produced by [`Nip46SignerHandle::from_bunker_uri`].  Carries the parsed
/// bunker URI + local ephemeral keys.  Call `complete()` after the kernel has
/// completed the connect / `get_public_key` handshake and learned the remote user
/// pubkey.
#[derive(Debug)]
pub struct Nip46SignerHandle {
    uri: BunkerUri,
    local_keys: Keys,
}

impl Nip46SignerHandle {
    /// Parse `bunker://...` and produce a handle.
    #[must_use]
    pub fn from_bunker_uri(s: &str) -> Result<Self, BunkerParseError> {
        let uri = parse_bunker_uri(s)?;
        Ok(Self {
            uri,
            local_keys: Keys::generate(),
        })
    }

    /// Parse and seed with a specific local key. Used by the signer broker
    /// (`nmp-signer-broker`) to restore sessions with a persisted local secret,
    /// and in tests for deterministic key seeding.
    #[must_use]
    pub fn from_bunker_uri_with_local_key(
        s: &str,
        sk: SecretKey,
    ) -> Result<Self, BunkerParseError> {
        let uri = parse_bunker_uri(s)?;
        Ok(Self {
            uri,
            local_keys: Keys::new(sk),
        })
    }

    /// The parsed URI.
    #[must_use]
    pub fn uri(&self) -> &BunkerUri {
        &self.uri
    }

    /// The local ephemeral pubkey (the bunker addresses RPC responses to this).
    #[must_use]
    pub fn local_pubkey(&self) -> PublicKey {
        self.local_keys.public_key()
    }

    /// Promote to a fully-connected signer once the remote handshake has
    /// resolved.  In test contexts the caller can pass `remote_user_pubkey`
    /// directly; in production the kernel performs the `connect` /
    /// `get_public_key` RPC dance via `transport` and supplies the result.
    #[must_use]
    pub fn complete<T: Nip46Transport + 'static>(
        self,
        transport: Arc<T>,
        remote_user_pubkey: PublicKey,
    ) -> Nip46Signer {
        Nip46Signer {
            uri: self.uri,
            local_keys: self.local_keys,
            remote_user_pubkey,
            transport,
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

/// Fully-connected NIP-46 signer.
pub struct Nip46Signer {
    uri: BunkerUri,
    local_keys: Keys,
    remote_user_pubkey: PublicKey,
    transport: Arc<dyn Nip46Transport>,
    /// Pending request id → response channel.
    pending: Arc<Mutex<PendingMap>>,
}

impl std::fmt::Debug for Nip46Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Nip46Signer")
            .field("remote_user_pubkey", &self.remote_user_pubkey.to_hex())
            .field("relays", &self.uri.relays)
            .field("pending_count", &self.pending.lock().map(|m| m.len()).unwrap_or(0))
            .finish_non_exhaustive()
    }
}

impl Nip46Signer {
    /// Restore from a payload.  Requires a transport (the kernel-side relay
    /// dispatcher) and the cached remote-user pubkey.  Returns `NotReady` if
    /// the payload has never been handshaken (no cached pubkey).
    #[must_use]
    pub fn from_payload<T: Nip46Transport + 'static>(
        p: &Nip46Payload,
        transport: Arc<T>,
    ) -> Result<Self, SignerError> {
        let remote_user_pubkey_hex = p
            .cached_remote_user_pubkey_hex
            .as_deref()
            .ok_or_else(|| {
                SignerError::NotReady(
                    "nip46 payload has no cached remote user pubkey; re-handshake required"
                        .to_string(),
                )
            })?;
        let remote_user_pubkey = PublicKey::from_hex(remote_user_pubkey_hex).map_err(|e| {
            SignerError::Backend(format!("invalid cached remote pubkey: {e}"))
        })?;
        let local_sk = SecretKey::from_hex(p.local_secret_hex.as_str())
            .map_err(|e| SignerError::Backend(format!("invalid local secret: {e}")))?;
        let uri = BunkerUri {
            remote_pubkey_hex: p.remote_pubkey_hex.clone(),
            relays: p.relays.clone(),
            secret: p.secret.clone(),
            permissions: p.permissions.clone(),
            // Restore from payload — `extra` is the BunkerUri's catch-all for
            // unrecognised query params (e.g. `name`), which we don't persist;
            // a restored URI is canonical.
            extra: Vec::new(),
        };
        Ok(Self {
            uri,
            local_keys: Keys::new(local_sk),
            remote_user_pubkey,
            transport,
            pending: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Deliver a decoded RPC response.  The kernel calls this when a kind:24133
    /// from the bunker arrives — it decrypts (using `local_keys` + remote
    /// pubkey), parses the NIP-46 RPC envelope, and routes by `id`.
    pub fn resolve_response(&self, id: &str, response: Result<String, SignerError>) {
        if let Ok(mut pending) = self.pending.lock() {
            if let Some(sender) = pending.remove(id) {
                let _ = sender.send(response);
            }
        }
    }

    /// Resolve every in-flight RPC with an error. Called when the signer
    /// session ends (e.g. account removal) so blocked `SignerOp::wait` callers
    /// fail immediately instead of hanging until the sign timeout elapses.
    pub fn drain_pending_with_error(&self, msg: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            for (_id, sender) in pending.drain() {
                let _ = sender.send(Err(SignerError::Rejected(msg.to_string())));
            }
        }
    }

    /// The cached remote user pubkey (sync).
    #[must_use]
    pub fn remote_user_pubkey(&self) -> PublicKey {
        self.remote_user_pubkey
    }

    /// The parsed bunker URI.
    #[must_use]
    pub fn uri(&self) -> &BunkerUri {
        &self.uri
    }

    fn enqueue(&self, method: &str, params_json: &str) -> SignerOp<String> {
        let id = generate_request_id();
        let body_json = format!(
            r#"{{"id":"{id}","method":"{method}","params":{params_json}}}"#,
        );
        let (tx, rx) = mpsc::channel();
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id.clone(), tx);
        } else {
            return SignerOp::err(SignerError::Backend(
                "pending map poisoned".to_string(),
            ));
        }
        let rpc = Nip46Rpc {
            id: id.clone(),
            body_json: body_json.clone(),
            // In a full impl, encrypt `body_json` with NIP-44 to remote
            // pubkey.  Kept as plain JSON here so unit tests can inspect what
            // would have been sent; the production transport is responsible
            // for performing the encryption per its policy contract.
            encrypted_payload: body_json,
            relays: self.uri.relays.clone(),
            remote_pubkey_hex: self.uri.remote_pubkey_hex.clone(),
        };
        if let Err(e) = self.transport.send_rpc(rpc) {
            // The pending entry was registered before `send_rpc` so a response
            // racing the send still resolves.  On a *failed* send no response
            // will ever arrive — drop the entry now, otherwise it leaks for
            // the lifetime of the signer (a slow unbounded growth under a
            // flaky transport).
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&id);
            }
            return SignerOp::err(e);
        }
        SignerOp::Pending(rx)
    }

    /// Number of in-flight RPCs awaiting a response.  Test-only — lets unit
    /// tests assert that failed sends do not leak entries into `pending`.
    #[cfg(test)]
    pub(super) fn pending_len(&self) -> usize {
        self.pending.lock().map(|m| m.len()).unwrap_or(0)
    }
}

impl Signer for Nip46Signer {
    fn backend(&self) -> SignerBackend {
        SignerBackend::Nip46
    }

    fn pubkey(&self) -> PublicKey {
        self.remote_user_pubkey
    }

    fn sign(&self, unsigned: UnsignedEvent) -> SignerOp<SignedEvent> {
        let params_json = match serde_json::to_string(&unsigned) {
            Ok(s) => format!("[{s}]"),
            Err(e) => {
                return SignerOp::err(SignerError::Backend(format!(
                    "serialize unsigned: {e}"
                )))
            }
        };
        let raw_op = self.enqueue("sign_event", &params_json);
        map_response_to_event(raw_op, unsigned, self.remote_user_pubkey)
    }

    fn nip04(&self) -> Option<&dyn Nip04> {
        Some(self)
    }

    fn nip44(&self) -> Option<&dyn Nip44> {
        Some(self)
    }

    fn to_payload(&self) -> SignerPayload {
        SignerPayload::Nip46(Nip46Payload {
            local_secret_hex: Zeroizing::new(self.local_keys.secret_key().to_secret_hex()),
            remote_pubkey_hex: self.uri.remote_pubkey_hex.clone(),
            relays: self.uri.relays.clone(),
            secret: self.uri.secret.clone(),
            permissions: self.uri.permissions.clone(),
            cached_remote_user_pubkey_hex: Some(self.remote_user_pubkey.to_hex()),
        })
    }
}

impl Nip04 for Nip46Signer {
    fn encrypt(&self, recipient: &PublicKey, plaintext: &str) -> SignerOp<String> {
        let params = format!(r#"["{}","{}"]"#, recipient.to_hex(), escape_json(plaintext));
        self.enqueue("nip04_encrypt", &params)
    }
    fn decrypt(&self, sender: &PublicKey, ciphertext: &str) -> SignerOp<String> {
        let params = format!(r#"["{}","{}"]"#, sender.to_hex(), escape_json(ciphertext));
        self.enqueue("nip04_decrypt", &params)
    }
}

impl Nip44 for Nip46Signer {
    fn encrypt(&self, recipient: &PublicKey, plaintext: &str) -> SignerOp<String> {
        let params = format!(r#"["{}","{}"]"#, recipient.to_hex(), escape_json(plaintext));
        self.enqueue("nip44_encrypt", &params)
    }
    fn decrypt(&self, sender: &PublicKey, payload: &str) -> SignerOp<String> {
        let params = format!(r#"["{}","{}"]"#, sender.to_hex(), escape_json(payload));
        self.enqueue("nip44_decrypt", &params)
    }
}

// Response mapping and helpers live in `mapper.rs` to keep this module under
// the 300 LOC soft cap.
