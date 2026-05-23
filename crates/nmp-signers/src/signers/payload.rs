//! Serializable signer payloads.
//!
//! Every [`Signer`](super::Signer) round-trips through a `SignerPayload`.  The
//! payload is a discriminated union so storage adapters (Keychain, LMDB, web
//! `IndexedDB`) treat all signer kinds uniformly: store JSON, parse on restore,
//! call `Signer::from_payload(payload)`.
//!
//! The applesauce / NDK pattern is mirrored: NIP-07 payloads carry no secret
//! (the extension is the secret); NIP-46 payloads carry the connection
//! material needed to re-handshake (local key + remote pubkey + relays);
//! local payloads carry the key material in either raw or NIP-49 form.

use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// Serializable representation of a signer.
///
/// `Debug` is hand-written (not derived) so that a `{:?}` log or panic
/// message never leaks raw key material. Each variant delegates to the
/// inner payload's redacting `Debug` impl.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "body", rename_all = "snake_case")]
pub enum SignerPayload {
    /// Local secret-key signer.
    Local(LocalPayload),
    /// NIP-46 bunker:// remote signer.
    Nip46(Nip46Payload),
    /// NIP-07 browser extension.  Carries no secret — re-acquisition prompts
    /// the user via `window.nostr.getPublicKey()` on restore.
    Nip07(Nip07Payload),
}

/// Local-key signer persistence form.
///
/// `Debug` is hand-written to avoid leaking the contained key material.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalPayload {
    /// One of:
    /// - `Raw(hex)` — raw secret hex.  Use only when storage is already secure
    ///   (e.g. an OS keychain/keystore item value).
    /// - `Ncryptsec(s)` — NIP-49-encrypted; requires password to decrypt.
    pub key: LocalKeyMaterial,
}

/// Raw-or-encrypted local key material.
///
/// `Debug` is hand-written to redact the secret — `Raw` carries a raw
/// private-key hex and even `Ncryptsec` is encrypted-but-sensitive.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", content = "value", rename_all = "snake_case")]
pub enum LocalKeyMaterial {
    /// Raw secret key, hex-encoded.  Caller-storage must be trusted.
    ///
    /// Wrapped in [`Zeroizing`] so the secret hex is wiped from the heap when
    /// the payload is dropped — a freed `String` would otherwise leave the key
    /// recoverable in a memory dump or crash report. `Zeroizing<String>`
    /// serializes transparently (inner value, no wrapper) via the `zeroize`
    /// `serde` feature, so the on-disk JSON form is unchanged.
    Raw(Zeroizing<String>),
    /// NIP-49 ncryptsec string (`ncryptsec1...`).
    Ncryptsec(String),
}

/// NIP-46 bunker payload — everything needed to re-handshake with the remote.
///
/// `Debug` is hand-written: `local_secret_hex` is a raw ephemeral private
/// key and `secret` is a connection token — neither must reach a log.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip46Payload {
    /// Ephemeral local key, hex.  Used for encrypting to / signing the
    /// 24133 RPCs sent to the remote.  Never used to sign user events.
    ///
    /// Wrapped in [`Zeroizing`] — this is plaintext secret key material in the
    /// same security class as `LocalKeyMaterial::Raw`, so the hex is wiped from
    /// the heap when the payload is dropped. `Zeroizing<String>` serializes
    /// transparently (inner value, no wrapper) via the `zeroize` `serde`
    /// feature, so the on-disk JSON form is unchanged.
    pub local_secret_hex: Zeroizing<String>,
    /// Remote (bunker) pubkey, hex.
    pub remote_pubkey_hex: String,
    /// Relays the remote rendezvous on (kind:24133).
    pub relays: Vec<String>,
    /// Optional connection secret (from `?secret=...` in the bunker URI).
    ///
    /// Wrapped in [`Zeroizing`] — a connection credential is sensitive and is
    /// wiped from the heap on drop. Serializes transparently via the `zeroize`
    /// `serde` feature, so the on-disk JSON form is unchanged.
    pub secret: Option<Zeroizing<String>>,
    /// Permissions string passed in `?perms=...` (CSV of `sign_event:<kind>`).
    pub permissions: Option<String>,
    /// Cached remote user pubkey.  Set after first successful handshake; lets
    /// us produce `pubkey()` synchronously on restore without a round-trip.
    pub cached_remote_user_pubkey_hex: Option<String>,
}

/// NIP-07 payload — empty modulo the discriminator.  The extension itself is
/// the secret, so the payload carries nothing sensitive and may derive
/// `Debug` directly.
#[derive(Clone, Debug, Eq, PartialEq, Default, Serialize, Deserialize)]
pub struct Nip07Payload {
    /// Optional cached pubkey to avoid an immediate `getPublicKey()` round-trip
    /// on restore (applesauce caches this — `0867a502`).
    pub cached_pubkey_hex: Option<String>,
}

// ---------------- Redacting `Debug` impls ----------------
//
// Secret-bearing payloads do NOT derive `Debug`; these hand-written impls
// guarantee that no `{:?}` log line, panic message, or error chain can leak
// raw key material.

impl fmt::Debug for LocalKeyMaterial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Raw(_) => f.write_str("LocalKeyMaterial::Raw([redacted])"),
            Self::Ncryptsec(_) => f.write_str("LocalKeyMaterial::Ncryptsec([redacted])"),
        }
    }
}

impl fmt::Debug for LocalPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalPayload")
            .field("key", &self.key)
            .finish()
    }
}

impl fmt::Debug for Nip46Payload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Non-secret fields are shown verbatim; `local_secret_hex` and
        // `secret` are redacted.
        f.debug_struct("Nip46Payload")
            .field("local_secret_hex", &"[redacted]")
            .field("remote_pubkey_hex", &self.remote_pubkey_hex)
            .field("relays", &self.relays)
            .field("secret", &self.secret.as_ref().map(|_| "[redacted]"))
            .field("permissions", &self.permissions)
            .field(
                "cached_remote_user_pubkey_hex",
                &self.cached_remote_user_pubkey_hex,
            )
            .finish()
    }
}

impl fmt::Debug for SignerPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local(p) => f.debug_tuple("SignerPayload::Local").field(p).finish(),
            Self::Nip46(p) => f.debug_tuple("SignerPayload::Nip46").field(p).finish(),
            Self::Nip07(p) => f.debug_tuple("SignerPayload::Nip07").field(p).finish(),
        }
    }
}
