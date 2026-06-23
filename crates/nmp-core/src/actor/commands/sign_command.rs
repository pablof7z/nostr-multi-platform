//! `SignCommand` — ADR-0050 signer-session capability port verbs
//! (`sign | nip44_encrypt | nip44_decrypt`).
//!
//! Grouped under `ActorCommand::Sign(SignCommand)`. Dispatch home:
//! `actor/signer_port_dispatch.rs` (the capability-port dispatch seam).
//!
//! D0: `nip44_*` is a crypto capability (present since ADR-0026), not an app
//! noun. D13: the continuation receives only the signed event / ciphertext /
//! plaintext, never raw key bytes. D8: the continuation runs on the actor
//! thread and MUST only enqueue further work, never block.

use super::super::{CipherContinuation, SignContinuation};

/// Backend-transparent signing / cipher verbs. The dispatch arm routes
/// through `sign_active_nonblocking` / `sign_with_account_nonblocking` /
/// `RemoteSignerHandle::nip44_*`; local keys resolve `Ready` and the
/// continuation runs inline on the actor thread, while NIP-46 / NIP-55
/// resolve `Pending` and park under the `SignContinuation` /
/// `CipherContinuation` sink — the idle-loop drain invokes the continuation
/// when the broker turns the request around (or on timeout, with an `Err`).
#[derive(Debug)]
pub enum SignCommand {
    /// Sign an unsigned event using the named account's signer and park the
    /// result in the `signed_events` snapshot projection keyed by
    /// `correlation_id`. The caller polls the projection to retrieve the
    /// signed event JSON. Works for both local nsec (resolves immediately) and
    /// NIP-46 bunker (resolves asynchronously via a parked `ParkedOp` with the
    /// `SignedEventsProjection` sink).
    ///
    /// Unlike every other sign path in the actor, this NEVER publishes — the
    /// signed event is handed straight back to the host through the projection
    /// so the host can attach it to an out-of-band transport (e.g. a Blossom
    /// upload `Authorization: Nostr …` header). This closes the D13 gap where a
    /// host that needed a signed auth event had to read raw private key bytes
    /// across the FFI boundary, which is impossible for NIP-46 bunker users.
    ///
    /// `unsigned_json` is a JSON object with fields:
    ///   `{ "kind": u64, "content": str, "tags": [[str, ...], ...], "created_at": u64 }`
    /// The `created_at` field is advisory — the actor re-stamps it from the
    /// kernel clock (D7) so the host never owns wall-clock time.
    ///
    /// `account_pubkey` is the hex pubkey of the registered signer to use.
    /// Pass the empty string `""` to use the active account.
    EventForReturn {
        account_pubkey: String,
        unsigned_json: String,
        correlation_id: String,
    },
    /// Generic, backend-transparent sign-account port for `ProtocolCommand`
    /// workers (ADR-0043 Decision 2). Sign `unsigned` with the named account
    /// (`signer_pubkey = Some(hex)`) or the active account (`None`), then
    /// invoke `continuation` with the resolved [`SignedEvent`] (or an error
    /// string).
    ///
    /// `signer_pubkey` matches the publish-path field byte-for-byte
    /// (`PublishUnsignedEvent` / `PublishUnsignedEventToRelays`): `None` =
    /// active account, `Some(pubkey)` = a named roster key.
    ///
    // V-78 reconcile (done): `nmp-nip57`'s `FetchLnurlInvoiceCommand` consumes
    // this port to sign the kind:9734 zap request (active account →
    // `signer_pubkey: None`), so a NIP-46 bunker can zap through the SAME seam
    // as a local nsec. One signing seam, both backends; the redundant
    // `ProtocolCommandContext::sign_active_nonblocking` method it used to call
    // is gone.
    EventForAccount {
        /// The unsigned event to sign. `created_at` should already be stamped
        /// by the caller from the kernel clock (D7).
        unsigned: crate::substrate::UnsignedEvent,
        /// `None` = active account; `Some(hex)` = a named roster key.
        signer_pubkey: Option<String>,
        /// Invoked with the resolved sign outcome — inline (local) or from the
        /// idle-loop drain (bunker / timeout).
        continuation: SignContinuation,
    },
    /// Backend-transparent NIP-44 ENCRYPT-account port — the cipher sibling of
    /// [`Self::EventForAccount`] (ADR-0050 §D1). Encrypt `plaintext` to
    /// `peer_pubkey` with the named (`Some(hex)`) or active (`None`) account,
    /// then invoke `continuation` with the ciphertext (or error). Local
    /// accounts run `nostr::nips::nip44` inside the identity runtime (D13);
    /// remote accounts route through `RemoteSignerHandle::nip44_encrypt` and
    /// park under the `CipherContinuation` sink.
    Nip44EncryptForAccount {
        /// Recipient pubkey (lowercase hex) the plaintext is encrypted to.
        peer_pubkey: String,
        /// The plaintext to encrypt.
        plaintext: String,
        /// `None` = active account; `Some(hex)` = a named roster key.
        signer_pubkey: Option<String>,
        /// Invoked with the resolved ciphertext (or an error string).
        continuation: CipherContinuation,
    },
    /// Backend-transparent NIP-44 DECRYPT-account port — the inbound twin of
    /// [`Self::Nip44EncryptForAccount`] (ADR-0050 §D1). Same contract; decrypts
    /// `ciphertext` from `peer_pubkey` to plaintext.
    Nip44DecryptForAccount {
        /// Sender pubkey (lowercase hex) the ciphertext was encrypted from.
        peer_pubkey: String,
        /// The ciphertext to decrypt.
        ciphertext: String,
        /// `None` = active account; `Some(hex)` = a named roster key.
        signer_pubkey: Option<String>,
        /// Invoked with the resolved plaintext (or an error string).
        continuation: CipherContinuation,
    },
}