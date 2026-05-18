//! `RemoteSignerHandle` — the actor-facing trait for signers whose key material
//! lives outside the kernel (NIP-46 today; NIP-07, hardware wallets future).
//!
//! Implementations live in `nmp-signers` (which depends on `nmp-core`, so it
//! can see this trait). The actor only ever holds `Box<dyn RemoteSignerHandle>`
//! — keeping doctrine **D0** intact (`nmp-core` must not import `nmp-signers`).

use nmp_signer_iface::SignerOp;

use crate::substrate::{SignedEvent, UnsignedEvent};

/// Trait the actor uses to drive remote signers (NIP-46 etc.).
///
/// Signing is potentially async — `sign` returns a `SignerOp<SignedEvent>`
/// that the actor polls or awaits via its existing publish-queue plumbing.
///
/// `deliver_relay_event` is the inbound hook: when the relay subscription
/// produces a kind:24133 event addressed to the local ephemeral pubkey, the
/// actor calls this so the signer can resolve a pending RPC by id.
pub trait RemoteSignerHandle: Send + Sync + std::fmt::Debug {
    /// The user's pubkey (hex). Synchronous + cached after handshake.
    fn pubkey_hex(&self) -> String;

    /// Stable label for the snapshot (`"nip46"`, `"nip07"`, …).
    fn signer_kind(&self) -> &'static str;

    /// Sign an unsigned event template. Returns a `SignerOp` so remote
    /// signers can resolve asynchronously without blocking the actor thread.
    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent>;

    /// Hand an inbound NIP-46 RPC response event to the signer. JSON is the
    /// already-decrypted RPC payload body (`{"id":"...","result":"..."}`).
    /// No-op for signers that don't have a relay-driven response path.
    fn deliver_rpc_response(&self, response_json: &str);
}
