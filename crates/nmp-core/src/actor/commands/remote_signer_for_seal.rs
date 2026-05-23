//! `RemoteSignerForSeal` — adapter from `Arc<dyn RemoteSignerHandle>` to
//! `nmp_nip59::SignerForSeal`. Closes ADR-0026 Phase 2.
//!
//! The adapter lets a NIP-46 bunker (or NIP-07 / hardware signer) drive the
//! NIP-59 gift-wrap seal step for a user whose secret key lives outside the
//! kernel. Without it, `IdentityRuntime::active_signer_for_seal` returned
//! `None` for remote accounts and `commands/dm.rs` surfaced a toast —
//! bunker users could not send NIP-17 DMs.
//!
//! # Translation responsibilities
//!
//! - [`pubkey`]: parse `RemoteSignerHandle::pubkey_hex()` once at
//!   construction. A malformed pubkey is treated as "no usable signer" —
//!   [`new`] returns `None` and `active_signer_for_seal` surfaces the
//!   pre-existing graceful-degrade signal rather than panicking on first
//!   use.
//! - [`nip44_encrypt`]: forwards directly — `RemoteSignerHandle::
//!   nip44_encrypt` already returns `SignerOp<String>` with the right
//!   shape.
//! - [`sign_seal`]: bridges `nostr::UnsignedEvent` → substrate
//!   `UnsignedEvent` (forward) and substrate `SignedEvent` →
//!   `nostr::Event` (return) over the inner `RemoteSignerHandle::sign`
//!   call. The inner op is awaited synchronously up to
//!   [`ADAPTER_SIGN_TIMEOUT`] and the adapter always returns
//!   `SignerOp::Ready` (see "Why always Ready" below).
//!
//! # Why `sign_seal` always returns `Ready`
//!
//! `nmp_nip59::gift_wrap_with_signer` has two code paths driven by the
//! shape of `SignerForSeal::nip44_encrypt`:
//!
//! - **Synchronous fast path** (encrypt is `Ready`): runs the whole chain
//!   on the calling thread. If `sign_seal` returns `Pending`, the function
//!   surfaces a "contract violation" error — `Pending` is only legal when
//!   the driver thread is in play.
//! - **Driver path** (encrypt is `Pending`): spawns a per-invocation
//!   thread that walks the chain and supports a `Pending` `sign_seal`
//!   with `recv_timeout(DRIVER_STEP_TIMEOUT)`.
//!
//! Because the adapter does not know which path will call it, the only
//! shape that is safe in both is `Ready`. The cost is that `sign_seal`
//! blocks for up to `ADAPTER_SIGN_TIMEOUT` while waiting on the inner
//! `RemoteSignerHandle::sign`. In practice every real bunker signer has
//! `Pending` `nip44_encrypt` too, so `sign_seal` is called from the
//! driver thread — the block happens there, never on the actor thread.
//!
//! # Timeout budget
//!
//! The gift-wrap chain has two sequential bunker RPCs (encrypt + sign).
//! Each step is bounded by `nmp_nip59::DRIVER_STEP_TIMEOUT` (5s). The
//! `commands/dm.rs` consumer waits up to
//! `nmp_nip59::GIFT_WRAP_TOTAL_TIMEOUT` (12s) on the outer `SignerOp`,
//! which covers both steps plus the in-process wrap assembly. For
//! local-key accounts every op is `Ready`, so the wait is non-blocking.

use std::sync::Arc;
use std::time::Duration;

use nmp_nip59::SignerForSeal;
use nmp_signer_iface::{SignerError, SignerOp};
use nostr::{Event, JsonUtil, PublicKey, UnsignedEvent as NostrUnsignedEvent};

use crate::remote_signer::RemoteSignerHandle;
use crate::substrate::UnsignedEvent as SubstrateUnsignedEvent;

/// Per-RPC budget for the inner `RemoteSignerHandle::sign` call inside
/// [`RemoteSignerForSeal::sign_seal`]. Mirrors
/// `nmp_nip59::DRIVER_STEP_TIMEOUT` (5s) — the consumer-facing total
/// budget is `nmp_nip59::GIFT_WRAP_TOTAL_TIMEOUT`.
const ADAPTER_SIGN_TIMEOUT: Duration = nmp_nip59::DRIVER_STEP_TIMEOUT;

/// Adapter wrapping an `Arc<dyn RemoteSignerHandle>` so the NIP-59
/// gift-wrap driver can seal events on its behalf.
#[derive(Debug)]
pub(crate) struct RemoteSignerForSeal {
    handle: Arc<dyn RemoteSignerHandle>,
    pubkey: PublicKey,
}

impl RemoteSignerForSeal {
    /// Construct an adapter. Returns `None` when `handle.pubkey_hex()` is
    /// not a valid hex pubkey — keeps `active_signer_for_seal`'s
    /// graceful-degrade signal (`None`) intact rather than panicking on
    /// the first call.
    pub(crate) fn new(handle: Arc<dyn RemoteSignerHandle>) -> Option<Self> {
        let pubkey = PublicKey::parse(&handle.pubkey_hex()).ok()?;
        Some(Self { handle, pubkey })
    }
}

impl SignerForSeal for RemoteSignerForSeal {
    fn pubkey(&self) -> PublicKey {
        self.pubkey
    }

    fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
        // `RemoteSignerHandle::nip44_encrypt` already produces the shape
        // `SignerForSeal::nip44_encrypt` requires — forward directly.
        self.handle.nip44_encrypt(recipient_pubkey, plaintext)
    }

    fn sign_seal(&self, unsigned: &NostrUnsignedEvent) -> SignerOp<Event> {
        // nostr → substrate. The substrate `UnsignedEvent` is the wire
        // shape every `RemoteSignerHandle::sign` impl consumes.
        let substrate_unsigned = SubstrateUnsignedEvent {
            pubkey: unsigned.pubkey.to_hex(),
            kind: unsigned.kind.as_u16() as u32,
            tags: unsigned
                .tags
                .iter()
                .map(|t| t.as_slice().to_vec())
                .collect(),
            content: unsigned.content.clone(),
            created_at: unsigned.created_at.as_secs(),
        };

        // Block on the inner sign op. See module docs — `sign_seal` must
        // return `Ready` (else `gift_wrap_with_signer`'s sync path errors
        // as a contract violation), so we wait synchronously rather than
        // chain a `SignerOp<SignedEvent>` into a `SignerOp<Event>`.
        let signed = match self
            .handle
            .sign(&substrate_unsigned)
            .wait(ADAPTER_SIGN_TIMEOUT)
        {
            Ok(signed) => signed,
            Err(e) => {
                return SignerOp::err(SignerError::Backend(format!(
                    "remote signer sign_seal failed: {e}"
                )));
            }
        };

        // substrate → nostr::Event. Build the wire JSON and reparse —
        // `Event::from_json` validates the bunker's signature, so a
        // bunker that returns a malformed signed event is caught here
        // rather than at the relay.
        let json = serde_json::json!({
            "id": signed.id,
            "pubkey": signed.unsigned.pubkey,
            "created_at": signed.unsigned.created_at,
            "kind": signed.unsigned.kind,
            "tags": signed.unsigned.tags,
            "content": signed.unsigned.content,
            "sig": signed.sig,
        });
        match Event::from_json(json.to_string()) {
            Ok(event) => SignerOp::ok(event),
            Err(e) => SignerOp::err(SignerError::Backend(format!(
                "remote signer returned malformed signed event: {e}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::substrate::SignedEvent as SubstrateSignedEvent;
    use nmp_nip59::gift_wrap_with_signer;
    use nostr::nips::nip44 as nostr_nip44;
    use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
    use nostr::{EventBuilder, Keys, Kind, SecretKey, Tag, Timestamp};
    use std::sync::atomic::{AtomicU32, Ordering};

    /// In-test stub: an in-memory `RemoteSignerHandle` backed by real
    /// `nostr::Keys`. Mirrors the production `StubRemoteSigner` in
    /// `remote_signer_tests` but kept local so the adapter's tests don't
    /// require cross-module visibility changes.
    #[derive(Debug)]
    struct TestKeysHandle {
        keys: Keys,
        pk: String,
        sign_count: AtomicU32,
    }

    impl TestKeysHandle {
        fn from_keys(keys: Keys) -> Self {
            let pk = keys.public_key().to_hex();
            Self {
                keys,
                pk,
                sign_count: AtomicU32::new(0),
            }
        }
    }

    impl RemoteSignerHandle for TestKeysHandle {
        fn pubkey_hex(&self) -> String {
            self.pk.clone()
        }
        fn signer_kind(&self) -> &'static str {
            "test"
        }
        fn sign(&self, unsigned: &SubstrateUnsignedEvent) -> SignerOp<SubstrateSignedEvent> {
            self.sign_count.fetch_add(1, Ordering::Relaxed);
            let kind = Kind::from_u16(unsigned.kind as u16);
            let tags = unsigned
                .tags
                .iter()
                .filter_map(|t| Tag::parse(t).ok())
                .collect::<Vec<_>>();
            let event = EventBuilder::new(kind, &unsigned.content)
                .tags(tags)
                .custom_created_at(Timestamp::from(unsigned.created_at))
                .sign_with_keys(&self.keys)
                .expect("test signer can build a kind:13 seal");
            SignerOp::ok(SubstrateSignedEvent {
                id: event.id.to_hex(),
                sig: event.sig.to_string(),
                unsigned: SubstrateUnsignedEvent {
                    pubkey: event.pubkey.to_hex(),
                    kind: event.kind.as_u16() as u32,
                    tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                    content: event.content.clone(),
                    created_at: event.created_at.as_secs(),
                },
            })
        }
        fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String> {
            let receiver = match PublicKey::parse(recipient_pubkey) {
                Ok(pk) => pk,
                Err(e) => {
                    return SignerOp::err(SignerError::Backend(format!(
                        "test: invalid recipient: {e}"
                    )))
                }
            };
            SignerOp::Ready(
                nostr_nip44::encrypt(
                    self.keys.secret_key(),
                    &receiver,
                    plaintext,
                    nostr_nip44::Version::V2,
                )
                .map_err(|e| SignerError::Backend(format!("test nip44 encrypt: {e}"))),
            )
        }
        fn nip44_decrypt(&self, _: &str, _: &str) -> SignerOp<String> {
            SignerOp::err(SignerError::Backend("unused in tests".to_string()))
        }
        fn deliver_rpc_response(&self, _: &str) {}
    }

    /// End-to-end gift-wrap through the adapter: the seal step routes
    /// through `RemoteSignerHandle`; the receiver can decrypt the rumor
    /// back out.
    #[test]
    fn gift_wrap_round_trips_through_remote_signer_adapter() {
        let alice_keys = Keys::new(SecretKey::generate());
        let alice_pk = alice_keys.public_key();
        let bob_keys = Keys::new(SecretKey::generate());
        let bob_pk = bob_keys.public_key();

        let handle: Arc<dyn RemoteSignerHandle> =
            Arc::new(TestKeysHandle::from_keys(alice_keys.clone()));
        let adapter =
            RemoteSignerForSeal::new(Arc::clone(&handle)).expect("stub has a valid pubkey");
        let signer: Arc<dyn SignerForSeal> = Arc::new(adapter);

        // Rumor: a kind:14 chat message from Alice to Bob.
        let rumor = EventBuilder::new(Kind::from_u16(14), "hello bob")
            .tag(nostr::Tag::public_key(bob_pk))
            .build(alice_pk);

        let op = gift_wrap_with_signer(
            &signer,
            &bob_pk,
            &rumor,
            Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK),
        );
        let envelope = op
            .wait(nmp_nip59::GIFT_WRAP_TOTAL_TIMEOUT)
            .expect("gift-wrap completes within total budget");

        assert_eq!(envelope.kind, Kind::GiftWrap);
        // Outer pubkey must be ephemeral, NOT Alice's — the unlinkability
        // invariant (NIP-59 § 1).
        assert_ne!(envelope.pubkey, alice_pk);
        // Bob (the receiver) unwraps the envelope back to Alice's rumor
        // via the crate-local sync helper (the upstream `extract_rumor`
        // is async; `nmp_nip59::unwrap_gift_wrap` is the kernel-side
        // sync entry point).
        let unwrapped = nmp_nip59::unwrap_gift_wrap(&bob_keys, &envelope)
            .expect("receiver decrypts the gift-wrap envelope");
        assert_eq!(unwrapped.sender, alice_pk);
        assert_eq!(unwrapped.rumor.content, "hello bob");
        assert_eq!(unwrapped.rumor.kind, Kind::from_u16(14));
    }

    /// A bunker whose `pubkey_hex()` is malformed must NOT panic the
    /// adapter — `new` returns `None` so `active_signer_for_seal`
    /// surfaces the same graceful-degrade signal.
    #[test]
    fn malformed_pubkey_yields_none_from_new() {
        #[derive(Debug)]
        struct BadPubkeySigner;
        impl RemoteSignerHandle for BadPubkeySigner {
            fn pubkey_hex(&self) -> String {
                "not-hex".to_string()
            }
            fn signer_kind(&self) -> &'static str {
                "test"
            }
            fn sign(&self, _: &SubstrateUnsignedEvent) -> SignerOp<SubstrateSignedEvent> {
                SignerOp::err(SignerError::Backend("unused".to_string()))
            }
            fn nip44_encrypt(&self, _: &str, _: &str) -> SignerOp<String> {
                SignerOp::err(SignerError::Backend("unused".to_string()))
            }
            fn nip44_decrypt(&self, _: &str, _: &str) -> SignerOp<String> {
                SignerOp::err(SignerError::Backend("unused".to_string()))
            }
            fn deliver_rpc_response(&self, _: &str) {}
        }
        let handle: Arc<dyn RemoteSignerHandle> = Arc::new(BadPubkeySigner);
        assert!(RemoteSignerForSeal::new(handle).is_none());
    }

    /// When the inner signer returns an error, the adapter surfaces it as
    /// a `SignerError::Backend` rather than wedging or panicking.
    #[test]
    fn inner_sign_failure_propagates_as_backend_error() {
        #[derive(Debug)]
        struct FailingSigner {
            pk: String,
        }
        impl RemoteSignerHandle for FailingSigner {
            fn pubkey_hex(&self) -> String {
                self.pk.clone()
            }
            fn signer_kind(&self) -> &'static str {
                "test"
            }
            fn sign(&self, _: &SubstrateUnsignedEvent) -> SignerOp<SubstrateSignedEvent> {
                SignerOp::err(SignerError::Backend("bunker rejected".to_string()))
            }
            fn nip44_encrypt(&self, _: &str, _: &str) -> SignerOp<String> {
                SignerOp::ok("ct".to_string())
            }
            fn nip44_decrypt(&self, _: &str, _: &str) -> SignerOp<String> {
                SignerOp::err(SignerError::Backend("unused".to_string()))
            }
            fn deliver_rpc_response(&self, _: &str) {}
        }
        let pk = Keys::generate().public_key().to_hex();
        let handle: Arc<dyn RemoteSignerHandle> = Arc::new(FailingSigner { pk });
        let adapter = RemoteSignerForSeal::new(handle).expect("valid pubkey");

        let seal_unsigned = EventBuilder::new(Kind::Seal, "ciphertext-placeholder")
            .custom_created_at(Timestamp::now())
            .build(adapter.pubkey());

        match adapter.sign_seal(&seal_unsigned) {
            SignerOp::Ready(Err(SignerError::Backend(msg))) => {
                assert!(
                    msg.contains("bunker rejected"),
                    "expected backend error, got {msg}"
                );
            }
            other => panic!("expected Ready(Err(Backend)), got {other:?}"),
        }
    }
}
