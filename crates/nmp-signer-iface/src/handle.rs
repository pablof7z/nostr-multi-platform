//! `RemoteSignerHandle` — the actor-facing trait for signers whose key material
//! lives outside the kernel (NIP-46 today; NIP-55/hardware-wallets future).
//!
//! Implementations live in `nmp-signers`; the kernel actor in `nmp-core` only
//! ever holds `Box<dyn RemoteSignerHandle>`. The trait is dependency-light
//! vocabulary — it names only this crate's own interface types (`SignerOp`,
//! `SignedEvent`, `UnsignedEvent`) — so it lives in this tier-0 interface crate
//! rather than in `nmp-core`. `nmp-core` re-exports it (`nmp_core::RemoteSignerHandle`)
//! so existing import paths are unchanged.

use std::time::Duration;

use crate::error::SignerError;
use crate::nip44_session::{
    Nip44DecryptBatchRequest, Nip44DecryptBatchResult, Nip44DecryptSessionBeginRequest,
    Nip44DecryptSessionEndRequest, Nip44DecryptSessionGrant,
};
use crate::op::SignerOp;
use crate::signing::{SignedEvent, UnsignedEvent};
use crate::PENDING_SIGN_TIMEOUT;

/// Trait the actor uses to drive remote signers (NIP-46, NIP-55, etc.).
///
/// Signing is potentially async — `sign` returns a `SignerOp<SignedEvent>`
/// that the actor polls or awaits via its existing publish-queue plumbing.
///
/// `deliver_response` is the inbound hook: when a relay subscription
/// produces a kind:24133 event (NIP-46), or the capability bridge reports a
/// result (NIP-55), the actor calls this so the signer can resolve a pending
/// op by correlation id. Content-agnostic: the already-decoded JSON is passed
/// verbatim to the signer.
pub trait RemoteSignerHandle: Send + Sync + std::fmt::Debug {
    /// The user's pubkey (hex). Synchronous + cached after handshake.
    fn pubkey_hex(&self) -> String;

    /// Stable label for the snapshot (`"nip46"`, `"nip55"`, …).
    fn signer_kind(&self) -> &'static str;

    /// Opaque JSON payload the actor can place in secure storage and later
    /// hand back to the broker/factory. `None` means the signer cannot be
    /// restored without user interaction.
    fn persistence_payload_json(&self) -> Option<String> {
        None
    }

    /// Per-op deadline budget for parked signer operations.
    ///
    /// Default is 5s (correct for a NIP-46 relay RPC). `Nip55Signer` overrides
    /// to 90s because an Android Intent round-trip requires the user to
    /// foreground Amber and tap approve (ADR-0072 D3). The actor reads this via
    /// the handle it already holds; the constant itself lives in
    /// `nmp-signer-iface` (alongside this trait) so `nmp-core` never sees a
    /// NIP-55 noun.
    ///
    /// Named `op_timeout` (ADR-0072 D4 — hard-break rename from `sign_timeout`,
    /// no compat alias per repo rule) because it now budgets all three port
    /// verbs — `sign`, `nip44_encrypt`, `nip44_decrypt` — uniformly. One budget
    /// per backend (NIP-46 = 5s, NIP-55 = 90s); per-verb differentiation inside
    /// one backend is deliberately not provided until a real backend needs it.
    fn op_timeout(&self) -> Duration {
        PENDING_SIGN_TIMEOUT
    }

    /// Sign an unsigned event template. Returns a `SignerOp` so remote
    /// signers can resolve asynchronously without blocking the actor thread.
    fn sign(&self, unsigned: &UnsignedEvent) -> SignerOp<SignedEvent>;

    /// NIP-44 encrypt `plaintext` to `recipient_pubkey`. Used to build the
    /// kind:13 seal in a NIP-59 gift-wrap (ADR-0072). The ephemeral kind:1059
    /// outer wrap is actor-local — the actor generates that ephemeral key
    /// itself — so only the seal needs this method.
    ///
    /// `recipient_pubkey` is lowercase hex. `&str` (not `&PublicKey`) keeps
    /// `nmp-core` free of a `nostr` type in the trait surface, matching
    /// `sign()`, which takes the `&UnsignedEvent`.
    ///
    /// Returns `SignerOp::Ready(Ok(ciphertext))` for in-memory signers;
    /// `SignerOp::Pending(..)` for remote signers (asynchronous RPC/IPC).
    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String>;

    /// NIP-44 decrypt `ciphertext` from `sender_pubkey`. Used for inbound
    /// kind:13 seal decryption on the DM receive path (ADR-0072).
    ///
    /// `sender_pubkey` is lowercase hex. See [`Self::nip44_encrypt`] for the
    /// `&str`-vs-`&PublicKey` and `SignerOp` rationale.
    fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> SignerOp<String>;

    /// Begin an optional scoped NIP-44 decrypt session.
    ///
    /// Non-capable signers return [`SignerError::Unsupported`] by default so
    /// the actor can keep using the ADR-0072 scalar fallback.  Implementations
    /// that support the NMP NIP-46 extension should return a signer-owned
    /// grant and keep reusable conversation secrets inside the signer boundary.
    fn nip44_decrypt_session_begin(
        &self,
        _request: Nip44DecryptSessionBeginRequest,
    ) -> SignerOp<Nip44DecryptSessionGrant> {
        SignerOp::err(SignerError::Unsupported(
            "nip44 decrypt sessions are not supported by this signer".to_string(),
        ))
    }

    /// Decrypt a batch of NIP-44 ciphertexts inside a scoped session.
    ///
    /// The request and result are secret-bearing; implementations must not log
    /// raw ciphertexts, plaintexts, or session ids.
    fn nip44_decrypt_batch(
        &self,
        _request: Nip44DecryptBatchRequest,
    ) -> SignerOp<Nip44DecryptBatchResult> {
        SignerOp::err(SignerError::Unsupported(
            "nip44 decrypt batches are not supported by this signer".to_string(),
        ))
    }

    /// End a scoped NIP-44 decrypt session.
    ///
    /// Session end is best-effort.  Non-capable signers return unsupported by
    /// default; capable signers should ask the remote signer to release any
    /// grant state but must also rely on signer-side expiry.
    fn nip44_decrypt_session_end(&self, _request: Nip44DecryptSessionEndRequest) -> SignerOp<bool> {
        SignerOp::err(SignerError::Unsupported(
            "nip44 decrypt session cleanup is not supported by this signer".to_string(),
        ))
    }

    /// Hand an inbound response to the signer for correlation-keyed dispatch.
    ///
    /// - **NIP-46**: `response_json` is the already-decrypted kind:24133 RPC
    ///   body (`{"id":"...","result":"..."}`).
    /// - **NIP-55**: `response_json` is the serialized
    ///   [`crate::ExternalSignerResponse`] from the capability bridge.
    ///
    /// No-op for signers that don't have an async response path (e.g. local
    /// key signer). Implementations silently drop malformed input so a bad
    /// frame degrades into the originating operation's normal timeout rather
    /// than poisoning the signer state.
    ///
    /// Named `deliver_response` (not `deliver_rpc_response`) because NIP-55
    /// is not RPC-based. This is the ADR-0072 hard-break rename; no compat
    /// alias is provided (no-compat-aliases rule).
    fn deliver_response(&self, response_json: &str);

    /// Called by the actor before the signer is removed. Implementations that
    /// hold in-flight async requests should resolve them with an error so
    /// callers fail fast rather than waiting for a timeout.
    fn disconnect(&self) {}
}
