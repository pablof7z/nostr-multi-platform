//! NIP-46 transport contract.
//!
//! [`Nip46Rpc`] is the value a NIP-46 signer emits when it wants the kernel
//! to publish a kind:24133 event on its behalf.  [`Nip46Transport`] is the
//! trait the kernel (or a test stub) implements to actually move the bytes.
//!
//! Hoisting these into the iface crate lets `nmp-core` hold an
//! `Arc<dyn Nip46Transport>` for the signer broker without taking a
//! dependency on `nmp-signers` (doctrine **D0**).

use crate::error::SignerError;

/// Outbound RPC the signer needs the kernel to perform.
#[derive(Clone, Debug)]
pub struct Nip46Rpc {
    /// Request id (echoed in the response).
    pub id: String,
    /// JSON-encoded request body (NIP-46 RPC envelope: `{id, method, params}`).
    pub body_json: String,
    /// Payload body to publish as kind:24133 after the transport applies
    /// NIP-46 encryption.
    pub encrypted_payload: String,
    /// Target relays (mirrors what `bunker://?relay=...` declared).
    pub relays: Vec<String>,
    /// Remote pubkey to address the kind:24133 event to (in a `p` tag).
    pub remote_pubkey_hex: String,
}

/// The transport contract.  The production kernel implements this; tests can
/// implement it with `Vec<Nip46Rpc>` + an inject-response helper.
pub trait Nip46Transport: Send + Sync + std::fmt::Debug {
    /// Send an RPC.  The signer holds a `Sender<Result<String, SignerError>>`
    /// keyed by `Nip46Rpc.id`; the transport delivers the decrypted response
    /// body by invoking a `resolve_response` helper on the signer (or, in
    /// practice, by routing through a kernel-owned dispatch table).
    ///
    /// The signer never blocks waiting for the response inside `send_rpc`; the
    /// response arrives later via the `Sender` that lives in the signer's
    /// pending-RPC map.
    fn send_rpc(&self, rpc: Nip46Rpc) -> Result<(), SignerError>;

    /// Hint that the underlying subscription was rebuilt.  Signer may re-send
    /// pending RPCs.  Default: no-op.
    fn reconnect_hint(&self) {}
}
