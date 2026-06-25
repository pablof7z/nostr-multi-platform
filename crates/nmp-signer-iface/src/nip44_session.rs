//! Optional NMP NIP-46 extension for scoped NIP-44 decrypt sessions.
//!
//! These are signer capability types, not DM-domain types.  The `scope`
//! string is opaque to this crate so the kernel can use the same cryptographic
//! seam for any future replay/backfill use without teaching the signer layer
//! app vocabulary.

use std::fmt;

use serde::{Deserialize, Serialize};

/// NIP-46 method name for beginning a scoped NIP-44 decrypt session.
pub const NMP_NIP44_DECRYPT_SESSION_BEGIN: &str = "nmp_nip44_decrypt_session_begin";

/// NIP-46 method name for decrypting a batch inside a scoped session.
pub const NMP_NIP44_DECRYPT_BATCH: &str = "nmp_nip44_decrypt_batch";

/// NIP-46 method name for ending a scoped NIP-44 decrypt session.
pub const NMP_NIP44_DECRYPT_SESSION_END: &str = "nmp_nip44_decrypt_session_end";

/// Scope string reserved by NMP for private-message backfill decrypt sessions.
pub const NMP_NIP44_BACKFILL_SCOPE: &str = "nmp.nip44.backfill";

/// Current version of the optional NMP decrypt-session extension.
pub const NMP_NIP44_DECRYPT_SESSION_EXTENSION_VERSION: u16 = 1;

/// Persisted, non-secret fact that a NIP-46 signer supports the NMP
/// decrypt-session extension.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip44DecryptSessionExtension {
    /// Extension version negotiated with the signer.
    pub version: u16,
}

impl Default for Nip44DecryptSessionExtension {
    fn default() -> Self {
        Self {
            version: NMP_NIP44_DECRYPT_SESSION_EXTENSION_VERSION,
        }
    }
}

/// Request payload for `nmp_nip44_decrypt_session_begin`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip44DecryptSessionBeginRequest {
    /// Opaque capability scope, for example [`NMP_NIP44_BACKFILL_SCOPE`].
    pub scope: String,
    /// Account pubkey requesting the scoped decrypt grant.
    pub requester_pubkey: String,
    /// Maximum total items the kernel expects to decrypt under the grant.
    pub max_items: usize,
    /// Kernel-chosen grant expiration as Unix seconds.
    pub expires_at: u64,
}

/// Successful result for `nmp_nip44_decrypt_session_begin`.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip44DecryptSessionGrant {
    /// Opaque signer-owned session token.  Treat as secret-bearing.
    pub session_id: String,
    /// Signer-advertised maximum number of items per batch call.
    pub max_batch_items: usize,
    /// Signer-confirmed expiration as Unix seconds.
    pub expires_at: u64,
}

impl fmt::Debug for Nip44DecryptSessionGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Nip44DecryptSessionGrant")
            .field("session_id", &"[redacted]")
            .field("max_batch_items", &self.max_batch_items)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// Request payload for `nmp_nip44_decrypt_batch`.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip44DecryptBatchRequest {
    /// Opaque signer-owned session token.  Treat as secret-bearing.
    pub session_id: String,
    /// Ciphertext items to decrypt inside the session.
    pub items: Vec<Nip44DecryptBatchItem>,
}

impl fmt::Debug for Nip44DecryptBatchRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Nip44DecryptBatchRequest")
            .field("session_id", &"[redacted]")
            .field("items_len", &self.items.len())
            .finish()
    }
}

/// One NIP-44 ciphertext item inside a batch decrypt request.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip44DecryptBatchItem {
    /// Caller-chosen correlation id, echoed in the batch result.
    pub id: String,
    /// Peer pubkey hex used for the NIP-44 conversation.
    pub peer_pubkey: String,
    /// NIP-44 ciphertext payload.  Treat as secret-bearing.
    pub ciphertext: String,
}

impl fmt::Debug for Nip44DecryptBatchItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Nip44DecryptBatchItem")
            .field("id", &self.id)
            .field("peer_pubkey", &self.peer_pubkey)
            .field("ciphertext", &"[redacted]")
            .finish()
    }
}

/// Result payload for `nmp_nip44_decrypt_batch`.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip44DecryptBatchResult {
    /// Per-item decrypt outcomes.  The item order should match the request,
    /// but callers must correlate by id rather than trusting position alone.
    pub items: Vec<Nip44DecryptBatchItemResult>,
}

impl fmt::Debug for Nip44DecryptBatchResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Nip44DecryptBatchResult")
            .field("items_len", &self.items.len())
            .finish()
    }
}

/// Per-item result for a batch decrypt call.
///
/// Exactly one of `plaintext` or `error` should be present.  The shape mirrors
/// the NIP-46 extension JSON so malformed signers can be tested directly.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip44DecryptBatchItemResult {
    /// Caller-chosen correlation id from the request item.
    pub id: String,
    /// Decrypted plaintext for successful items.  Treat as secret-bearing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plaintext: Option<String>,
    /// Per-item decrypt failure code or diagnostic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl fmt::Debug for Nip44DecryptBatchItemResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Nip44DecryptBatchItemResult")
            .field("id", &self.id)
            .field("plaintext", &self.plaintext.as_ref().map(|_| "[redacted]"))
            .field("error", &self.error)
            .finish()
    }
}

/// Request payload for `nmp_nip44_decrypt_session_end`.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip44DecryptSessionEndRequest {
    /// Opaque signer-owned session token.  Treat as secret-bearing.
    pub session_id: String,
}

impl fmt::Debug for Nip44DecryptSessionEndRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Nip44DecryptSessionEndRequest")
            .field("session_id", &"[redacted]")
            .finish()
    }
}
