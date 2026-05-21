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

    /// Opaque JSON payload the actor can place in secure storage and later
    /// hand back to the broker. `None` means the signer cannot be restored
    /// without user interaction.
    fn persistence_payload_json(&self) -> Option<String> {
        None
    }

    /// Sign an unsigned event template. Returns a `SignerOp` so remote
    /// signers can resolve asynchronously without blocking the actor thread.
    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent>;

    /// NIP-44 encrypt `plaintext` to `recipient_pubkey`. Used to build the
    /// kind:13 seal in a NIP-59 gift-wrap (ADR-0026). The ephemeral kind:1059
    /// outer wrap is actor-local — the actor generates that ephemeral key
    /// itself — so only the seal needs this method.
    ///
    /// `recipient_pubkey` is lowercase hex. `&str` (not `&PublicKey`) keeps
    /// `nmp-core` free of a `nostr` type in the trait surface, matching
    /// `sign()`, which takes the substrate `&UnsignedEvent`.
    ///
    /// Returns `SignerOp::Ready(Ok(ciphertext))` for in-memory signers;
    /// `SignerOp::Pending(..)` for NIP-46 bunkers (asynchronous RPC).
    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String>;

    /// NIP-44 decrypt `ciphertext` from `sender_pubkey`. Used for inbound
    /// kind:13 seal decryption on the DM receive path (ADR-0026).
    ///
    /// `sender_pubkey` is lowercase hex. See [`Self::nip44_encrypt`] for the
    /// `&str`-vs-`&PublicKey` and `SignerOp` rationale.
    fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> SignerOp<String>;

    /// Hand an inbound NIP-46 RPC response event to the signer. JSON is the
    /// already-decrypted RPC payload body (`{"id":"...","result":"..."}`).
    /// No-op for signers that don't have a relay-driven response path.
    fn deliver_rpc_response(&self, response_json: &str);

    /// Called by the actor before the signer is removed. Implementations that
    /// hold in-flight async requests should resolve them with an error so
    /// callers fail fast rather than waiting for a timeout.
    fn disconnect(&self) {}
}
