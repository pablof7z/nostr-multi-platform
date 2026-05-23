//! `SignerForSeal` — the ADR-0026 sealing seam for NIP-59 gift-wrap.
//!
//! # Motivation
//!
//! `nmp_nip59::gift_wrap` is a local-keys primitive: it requires the caller
//! to hold the sender's raw `nostr::Keys` and performs the kind:13 seal
//! step (a NIP-44 ECDH encryption from the sender to the receiver) in
//! process. Remote signers (NIP-46 bunker, NIP-07 browser extension,
//! hardware wallets) hold the user's secret key OUT of process — there is
//! no way to hand them a `&Keys` reference.
//!
//! ADR-0026 introduces a single seam over both shapes: this trait. A
//! caller that wants to gift-wrap a rumor produces a `SignerForSeal` (from
//! `nostr::Keys` for local, from an `Arc<dyn RemoteSignerHandle>` adapter
//! for remote — see `nmp-core::actor::commands::remote_signer_for_seal`)
//! and hands it to [`gift_wrap_with_signer`]. The kind:13 seal step
//! routes through the trait's `nip44_encrypt` + `sign` methods; the
//! kind:1059 outer wrap is always local (an ephemeral key minted in
//! process — the unlinkability guarantee).
//!
//! # `SignerOp` and the actor loop
//!
//! Each trait method returns a [`SignerOp<T>`][SignerOp]. For an in-memory
//! local signer the op is `Ready` and resolves synchronously. For a
//! remote signer the op is `Pending(rx)` carrying a `std::sync::mpsc::
//! Receiver` that the actor can poll on its existing
//! `try_recv`-style loop (D8 — no polling sleep loops, no `tokio`).
//!
//! [`gift_wrap_with_signer`] returns a `SignerOp<Event>` that mirrors the
//! per-step shape: `Ready(Ok(event))` on the synchronous fast path,
//! `Pending(rx)` on the remote path, with a small driver thread inside
//! the function forwarding the multi-step chain (`nip44_encrypt` →
//! `sign_seal` → assemble wrap) to the rx. The driver thread is
//! per-invocation and short-lived; it does NOT busy-poll (D8) — each
//! step blocks on the underlying signer-op `recv_timeout`.
//!
//! [SignerOp]: nmp_signer_iface::SignerOp

use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use nmp_signer_iface::{SignerError, SignerOp};
use nostr::{
    nips::nip44::{self, Version as Nip44Version},
    Event, EventBuilder, JsonUtil, Keys, Kind, PublicKey, SecretKey, Timestamp, UnsignedEvent,
};

use crate::error::Nip59Error;

/// Wall-clock budget for each per-step wait inside the remote-signer
/// driver thread. Mirrors `nmp-core`'s `PENDING_SIGN_TIMEOUT` (5s) — long
/// enough for a fast / auto-approving bunker, short enough that a stuck
/// broker cannot strand the gift-wrap chain indefinitely.
///
/// Total bound on a remote chain: `2 × DRIVER_STEP_TIMEOUT` (one `nip44_
/// encrypt`, one `sign`). The caller's outer `PendingSign` deadline
/// should accommodate this — see [`GIFT_WRAP_TOTAL_TIMEOUT`].
pub const DRIVER_STEP_TIMEOUT: Duration = Duration::from_secs(5);

/// End-to-end budget for [`gift_wrap_with_signer`] on the remote-signer
/// path: covers two sequential bunker RPCs ([`DRIVER_STEP_TIMEOUT`] each
/// for `nip44_encrypt` + `sign_seal`) plus the in-process wrap assembly
/// and channel hand-off, with 2s of headroom.
///
/// Callers waiting on the returned `SignerOp` MUST use this (not
/// [`DRIVER_STEP_TIMEOUT`]) as their `wait` budget — otherwise a slow
/// bunker that responds within the per-step bound still misses the
/// caller's window, surfacing a misleading "timed out" toast for a send
/// that actually succeeded mid-chain.
pub const GIFT_WRAP_TOTAL_TIMEOUT: Duration = Duration::from_secs(12);

/// Abstract signer surface for the NIP-59 seal step.
///
/// The two production implementors are `nostr::Keys` (synchronous local-
/// keys path) and an `Arc<dyn RemoteSignerHandle>` adapter (NIP-46 / NIP-07
/// / hardware — pending RPC path). Both run through the same
/// [`gift_wrap_with_signer`] call site; the implementor's `SignerOp`
/// shape (`Ready` vs `Pending`) is what determines whether the driver
/// thread is spawned.
///
/// `Send + Sync + 'static` is required so the trait object can be moved
/// into the driver thread for the remote case. The local `Keys` blanket
/// impl trivially satisfies this; the remote adapter is the only other
/// place this matters (handled in `nmp-core`).
pub trait SignerForSeal: Send + Sync + 'static {
    /// The signer's public key. Used to set the kind:13 seal's pubkey.
    fn pubkey(&self) -> PublicKey;

    /// NIP-44 encrypt `plaintext` to `recipient_pubkey`. The seal step's
    /// payload is `nip44_encrypt(sender, receiver, rumor.as_json())`.
    ///
    /// `recipient_pubkey` is lowercase hex (matches
    /// `RemoteSignerHandle::nip44_encrypt`'s shape).
    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String>;

    /// Sign the kind:13 seal envelope. The implementor receives an
    /// `UnsignedEvent` whose `content` is the already-encrypted seal
    /// payload (NIP-44 ciphertext produced by [`Self::nip44_encrypt`])
    /// and whose `created_at` carries a NIP-59 random tweak — and
    /// returns the fully signed kind:13 `Event`.
    fn sign_seal(&self, unsigned: &UnsignedEvent) -> SignerOp<Event>;
}

/// Local-keys blanket impl. Both `nip44_encrypt` and `sign_seal` resolve
/// synchronously — every `SignerOp` is `Ready`.
impl SignerForSeal for Keys {
    fn pubkey(&self) -> PublicKey {
        self.public_key()
    }

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
        let receiver = match PublicKey::parse(recipient_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                return SignerOp::err(SignerError::Backend(format!(
                    "nip44_encrypt: malformed recipient pubkey: {e}"
                )));
            }
        };
        match nip44::encrypt(self.secret_key(), &receiver, plaintext, Nip44Version::V2) {
            Ok(ct) => SignerOp::ok(ct),
            Err(e) => SignerOp::err(SignerError::Backend(format!("nip44_encrypt: {e}"))),
        }
    }

    fn sign_seal(&self, unsigned: &UnsignedEvent) -> SignerOp<Event> {
        // The local-keys sign path is in-process and infallible barring
        // a secp256k1 backend failure. `sign_with_keys` is the synchronous
        // entry point on the `nostr` builder; we wrap any error as a
        // `SignerError::Backend` so the seal step's failure mode is
        // uniform with the remote case.
        match unsigned.clone().sign_with_keys(self) {
            Ok(event) => SignerOp::ok(event),
            Err(e) => SignerOp::err(SignerError::Backend(format!("sign_seal: {e}"))),
        }
    }
}

/// Build the kind:13 seal `UnsignedEvent` from the already-encrypted
/// payload. Mirrors `nostr::nips::nip59::make_seal` but takes the
/// ciphertext as input (instead of re-encrypting) so the function works
/// regardless of which signer path produced the ciphertext.
fn build_seal_unsigned(
    sender_pubkey: PublicKey,
    encrypted_content: String,
    created_at: Timestamp,
) -> UnsignedEvent {
    EventBuilder::new(Kind::Seal, encrypted_content)
        .custom_created_at(created_at)
        .build(sender_pubkey)
}

/// Locally wrap the signed kind:13 seal in a kind:1059 envelope using a
/// freshly-minted ephemeral key. Always runs in-process — the ephemeral
/// key never leaves this function (the unlinkability guarantee per
/// NIP-59 §1).
///
/// The wrap timestamp re-uses `created_at` rather than re-tweaking; this
/// matches the seal/wrap pair the `nostr` 0.44 `gift_wrap` helper
/// produces today, where the same tweaked timestamp is applied to both
/// envelopes (`make_seal` tweaks; `EventBuilder::gift_wrap` re-uses).
fn wrap_signed_seal(
    receiver: &PublicKey,
    seal_event: &Event,
    created_at: Timestamp,
) -> Result<Event, Nip59Error> {
    // Mint a fresh ephemeral keypair for the outer wrap. NEVER reused —
    // the unlinkability property depends on every kind:1059 envelope
    // carrying a distinct outer pubkey.
    let ephemeral_sk = SecretKey::generate();
    let ephemeral = Keys::new(ephemeral_sk);

    // Encrypt the seal JSON to the receiver, using the EPHEMERAL secret
    // (NOT the sender's). This is what gives the outer envelope its
    // "anyone could have sent this" property.
    let seal_json = seal_event.as_json();
    let outer_content = nip44::encrypt(
        ephemeral.secret_key(),
        receiver,
        &seal_json,
        Nip44Version::V2,
    )
    .map_err(|e| Nip59Error::Nostr(format!("outer wrap nip44_encrypt: {e}")))?;

    // Build + sign the kind:1059 envelope with the ephemeral key.
    let event = EventBuilder::new(Kind::GiftWrap, outer_content)
        .custom_created_at(created_at)
        .tag(nostr::Tag::public_key(*receiver))
        .sign_with_keys(&ephemeral)
        .map_err(|e| Nip59Error::Nostr(format!("outer wrap sign: {e}")))?;
    Ok(event)
}

/// Seal (kind:13) + gift-wrap (kind:1059) a rumor via an abstract signer.
///
/// # Local-keys fast path
///
/// When the signer is a `Keys` (every `SignerOp` is `Ready`), the function
/// runs the full chain synchronously and returns
/// `SignerOp::Ready(Ok(event))`. No thread is spawned.
///
/// # Remote-signer path
///
/// When the signer's first op (`nip44_encrypt` for the seal) is
/// `Pending`, the function spawns a short-lived driver thread that:
/// 1. Blocks on the `nip44_encrypt` receiver (up to [`DRIVER_STEP_TIMEOUT`])
/// 2. Builds the kind:13 seal `UnsignedEvent` from the ciphertext
/// 3. Issues `signer.sign_seal(&seal_unsigned)` and blocks on it (up to
///    another [`DRIVER_STEP_TIMEOUT`])
/// 4. Locally wraps + signs the kind:1059 envelope with a fresh ephemeral
///    key (in-process — no signer round-trip)
/// 5. Sends the final `Event` (or error) on the returned channel.
///
/// The function returns `SignerOp::Pending(rx)` immediately. The actor
/// polls `rx` on its existing `PendingSign` tick loop — no busy waits, no
/// blocking the actor thread, no parallel state cluster.
///
/// # Constraints
///
/// - `signer` must be `Send + Sync + 'static` (the trait bound). The
///   driver thread takes ownership of an `Arc` clone for the remote
///   path; the local path uses the value directly.
/// - `receiver_pubkey` is the recipient of the gift-wrap. The same
///   helper is called once per receiver — NIP-17 sends two envelopes
///   (recipient + self-copy) by calling this function twice, each with
///   a freshly-minted ephemeral key.
/// - `created_at` is the timestamp stamped on BOTH the seal and the
///   wrap. Callers passing a NIP-59-tweaked timestamp (per
///   [`nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK`]) get the
///   privacy property the spec intends.
#[must_use]
pub fn gift_wrap_with_signer(
    signer: &Arc<dyn SignerForSeal>,
    receiver_pubkey: &PublicKey,
    rumor: &UnsignedEvent,
    created_at: Timestamp,
) -> SignerOp<Event> {
    // Stage 1 — seal-content encrypt.
    let sender_pubkey = signer.pubkey();
    let receiver_hex = receiver_pubkey.to_hex();
    let rumor_json = rumor.as_json();
    let encrypt_op = signer.nip44_encrypt(&receiver_hex, &rumor_json);

    match encrypt_op {
        SignerOp::Ready(Ok(ciphertext)) => {
            // Local-keys fast path: continue synchronously.
            let seal_unsigned = build_seal_unsigned(sender_pubkey, ciphertext, created_at);
            let sign_op = signer.sign_seal(&seal_unsigned);
            let seal_event = match sign_op {
                SignerOp::Ready(Ok(e)) => e,
                SignerOp::Ready(Err(e)) => {
                    return SignerOp::err(SignerError::Backend(format!(
                        "gift_wrap_with_signer: sign_seal failed on sync path: {e}"
                    )));
                }
                SignerOp::Pending(_) => {
                    // A local-keys impl must never return Pending — that's
                    // a contract violation, not a recoverable case. Bridge
                    // to a backend error so the caller surfaces it.
                    return SignerOp::err(SignerError::Backend(
                        "gift_wrap_with_signer: sign_seal returned Pending on sync path \
                         (SignerForSeal impl contract violation)"
                            .to_string(),
                    ));
                }
            };
            match wrap_signed_seal(receiver_pubkey, &seal_event, created_at) {
                Ok(event) => SignerOp::ok(event),
                Err(e) => SignerOp::err(SignerError::Backend(format!(
                    "gift_wrap_with_signer: outer wrap failed: {e}"
                ))),
            }
        }
        SignerOp::Ready(Err(e)) => {
            // Synchronous failure on the encrypt step — surface as-is.
            SignerOp::err(SignerError::Backend(format!(
                "gift_wrap_with_signer: nip44_encrypt failed on sync path: {e}"
            )))
        }
        SignerOp::Pending(encrypt_rx) => {
            // Remote-signer path. Spawn a driver thread to walk the
            // chain; return Pending(rx) immediately so the actor never
            // blocks.
            let (tx, rx) = mpsc::channel::<Result<Event, SignerError>>();
            let receiver_clone = *receiver_pubkey;
            let created_at_clone = created_at;
            let signer_for_driver = Arc::clone(signer);
            let spawn_result = thread::Builder::new()
                .name("nmp-nip59-gift-wrap-driver".to_string())
                .spawn(move || {
                    let result = drive_remote_chain(
                        signer_for_driver,
                        sender_pubkey,
                        receiver_clone,
                        created_at_clone,
                        encrypt_rx,
                    );
                    // Best-effort: receiver may have been dropped (caller
                    // gave up). A failed send is therefore not a panic.
                    let _ = tx.send(result);
                });
            if let Err(e) = spawn_result {
                // OS thread spawn failed (exhausted thread budget, ulimit
                // hit). Convert to a `Backend` error on the same rx the
                // caller is polling — never panic at this seam (D6).
                return SignerOp::err(SignerError::Backend(format!(
                    "gift_wrap_with_signer: OS thread spawn for remote driver \
                     failed: {e}"
                )));
            }
            SignerOp::Pending(rx)
        }
    }
}

/// Driver loop for the remote-signer chain. Runs on a spawned thread —
/// blocking calls inside here do NOT block the actor.
///
/// Steps: wait on the `nip44_encrypt` rx → build seal `UnsignedEvent` →
/// call `sign_seal` → wait on the sign rx → wrap with fresh ephemeral
/// → return.
#[allow(clippy::needless_pass_by_value)] // Arc by value: thread spawn needs ownership; Timestamp: Copy but clear in context
fn drive_remote_chain(
    signer: Arc<dyn SignerForSeal>,
    sender_pubkey: PublicKey,
    receiver: PublicKey,
    created_at: Timestamp,
    encrypt_rx: mpsc::Receiver<Result<String, SignerError>>,
) -> Result<Event, SignerError> {
    // Step 1: wait for the seal ciphertext.
    let ciphertext = match encrypt_rx.recv_timeout(DRIVER_STEP_TIMEOUT) {
        Ok(Ok(ct)) => ct,
        Ok(Err(e)) => {
            return Err(SignerError::Backend(format!(
                "gift_wrap driver: nip44_encrypt failed: {e}"
            )));
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(SignerError::Timeout(format!(
                "gift_wrap driver: nip44_encrypt did not complete within {DRIVER_STEP_TIMEOUT:?}"
            )));
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(SignerError::Backend(
                "gift_wrap driver: nip44_encrypt channel disconnected before completion"
                    .to_string(),
            ));
        }
    };

    // Step 2: build the seal `UnsignedEvent` and dispatch sign.
    let seal_unsigned = build_seal_unsigned(sender_pubkey, ciphertext, created_at);
    let sign_op = signer.sign_seal(&seal_unsigned);

    let seal_event = match sign_op {
        SignerOp::Ready(Ok(e)) => e,
        SignerOp::Ready(Err(e)) => {
            return Err(SignerError::Backend(format!(
                "gift_wrap driver: sign_seal failed (ready): {e}"
            )));
        }
        SignerOp::Pending(sign_rx) => match sign_rx.recv_timeout(DRIVER_STEP_TIMEOUT) {
            Ok(Ok(e)) => e,
            Ok(Err(e)) => {
                return Err(SignerError::Backend(format!(
                    "gift_wrap driver: sign_seal failed (pending): {e}"
                )));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(SignerError::Timeout(format!(
                    "gift_wrap driver: sign_seal did not complete within {DRIVER_STEP_TIMEOUT:?}"
                )));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(SignerError::Backend(
                    "gift_wrap driver: sign_seal channel disconnected before completion"
                        .to_string(),
                ));
            }
        },
    };

    // Step 3: wrap with a fresh ephemeral key (in-process — no signer
    // round-trip; the ephemeral key never leaves this function).
    wrap_signed_seal(&receiver, &seal_event, created_at).map_err(|e| {
        SignerError::Backend(format!("gift_wrap driver: outer wrap failed: {e}"))
    })
}

#[cfg(test)]
#[path = "signer_seal/tests.rs"]
mod tests;
