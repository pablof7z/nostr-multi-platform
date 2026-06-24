//! `LocalKeySigner` — in-memory secret key signer with optional NIP-49
//! encryption at rest.
//!
//! Mirrors applesauce `PrivateKeySigner` (38 LOC reference) and NDK
//! `NDKPrivateKeySigner`: holds the raw secret bytes (zeroizing) plus the
//! cached `PublicKey`, and reconstructs a transient `nostr::SecretKey` for the
//! duration of each individual crypto operation.

use nmp_signer_iface::{SignedEvent, SignerError, UnsignedEvent};
use nostr::nips::{nip04, nip44};
use nostr::{EventBuilder, Keys, Kind, PublicKey, SecretKey, Tag, Timestamp};
use zeroize::{Zeroize, Zeroizing};

use super::payload::{LocalKeyMaterial, LocalPayload, SignerPayload};
use super::traits::{Nip04, Nip44, Signer, SignerBackend};
use super::SignerOp;

/// In-memory secret key signer.
///
/// Construct via [`LocalKeySigner::generate`], [`LocalKeySigner::from_secret_hex`],
/// [`LocalKeySigner::from_nsec`], or [`LocalKeySigner::from_ncryptsec`].
///
/// ## Secret-key residency (V-55 / issue #971)
///
/// This signer deliberately does **not** retain a long-lived `nostr::Keys` (or
/// any other long-lived secret-key value).  Instead it stores only the raw 32
/// secret bytes inside a [`Zeroizing`] buffer (wiped on `Drop`) plus the cached
/// public key.  Every crypto operation reconstructs a transient
/// [`nostr::SecretKey`] from those bytes via [`LocalKeySigner::with_secret_key`]
/// and lets it drop at the end of the operation — `nostr::SecretKey::drop`
/// calls `secp256k1::SecretKey::non_secure_erase`, so the heap copy created for
/// the operation is erased before the call returns.
///
/// This collapses the window during which a recoverable copy of the secret
/// exists in `secp256k1`-owned memory from *signer lifetime* down to *a single
/// in-stack operation*.  One irreducible remainder remains: while a transient
/// secret key (or transient [`nostr::Keys`] built for signing) is alive on the
/// stack for the duration of an op, `secp256k1` may keep its own internal
/// copies; `nostr` 0.44 does not implement `Zeroize`/`ZeroizeOnDrop` on `Keys`,
/// only `non_secure_erase`-on-drop on `SecretKey`.  Narrowing this to the
/// per-operation window is the best achievable without upstream support;
/// adding `ZeroizeOnDrop` to `nostr::Keys` is filed upstream as
/// rust-nostr/nostr#1378 (tracked here as V-55 / issue #971).
pub struct LocalKeySigner {
    /// Cached public key — derived once at construction, never secret.
    pubkey: PublicKey,
    /// If the signer was constructed from an ncryptsec, retain the password so
    /// `to_payload()` can re-encrypt to the same form (round-trip).  None for
    /// raw-constructed signers; callers can re-supply via
    /// [`LocalKeySigner::with_password`].
    password: Option<String>,
    /// NIP-49 `log_n` parameter — default 16, lowered for tests via
    /// [`LocalKeySigner::with_ncryptsec_log_n`].
    ncryptsec_log_n: u8,
    /// The *only* long-lived copy of the secret: the raw 32 key bytes wrapped in
    /// [`Zeroizing`] so they are wiped from the heap on `Drop`.  No
    /// `nostr::Keys` / `secp256k1::SecretKey` value is retained across
    /// operations — see the type-level doc for the full V-55 / issue #971
    /// rationale.  `[u8; 32]` implements `Zeroize` natively.
    secret: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for LocalKeySigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never expose the secret key.
        f.debug_struct("LocalKeySigner")
            .field("pubkey", &self.pubkey.to_hex())
            .field("encrypted_at_rest", &self.password.is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for LocalKeySigner {
    /// Zero the Rust-owned secret copies on drop so freed heap memory does not
    /// retain key material (recoverable via memory dumps / crash reports).
    ///
    /// We explicitly zero the plaintext `password` copy here; the `secret`
    /// field (the only long-lived secret copy) is wiped automatically when its
    /// `Zeroizing` wrapper drops (field drops run after this `Drop` body).
    ///
    /// No `nostr::Keys` / `secp256k1::SecretKey` is retained by this struct, so
    /// there is no unreachable, un-erased secp256k1-owned secret copy living
    /// for the signer's lifetime — transient keys are erased per operation (see
    /// the type-level doc).
    fn drop(&mut self) {
        if let Some(ref mut pw) = self.password {
            pw.zeroize();
        }
    }
}

impl LocalKeySigner {
    /// Generate a fresh keypair via OS RNG.
    #[must_use]
    pub fn generate() -> Self {
        Self::from_secret_key(SecretKey::generate())
    }

    /// Construct from a 64-char hex secret.
    #[must_use]
    pub fn from_secret_hex(hex: &str) -> Result<Self, SignerError> {
        let sk = SecretKey::from_hex(hex)
            .map_err(|e| SignerError::Backend(format!("invalid hex secret: {e}")))?;
        Ok(Self::from_secret_key(sk))
    }

    /// Construct from an `nsec1...` bech32 string.
    #[must_use]
    pub fn from_nsec(nsec: &str) -> Result<Self, SignerError> {
        use nostr::nips::nip19::FromBech32;
        let sk = SecretKey::from_bech32(nsec)
            .map_err(|e| SignerError::Backend(format!("invalid nsec: {e}")))?;
        Ok(Self::from_secret_key(sk))
    }

    /// Construct from an `ncryptsec1...` (NIP-49) string + password.
    #[must_use]
    pub fn from_ncryptsec(ncryptsec: &str, password: &str) -> Result<Self, SignerError> {
        use nostr::nips::nip19::FromBech32;
        use nostr::nips::nip49::EncryptedSecretKey;
        let enc = EncryptedSecretKey::from_bech32(ncryptsec)
            .map_err(|e| SignerError::Backend(format!("invalid ncryptsec: {e}")))?;
        let sk = enc
            .decrypt(password)
            .map_err(|e| SignerError::Rejected(format!("ncryptsec decrypt failed: {e}")))?;
        let mut signer = Self::from_secret_key(sk);
        signer.password = Some(password.to_string());
        Ok(signer)
    }

    /// Restore from a `LocalPayload` produced by [`Signer::to_payload`].
    #[must_use]
    pub fn from_payload(p: &LocalPayload) -> Result<Self, SignerError> {
        Self::from_payload_with_password(p, None)
    }

    /// Restore from a payload, supplying a password for `Ncryptsec` form.
    pub fn from_payload_with_password(
        p: &LocalPayload,
        password: Option<&str>,
    ) -> Result<Self, SignerError> {
        match &p.key {
            LocalKeyMaterial::Raw(hex) => Self::from_secret_hex(hex),
            LocalKeyMaterial::Ncryptsec(s) => {
                let pwd = password.ok_or_else(|| {
                    SignerError::NotReady("ncryptsec requires password to unlock".to_string())
                })?;
                Self::from_ncryptsec(s, pwd)
            }
        }
    }

    /// Set / clear the password used by `to_payload()` to NIP-49-encrypt.
    #[must_use]
    pub fn with_password(mut self, password: Option<String>) -> Self {
        self.password = password;
        self
    }

    /// NIP-49 `log_n` parameter used by `to_payload()`.  Default 16
    /// (~65k scrypt iterations — production-grade but slow: 1-3 seconds on a
    /// laptop).  Lower values (e.g. 8) are appropriate for tests and CI to
    /// keep the build fast; never go below 14 for real user keys.
    #[must_use]
    pub fn with_ncryptsec_log_n(mut self, log_n: u8) -> Self {
        self.ncryptsec_log_n = log_n;
        self
    }

    /// Access the underlying secret as hex (for export flows that explicitly
    /// want the raw value; callers should warn the user).
    ///
    /// Returns a [`Zeroizing<String>`] so the exported copy is wiped from the
    /// heap when the caller drops it — a plain `String` return would leave the
    /// secret recoverable in freed memory.
    pub fn secret_hex(&self) -> Zeroizing<String> {
        self.with_secret_key(|sk| Zeroizing::new(sk.to_secret_hex()))
    }

    fn from_secret_key(sk: SecretKey) -> Self {
        // Capture the raw bytes into a `Zeroizing` buffer, derive the public
        // key, then let `sk` drop (which `non_secure_erase`s its secp256k1
        // copy).  No `Keys` / `SecretKey` is retained past this constructor —
        // the bytes are the sole long-lived secret copy.
        let secret: Zeroizing<[u8; 32]> = Zeroizing::new(sk.to_secret_bytes());
        let pubkey = Keys::new(sk).public_key();
        Self {
            pubkey,
            password: None,
            ncryptsec_log_n: 16,
            secret,
        }
    }

    /// Reconstruct a transient [`nostr::SecretKey`] from the stored bytes, run
    /// `f` with a borrow of it, then drop the transient key.  `nostr::SecretKey`
    /// erases its secp256k1 copy on drop (`non_secure_erase`), so the secret
    /// copy created for this call is wiped before the borrow window closes.
    ///
    /// This is the single choke-point through which every crypto operation
    /// touches the secret, keeping the secp256k1-residency window bounded to one
    /// operation (V-55 / issue #971).
    fn with_secret_key<R>(&self, f: impl FnOnce(&SecretKey) -> R) -> R {
        // `from_slice` on 32 valid bytes round-tripped from a real secret key is
        // infallible; treat a failure as a logic bug, not an operational error.
        let sk = SecretKey::from_slice(self.secret.as_slice())
            .expect("stored 32 secret bytes always form a valid SecretKey"); // doctrine-allow: D6 — bytes were produced by `to_secret_bytes()` on an already-validated `SecretKey`; reconstruction is infallible and a failure here is a logic bug, not an operational error
        f(&sk)
        // `sk` drops here → `non_secure_erase` wipes the transient secp256k1 copy.
    }

    fn sign_now(&self, unsigned: &UnsignedEvent) -> Result<SignedEvent, SignerError> {
        let kind_u16 = u16::try_from(unsigned.kind).map_err(|_| SignerError::KindOutOfRange {
            kind: unsigned.kind,
        })?;
        let kind = Kind::from_u16(kind_u16);
        // Hard-fail on any malformed tag rather than silently dropping it.
        // A dropped tag would produce a signed event that differs from the
        // caller's intent — the actor's `sign_with` enforces the same
        // post-condition (D6 — correctness hazard for kind-agnostic publish).
        let tags = unsigned
            .tags
            .iter()
            .map(Tag::parse)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| SignerError::Backend(format!("malformed tag: {e}")))?;
        let builder = EventBuilder::new(kind, &unsigned.content)
            .tags(tags)
            .custom_created_at(Timestamp::from(unsigned.created_at));
        // Build a transient `Keys` from the stored bytes, sign, then let it
        // drop — its embedded `SecretKey` erases itself on drop.
        let event = self
            .with_secret_key(|sk| builder.sign_with_keys(&Keys::new(sk.clone())))
            .map_err(|e| SignerError::Backend(format!("sign failed: {e}")))?;
        if event.pubkey != self.pubkey {
            return Err(SignerError::Mismatch(format!(
                "signed event pubkey {} != signer pubkey {}",
                event.pubkey.to_hex(),
                self.pubkey.to_hex()
            )));
        }
        Ok(SignedEvent {
            id: event.id.to_hex(),
            sig: event.sig.to_string(),
            unsigned: UnsignedEvent {
                pubkey: event.pubkey.to_hex(),
                kind: u32::from(event.kind.as_u16()),
                tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
                content: event.content.clone(),
                created_at: event.created_at.as_secs(),
            },
        })
    }
}

impl Signer for LocalKeySigner {
    fn backend(&self) -> SignerBackend {
        SignerBackend::LocalKey
    }

    fn pubkey(&self) -> PublicKey {
        self.pubkey
    }

    fn sign(&self, unsigned: UnsignedEvent) -> SignerOp<SignedEvent> {
        SignerOp::Ready(self.sign_now(&unsigned))
    }

    fn nip04(&self) -> Option<&dyn Nip04> {
        Some(self)
    }

    fn nip44(&self) -> Option<&dyn Nip44> {
        Some(self)
    }

    fn to_payload(&self) -> SignerPayload {
        let key = match &self.password {
            Some(pwd) => {
                use nostr::nips::nip19::ToBech32;
                use nostr::nips::nip49::{EncryptedSecretKey, KeySecurity};
                let enc = self
                    .with_secret_key(|sk| {
                        EncryptedSecretKey::new(sk, pwd, self.ncryptsec_log_n, KeySecurity::Medium)
                    })
                    .expect("NIP-49 encrypt with a valid key + password should not fail"); // doctrine-allow: D6 — `to_payload` (Signer trait) returns `SignerPayload`, not `Result`; the key is held + validated at construction. CAVEAT: scrypt at log_n=16 is theoretically OOM-reachable on memory-constrained devices — refactoring the trait to `-> Result<SignerPayload, SignerError>` is tracked as a follow-up
                let bech = enc
                    .to_bech32()
                    .expect("EncryptedSecretKey -> bech32 should not fail"); // doctrine-allow: D6 — bech32 encoding of an already-constructed `EncryptedSecretKey` is infallible (fixed HRP + valid payload); a failure here is a logic bug, not an operational error
                LocalKeyMaterial::Ncryptsec(bech)
            }
            None => {
                LocalKeyMaterial::Raw(self.with_secret_key(|sk| Zeroizing::new(sk.to_secret_hex())))
            }
        };
        SignerPayload::Local(LocalPayload { key })
    }
}

impl Nip04 for LocalKeySigner {
    fn encrypt(&self, recipient: &PublicKey, plaintext: &str) -> SignerOp<String> {
        SignerOp::Ready(
            self.with_secret_key(|sk| nip04::encrypt(sk, recipient, plaintext))
                .map_err(|e| SignerError::Backend(format!("nip04 encrypt: {e}"))),
        )
    }
    fn decrypt(&self, sender: &PublicKey, ciphertext: &str) -> SignerOp<String> {
        SignerOp::Ready(
            self.with_secret_key(|sk| nip04::decrypt(sk, sender, ciphertext))
                .map_err(|e| SignerError::Backend(format!("nip04 decrypt: {e}"))),
        )
    }
}

impl Nip44 for LocalKeySigner {
    fn encrypt(&self, recipient: &PublicKey, plaintext: &str) -> SignerOp<String> {
        SignerOp::Ready(
            self.with_secret_key(|sk| nip44::encrypt(sk, recipient, plaintext, nip44::Version::V2))
                .map_err(|e| SignerError::Backend(format!("nip44 encrypt: {e}"))),
        )
    }
    fn decrypt(&self, sender: &PublicKey, payload: &str) -> SignerOp<String> {
        SignerOp::Ready(
            self.with_secret_key(|sk| nip44::decrypt(sk, sender, payload))
                .map_err(|e| SignerError::Backend(format!("nip44 decrypt: {e}"))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unsigned_with_kind(kind: u32) -> UnsignedEvent {
        UnsignedEvent {
            pubkey: String::new(),
            kind,
            tags: Vec::new(),
            content: "hi".to_string(),
            created_at: 1_700_000_000,
        }
    }

    #[test]
    fn sign_now_rejects_kind_above_u16_max() {
        let signer = LocalKeySigner::generate();
        let err = signer
            .sign_now(&unsigned_with_kind(70_000))
            .expect_err("kind above u16::MAX must not be silently coerced");
        match err {
            SignerError::KindOutOfRange { kind } => assert_eq!(kind, 70_000),
            other => panic!("expected KindOutOfRange, got {other:?}"),
        }
    }

    #[test]
    fn sign_now_accepts_in_range_kind() {
        let signer = LocalKeySigner::generate();
        let signed = signer
            .sign_now(&unsigned_with_kind(1))
            .expect("kind 1 must sign");
        assert_eq!(signed.unsigned.kind, 1);
    }

    /// V-55 / issue #971: the signer must NOT retain a long-lived `nostr::Keys`
    /// (which embeds a `secp256k1::SecretKey` + cached `Keypair` that NMP cannot
    /// erase).  The only long-lived secret copy must be the inline 32-byte
    /// `Zeroizing` buffer; transient keys are reconstructed per operation and
    /// erased on drop.
    ///
    /// This is enforced structurally via `size_of`: a `nostr::Keys` is far
    /// larger than 32 bytes (it holds a `OnceCell<Keypair>` ~96 bytes plus the
    /// `SecretKey`).  If a `keys: Keys` field were reintroduced the struct size
    /// would jump well past the byte-buffer-only footprint and this test would
    /// fail.
    #[test]
    fn signer_does_not_retain_a_long_lived_keys() {
        use std::mem::size_of;
        // Upper bound on the legitimate footprint: 32 secret bytes + pubkey
        // (32) + Option<String> (24) + u8 + padding.  A retained `nostr::Keys`
        // would add its own `SecretKey` (32) AND a `OnceCell<Keypair>` (which
        // alone exceeds this whole budget), so this bound cannot be met while
        // holding a `Keys`.
        const MAX_FOOTPRINT: usize = 32 + size_of::<PublicKey>() + size_of::<Option<String>>() + 8;
        assert!(
            size_of::<LocalKeySigner>() <= MAX_FOOTPRINT,
            "LocalKeySigner footprint {} exceeds {MAX_FOOTPRINT}; a long-lived \
             secret-key value (e.g. a `Keys` field) appears to have been \
             reintroduced — see V-55 / issue #971",
            size_of::<LocalKeySigner>(),
        );
        assert!(
            size_of::<LocalKeySigner>() < size_of::<Keys>() + 32,
            "LocalKeySigner ({}) should be far smaller than embedding a Keys ({})",
            size_of::<LocalKeySigner>(),
            size_of::<Keys>(),
        );
    }

    /// Every crypto path still works when the secret is reconstructed
    /// per-operation from the stored bytes (regression guard for the
    /// transient-key refactor).  nip04/nip44 round-trip + ncryptsec round-trip +
    /// secret_hex stability all exercise `with_secret_key`.
    #[test]
    fn transient_secret_reconstruction_preserves_all_ops() {
        let signer = LocalKeySigner::generate();
        let counterparty = LocalKeySigner::generate();

        // Stable across repeated reconstructions.
        assert_eq!(&*signer.secret_hex(), &*signer.secret_hex());

        // nip44 round-trip between the two signers.
        let ct = match Nip44::encrypt(&signer, &counterparty.pubkey(), "hello-44") {
            SignerOp::Ready(r) => r.expect("nip44 encrypt"),
            other => panic!("expected Ready, got {other:?}"),
        };
        let pt = match Nip44::decrypt(&counterparty, &signer.pubkey(), &ct) {
            SignerOp::Ready(r) => r.expect("nip44 decrypt"),
            other => panic!("expected Ready, got {other:?}"),
        };
        assert_eq!(pt, "hello-44");

        // nip04 round-trip.
        let ct = match Nip04::encrypt(&signer, &counterparty.pubkey(), "hello-04") {
            SignerOp::Ready(r) => r.expect("nip04 encrypt"),
            other => panic!("expected Ready, got {other:?}"),
        };
        let pt = match Nip04::decrypt(&counterparty, &signer.pubkey(), &ct) {
            SignerOp::Ready(r) => r.expect("nip04 decrypt"),
            other => panic!("expected Ready, got {other:?}"),
        };
        assert_eq!(pt, "hello-04");

        // ncryptsec round-trip via to_payload (fast log_n for the test).
        let hex = signer.secret_hex();
        let with_pwd = LocalKeySigner::from_secret_hex(&hex)
            .expect("from hex")
            .with_password(Some("hunter2".to_string()))
            .with_ncryptsec_log_n(8);
        let payload = with_pwd.to_payload();
        let SignerPayload::Local(lp) = payload else {
            panic!("expected local payload");
        };
        let restored = LocalKeySigner::from_payload_with_password(&lp, Some("hunter2"))
            .expect("ncryptsec round-trip");
        assert_eq!(restored.pubkey(), signer.pubkey());
        assert_eq!(&*restored.secret_hex(), &*signer.secret_hex());
    }
}
