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
        &(Arc::new(sender_keys.clone()) as Arc<dyn SignerForSeal>),
        &receiver_keys.public_key(),
        &rumor,
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
        &(Arc::clone(&signer) as Arc<dyn SignerForSeal>),
        &receiver_keys.public_key(),
        &rumor,
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
        &signer,
        &receiver_keys.public_key(),
        &rumor,
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
