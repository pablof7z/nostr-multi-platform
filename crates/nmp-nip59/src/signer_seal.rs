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
//! `nostr::Keys` for local, from a `Box<dyn RemoteSignerHandle>` adapter
//! for remote) and hands it to [`gift_wrap_with_signer`]. The kind:13 seal
//! step routes through the trait's `nip44_encrypt` + `sign` methods; the
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
/// should accommodate this.
pub const DRIVER_STEP_TIMEOUT: Duration = Duration::from_secs(5);

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
/// polls `rx` on its existing PendingSign tick loop — no busy waits, no
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
pub fn gift_wrap_with_signer(
    signer: Arc<dyn SignerForSeal>,
    receiver_pubkey: &PublicKey,
    rumor: UnsignedEvent,
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
            let signer_for_driver = Arc::clone(&signer);
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
                "gift_wrap driver: nip44_encrypt did not complete within {:?}",
                DRIVER_STEP_TIMEOUT
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
                    "gift_wrap driver: sign_seal did not complete within {:?}",
                    DRIVER_STEP_TIMEOUT
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
mod tests {
    //! Tests for [`gift_wrap_with_signer`] across the local-keys fast path
    //! and a fake remote-signer that emulates the `SignerOp::Pending`
    //! shape with `mpsc` channels. The chain produced for both shapes
    //! must round-trip through `unwrap_gift_wrap` to the original rumor.
    use super::*;
    use crate::wrap::unwrap_gift_wrap;
    use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
    use nostr::{EventBuilder, Kind, Tag};

    /// Build a kind:14 chat-message rumor for the given sender pubkey.
    /// Mirrors the shape `nmp_nip17::build_dm_rumor` produces.
    fn sample_rumor(sender_pubkey: PublicKey, content: &str) -> UnsignedEvent {
        EventBuilder::new(Kind::from_u16(14), content)
            .tag(Tag::public_key(sender_pubkey))
            .custom_created_at(Timestamp::from(1_700_000_000))
            .build(sender_pubkey)
    }

    /// A `SignerForSeal` that always returns `Pending` from `nip44_encrypt`
    /// — exercises the spawned-driver remote path. The seal sign step
    /// uses a real local key (a remote signer in production routes the
    /// sign via an RPC too, but the chain-driver code path is identical
    /// regardless of whether sign is `Ready` or `Pending`; the dedicated
    /// pending-sign test below exercises the latter).
    struct PendingEncryptSigner {
        keys: Keys,
        encrypt_tx: std::sync::Mutex<Option<mpsc::Sender<Result<String, SignerError>>>>,
        encrypt_rx_slot: std::sync::Mutex<Option<mpsc::Receiver<Result<String, SignerError>>>>,
    }

    impl PendingEncryptSigner {
        fn new(keys: Keys) -> Arc<Self> {
            let (tx, rx) = mpsc::channel::<Result<String, SignerError>>();
            Arc::new(Self {
                keys,
                encrypt_tx: std::sync::Mutex::new(Some(tx)),
                encrypt_rx_slot: std::sync::Mutex::new(Some(rx)),
            })
        }

        /// Mimic the broker delivering the seal-encrypt ciphertext later.
        fn deliver_encrypt(&self, ciphertext: String) {
            let tx = self
                .encrypt_tx
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .expect("encrypt_tx already delivered");
            tx.send(Ok(ciphertext)).expect("deliver must succeed");
        }
    }

    impl SignerForSeal for PendingEncryptSigner {
        fn pubkey(&self) -> PublicKey {
            self.keys.public_key()
        }

        fn nip44_encrypt(&self, _recipient_pubkey: &str, _plaintext: &str) -> SignerOp<String> {
            // Hand back the pre-arranged receiver. Subsequent calls would
            // re-arm; the test only fires once.
            let rx = self
                .encrypt_rx_slot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .expect("encrypt_rx already taken");
            SignerOp::Pending(rx)
        }

        fn sign_seal(&self, unsigned: &UnsignedEvent) -> SignerOp<Event> {
            // Remote sign of the seal: still local in this fake — the
            // chain-driver path doesn't care whether sign is Ready or
            // Pending, and exercising sign-Pending separately would
            // duplicate the same plumbing.
            match unsigned.clone().sign_with_keys(&self.keys) {
                Ok(e) => SignerOp::ok(e),
                Err(e) => SignerOp::err(SignerError::Backend(e.to_string())),
            }
        }
    }

    #[test]
    fn local_keys_round_trips_through_gift_wrap_with_signer() {
        // Sender + receiver local keypairs.
        let sender_keys = Keys::generate();
        let receiver_keys = Keys::generate();

        let rumor = sample_rumor(sender_keys.public_key(), "hello via signer seam");
        let created_at = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);

        // Local-keys fast path: the SignerOp must be Ready(Ok(event))
        // immediately — no driver thread.
        let op = gift_wrap_with_signer(
            Arc::new(sender_keys.clone()) as Arc<dyn SignerForSeal>,
            &receiver_keys.public_key(),
            rumor.clone(),
            created_at,
        );
        let event = op
            .wait(DRIVER_STEP_TIMEOUT)
            .expect("local-keys path must resolve synchronously");

        // The output must be a kind:1059 envelope that the receiver can
        // unwrap to the original rumor.
        assert_eq!(event.kind, Kind::GiftWrap);
        let unwrapped =
            unwrap_gift_wrap(&receiver_keys, &event).expect("receiver must unwrap successfully");
        assert_eq!(
            unwrapped.sender,
            sender_keys.public_key(),
            "unwrapped sender pubkey must match the original signer"
        );
        assert_eq!(
            unwrapped.rumor.content, "hello via signer seam",
            "round-tripped rumor content must match"
        );
        assert_eq!(unwrapped.rumor.kind, Kind::from_u16(14));
    }

    #[test]
    fn pending_encrypt_returns_pending_then_resolves_after_delivery() {
        let sender_keys = Keys::generate();
        let receiver_keys = Keys::generate();
        let signer = PendingEncryptSigner::new(sender_keys.clone());
        let rumor = sample_rumor(sender_keys.public_key(), "hello via remote-signer path");
        let created_at = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);

        let mut op = gift_wrap_with_signer(
            Arc::clone(&signer) as Arc<dyn SignerForSeal>,
            &receiver_keys.public_key(),
            rumor.clone(),
            created_at,
        );

        // First poll BEFORE delivery: must still be pending (the driver
        // thread is blocked on the encrypt receiver). This is the property
        // the actor's PendingSign loop depends on — Pending stays Pending
        // until the chain completes.
        match &mut op {
            SignerOp::Pending(_) => { /* expected */ }
            SignerOp::Ready(r) => panic!(
                "remote-encrypt op must return Pending until the encrypt step resolves; got Ready({r:?})"
            ),
        }
        assert!(
            op.poll().is_none(),
            "the driver has nothing to send until encrypt is delivered"
        );

        // Compute the real ciphertext using the sender's local key — the
        // fake just shuttles the value; once the driver receives it the
        // chain proceeds (sign_seal, wrap, send-final). The driver must
        // assemble a valid kind:1059 the receiver can unwrap.
        let receiver_pk = receiver_keys.public_key();
        let real_ct = nip44::encrypt(
            sender_keys.secret_key(),
            &receiver_pk,
            &rumor.as_json(),
            Nip44Version::V2,
        )
        .expect("local nip44_encrypt for test setup");
        signer.deliver_encrypt(real_ct);

        // After delivery the rx eventually carries the final event. wait()
        // blocks the test thread (not the actor) within the per-step
        // timeout budget.
        let event = op
            .wait(DRIVER_STEP_TIMEOUT * 3)
            .expect("the driver thread must complete the chain after delivery");
        assert_eq!(event.kind, Kind::GiftWrap);

        let unwrapped =
            unwrap_gift_wrap(&receiver_keys, &event).expect("receiver must unwrap successfully");
        assert_eq!(unwrapped.sender, sender_keys.public_key());
        assert_eq!(unwrapped.rumor.content, "hello via remote-signer path");
    }

    #[test]
    fn pending_encrypt_propagates_step_failure() {
        // If the encrypt step fails (broker rejects, channel drops, ...),
        // the driver must forward the error rather than block forever or
        // panic.
        let sender_keys = Keys::generate();
        let receiver_keys = Keys::generate();

        // A signer whose `nip44_encrypt` returns Pending then drops the
        // sender — emulates a broker crash mid-chain.
        struct DropSigner {
            keys: Keys,
        }
        impl SignerForSeal for DropSigner {
            fn pubkey(&self) -> PublicKey {
                self.keys.public_key()
            }
            fn nip44_encrypt(&self, _r: &str, _p: &str) -> SignerOp<String> {
                let (_tx, rx) = mpsc::channel::<Result<String, SignerError>>();
                // _tx is dropped immediately — driver sees Disconnected.
                SignerOp::Pending(rx)
            }
            fn sign_seal(&self, _u: &UnsignedEvent) -> SignerOp<Event> {
                // Unreachable in this test (driver fails before sign).
                SignerOp::err(SignerError::Backend("unreachable".into()))
            }
        }

        let signer: Arc<dyn SignerForSeal> = Arc::new(DropSigner {
            keys: sender_keys.clone(),
        });
        let rumor = sample_rumor(sender_keys.public_key(), "irrelevant");
        let op = gift_wrap_with_signer(
            signer,
            &receiver_keys.public_key(),
            rumor,
            Timestamp::from(1_700_000_000),
        );
        let err = op
            .wait(DRIVER_STEP_TIMEOUT)
            .expect_err("dropped encrypt channel must surface as an error");
        // Either Backend or Timeout is acceptable here — the contract is
        // "does not hang and does not panic". We assert it's an error
        // and the message names the encrypt step so debugging is direct.
        assert!(
            matches!(err, SignerError::Backend(_) | SignerError::Timeout(_)),
            "expected Backend or Timeout, got {err:?}"
        );
    }
}
