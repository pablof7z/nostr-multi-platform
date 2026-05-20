//! `LocalKeySigner` — in-memory secret key signer with optional NIP-49
//! encryption at rest.
//!
//! Mirrors applesauce `PrivateKeySigner` (38 LOC reference) and NDK
//! `NDKPrivateKeySigner`: holds `SecretKey`, derives `PublicKey` lazily once
//! and caches it, signs via `nostr` crate primitives.

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nostr::nips::{nip04, nip44};
use nostr::{EventBuilder, Keys, Kind, PublicKey, SecretKey, Tag, Timestamp};
use zeroize::{Zeroize, Zeroizing};

use super::payload::{LocalKeyMaterial, LocalPayload, SignerPayload};
use super::traits::{Nip04, Nip44, Signer, SignerBackend, SignerError};
use super::SignerOp;

/// In-memory secret key signer.
///
/// Construct via [`LocalKeySigner::generate`], [`LocalKeySigner::from_secret_hex`],
/// [`LocalKeySigner::from_nsec`], or [`LocalKeySigner::from_ncryptsec`].
pub struct LocalKeySigner {
    keys: Keys,
    pubkey: PublicKey,
    /// If the signer was constructed from an ncryptsec, retain the password so
    /// `to_payload()` can re-encrypt to the same form (round-trip).  None for
    /// raw-constructed signers; callers can re-supply via
    /// [`LocalKeySigner::with_password`].
    password: Option<String>,
    /// NIP-49 `log_n` parameter — default 16, lowered for tests via
    /// [`LocalKeySigner::with_ncryptsec_log_n`].
    ncryptsec_log_n: u8,
    /// Redundant `Zeroizing` copy of the raw secret-key bytes, held purely so
    /// that *a* copy of the secret is reliably wiped from the heap on drop.
    ///
    /// PARTIAL MITIGATION — read carefully before changing this.  `nostr::Keys`
    /// keeps the secret in two places we cannot reach: its private
    /// `secret_key: secp256k1::SecretKey` field (exposed only as `&SecretKey`,
    /// no `&mut` accessor) and a cached `key_pair: OnceCell<Keypair>` (a
    /// `Keypair` also embeds the secret).  `secp256k1` 0.29 has no `zeroize`
    /// feature and `nostr` 0.44 implements neither `Zeroize` nor `Drop` on
    /// `Keys`, so those two copies are NOT wiped on drop.  This field gives us
    /// a third copy that *is* wiped (via the `Zeroizing` wrapper's `Drop`),
    /// reducing — but not eliminating — recoverable secret material in freed
    /// memory.  Full mitigation requires upstream `Zeroize` support in `nostr`
    /// (or a `nostr`/`secp256k1` upgrade that adds it).  Tracked in
    /// `docs/arch-review-queue.md`.
    ///
    /// Stored as an inline `[u8; 32]` (which `zeroize` natively implements
    /// `Zeroize` for); the bytes are wiped wherever the enclosing
    /// `LocalKeySigner` is allocated.
    _secret_bytes: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for LocalKeySigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never expose the secret key.
        f.debug_struct("LocalKeySigner")
            .field("pubkey", &self.pubkey.to_hex())
            .field("encrypted_at_rest", &self.password.is_some())
            .finish()
    }
}

impl Drop for LocalKeySigner {
    /// Zero the Rust-owned secret copies on drop so freed heap memory does not
    /// retain key material (recoverable via memory dumps / crash reports).
    ///
    /// `#[derive(ZeroizeOnDrop)]` is not applicable here: `nostr::Keys` does
    /// not implement `Zeroize`. `Keys` is an external type that manages its
    /// own secret memory.  We explicitly zero the plaintext `password` copy
    /// here; the `_secret_bytes` field is wiped automatically when its
    /// `Zeroizing` wrapper drops (field drops run after this `Drop` body).
    ///
    /// The secret copies *inside* `nostr::Keys` (its `secp256k1::SecretKey`
    /// and cached `Keypair`) are NOT reachable for zeroing — see the
    /// `_secret_bytes` field doc for the full honest scope of this mitigation.
    fn drop(&mut self) {
        if let Some(ref mut pw) = self.password {
            pw.zeroize();
        }
    }
}

impl LocalKeySigner {
    /// Generate a fresh keypair via OS RNG.
    pub fn generate() -> Self {
        Self::from_secret_key(SecretKey::generate())
    }

    /// Construct from a 64-char hex secret.
    pub fn from_secret_hex(hex: &str) -> Result<Self, SignerError> {
        let sk = SecretKey::from_hex(hex)
            .map_err(|e| SignerError::Backend(format!("invalid hex secret: {e}")))?;
        Ok(Self::from_secret_key(sk))
    }

    /// Construct from an `nsec1...` bech32 string.
    pub fn from_nsec(nsec: &str) -> Result<Self, SignerError> {
        use nostr::nips::nip19::FromBech32;
        let sk = SecretKey::from_bech32(nsec)
            .map_err(|e| SignerError::Backend(format!("invalid nsec: {e}")))?;
        Ok(Self::from_secret_key(sk))
    }

    /// Construct from an `ncryptsec1...` (NIP-49) string + password.
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
    pub fn with_password(mut self, password: Option<String>) -> Self {
        self.password = password;
        self
    }

    /// NIP-49 `log_n` parameter used by `to_payload()`.  Default 16
    /// (~65k scrypt iterations — production-grade but slow: 1-3 seconds on a
    /// laptop).  Lower values (e.g. 8) are appropriate for tests and CI to
    /// keep the build fast; never go below 14 for real user keys.
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
        Zeroizing::new(self.keys.secret_key().to_secret_hex())
    }

    fn from_secret_key(sk: SecretKey) -> Self {
        // Capture the raw bytes into a `Zeroizing` buffer before the
        // `SecretKey` is moved into `Keys`.  `to_secret_bytes()` returns an
        // owned `[u8; 32]`; the `Zeroizing` wrapper wipes it on drop.  See the
        // `_secret_bytes` field doc for why this is a partial — not complete —
        // mitigation.
        let secret_bytes: Zeroizing<[u8; 32]> = Zeroizing::new(sk.to_secret_bytes());
        let keys = Keys::new(sk);
        let pubkey = keys.public_key();
        Self {
            keys,
            pubkey,
            password: None,
            ncryptsec_log_n: 16,
            _secret_bytes: secret_bytes,
        }
    }

    fn sign_now(&self, unsigned: UnsignedEvent) -> Result<SignedEvent, SignerError> {
        let kind = Kind::from_u16(unsigned.kind as u16);
        // Hard-fail on any malformed tag rather than silently dropping it.
        // A dropped tag would produce a signed event that differs from the
        // caller's intent — the actor's `sign_with` enforces the same
        // post-condition (D6 — correctness hazard for kind-agnostic publish).
        let tags = unsigned
            .tags
            .iter()
            .map(|t| Tag::parse(t))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| SignerError::Backend(format!("malformed tag: {e}")))?;
        let builder = EventBuilder::new(kind, &unsigned.content)
            .tags(tags)
            .custom_created_at(Timestamp::from(unsigned.created_at));
        let event = builder
            .sign_with_keys(&self.keys)
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
                kind: event.kind.as_u16() as u32,
                tags: event
                    .tags
                    .iter()
                    .map(|t| t.as_slice().to_vec())
                    .collect(),
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
        SignerOp::Ready(self.sign_now(unsigned))
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
                let enc = EncryptedSecretKey::new(
                    self.keys.secret_key(),
                    pwd,
                    self.ncryptsec_log_n,
                    KeySecurity::Medium,
                )
                .expect("NIP-49 encrypt with a valid key + password should not fail"); // doctrine-allow: D6 — `to_payload` (Signer trait) returns `SignerPayload`, not `Result`; the key is held + validated at construction. CAVEAT: scrypt at log_n=16 is theoretically OOM-reachable on memory-constrained devices — refactoring the trait to `-> Result<SignerPayload, SignerError>` is tracked as a follow-up
                let bech = enc
                    .to_bech32()
                    .expect("EncryptedSecretKey -> bech32 should not fail"); // doctrine-allow: D6 — bech32 encoding of an already-constructed `EncryptedSecretKey` is infallible (fixed HRP + valid payload); a failure here is a logic bug, not an operational error
                LocalKeyMaterial::Ncryptsec(bech)
            }
            None => {
                LocalKeyMaterial::Raw(Zeroizing::new(self.keys.secret_key().to_secret_hex()))
            }
        };
        SignerPayload::Local(LocalPayload { key })
    }
}

impl Nip04 for LocalKeySigner {
    fn encrypt(&self, recipient: &PublicKey, plaintext: &str) -> SignerOp<String> {
        SignerOp::Ready(
            nip04::encrypt(self.keys.secret_key(), recipient, plaintext)
                .map_err(|e| SignerError::Backend(format!("nip04 encrypt: {e}"))),
        )
    }
    fn decrypt(&self, sender: &PublicKey, ciphertext: &str) -> SignerOp<String> {
        SignerOp::Ready(
            nip04::decrypt(self.keys.secret_key(), sender, ciphertext)
                .map_err(|e| SignerError::Backend(format!("nip04 decrypt: {e}"))),
        )
    }
}

impl Nip44 for LocalKeySigner {
    fn encrypt(&self, recipient: &PublicKey, plaintext: &str) -> SignerOp<String> {
        SignerOp::Ready(
            nip44::encrypt(
                self.keys.secret_key(),
                recipient,
                plaintext,
                nip44::Version::V2,
            )
            .map_err(|e| SignerError::Backend(format!("nip44 encrypt: {e}"))),
        )
    }
    fn decrypt(&self, sender: &PublicKey, payload: &str) -> SignerOp<String> {
        SignerOp::Ready(
            nip44::decrypt(self.keys.secret_key(), sender, payload)
                .map_err(|e| SignerError::Backend(format!("nip44 decrypt: {e}"))),
        )
    }
}

