//! `RemoteSignerHandle` — the actor-facing trait for signers whose key material
//! lives outside the kernel (NIP-46 today; NIP-55/hardware-wallets future).
//!
//! Implementations live in `nmp-signers` (which depends on `nmp-core`, so it
//! can see this trait). The actor only ever holds `Box<dyn RemoteSignerHandle>`
//! — keeping doctrine **D0** intact (`nmp-core` must not import `nmp-signers`).

use std::time::Duration;

use nmp_signer_iface::SignerOp;

use crate::substrate::{SignedEvent, UnsignedEvent};

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
    /// foreground Amber and tap approve (ADR-0048 D3). The actor reads this via
    /// the handle it already holds; the constant itself lives in
    /// `nmp-signer-iface` (not here) so `nmp-core` never sees a NIP-55 noun.
    ///
    /// Named `op_timeout` (ADR-0050 D4 — hard-break rename from `sign_timeout`,
    /// no compat alias per repo rule) because it now budgets all three port
    /// verbs — `sign`, `nip44_encrypt`, `nip44_decrypt` — uniformly. One budget
    /// per backend (NIP-46 = 5s, NIP-55 = 90s); per-verb differentiation inside
    /// one backend is deliberately not provided until a real backend needs it.
    fn op_timeout(&self) -> Duration {
        nmp_signer_iface::PENDING_SIGN_TIMEOUT
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
    /// `SignerOp::Pending(..)` for remote signers (asynchronous RPC/IPC).
    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String>;

    /// NIP-44 decrypt `ciphertext` from `sender_pubkey`. Used for inbound
    /// kind:13 seal decryption on the DM receive path (ADR-0026).
    ///
    /// `sender_pubkey` is lowercase hex. See [`Self::nip44_encrypt`] for the
    /// `&str`-vs-`&PublicKey` and `SignerOp` rationale.
    fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> SignerOp<String>;

    /// Hand an inbound response to the signer for correlation-keyed dispatch.
    ///
    /// - **NIP-46**: `response_json` is the already-decrypted kind:24133 RPC
    ///   body (`{"id":"...","result":"..."}`).
    /// - **NIP-55**: `response_json` is the serialized
    ///   [`nmp_signer_iface::ExternalSignerResponse`] from the capability bridge.
    ///
    /// No-op for signers that don't have an async response path (e.g. local
    /// key signer). Implementations silently drop malformed input so a bad
    /// frame degrades into the originating operation's normal timeout rather
    /// than poisoning the signer state.
    ///
    /// Named `deliver_response` (not `deliver_rpc_response`) because NIP-55
    /// is not RPC-based. This is the ADR-0048 hard-break rename; no compat
    /// alias is provided (no-compat-aliases rule).
    fn deliver_response(&self, response_json: &str);

    /// Called by the actor before the signer is removed. Implementations that
    /// hold in-flight async requests should resolve them with an error so
    /// callers fail fast rather than waiting for a timeout.
    fn disconnect(&self) {}
}
