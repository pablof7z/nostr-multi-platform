//! Serializable signer payloads.
//!
//! Every [`Signer`](super::Signer) round-trips through a `SignerPayload`.  The
//! payload is a discriminated union so storage adapters (Keychain, LMDB, web
//! IndexedDB) treat all signer kinds uniformly: store JSON, parse on restore,
//! call `Signer::from_payload(payload)`.
//!
//! The applesauce / NDK pattern is mirrored: NIP-07 payloads carry no secret
//! (the extension is the secret); NIP-46 payloads carry the connection
//! material needed to re-handshake (local key + remote pubkey + relays);
//! local payloads carry the key material in either raw or NIP-49 form.

use serde::{Deserialize, Serialize};

/// Serializable representation of a signer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocalPayload {
    /// One of:
    /// - `Raw(hex)` — raw secret hex.  Use only when storage is already secure
    ///   (e.g. iOS Keychain item value).
    /// - `Ncryptsec(s)` — NIP-49-encrypted; requires password to decrypt.
    pub key: LocalKeyMaterial,
}

/// Raw-or-encrypted local key material.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "form", content = "value", rename_all = "snake_case")]
pub enum LocalKeyMaterial {
    /// Raw secret key, hex-encoded.  Caller-storage must be trusted.
    Raw(String),
    /// NIP-49 ncryptsec string (`ncryptsec1...`).
    Ncryptsec(String),
}

/// NIP-46 bunker payload — everything needed to re-handshake with the remote.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip46Payload {
    /// Ephemeral local key, hex.  Used for encrypting to / signing the
    /// 24133 RPCs sent to the remote.  Never used to sign user events.
    pub local_secret_hex: String,
    /// Remote (bunker) pubkey, hex.
    pub remote_pubkey_hex: String,
    /// Relays the remote rendezvous on (kind:24133).
    pub relays: Vec<String>,
    /// Optional connection secret (from `?secret=...` in the bunker URI).
    pub secret: Option<String>,
    /// Permissions string passed in `?perms=...` (CSV of `sign_event:<kind>`).
    pub permissions: Option<String>,
    /// Cached remote user pubkey.  Set after first successful handshake; lets
    /// us produce `pubkey()` synchronously on restore without a round-trip.
    pub cached_remote_user_pubkey_hex: Option<String>,
}

/// NIP-07 payload — empty modulo the discriminator.  The extension itself is
/// the secret.
#[derive(Clone, Debug, Eq, PartialEq, Default, Serialize, Deserialize)]
pub struct Nip07Payload {
    /// Optional cached pubkey to avoid an immediate `getPublicKey()` round-trip
    /// on restore (applesauce caches this — `0867a502`).
    pub cached_pubkey_hex: Option<String>,
}
