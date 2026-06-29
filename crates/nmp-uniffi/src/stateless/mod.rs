//! Stateless (no-kernel-IO) UniFFI helpers — M14-C1 per-surface layout.
//!
//! Each sub-module owns one surface and carries parity tests that verify
//! equivalence with the corresponding `nmp-ffi` C-ABI path. All four functions
//! migrated here are synchronous and side-effect-free:
//!
//! | Module        | UniFFI fn              | C-ABI counterpart            |
//! |---------------|------------------------|------------------------------|
//! | `nip19`       | `encode_profile`       | `nmp_app_encode_profile`     |
//! | `nip21`       | `decode_nostr_uri`     | `nmp_nip21_decode_uri`       |
//! | `content`     | `tokenize_content`     | `nmp_content_tokenize_text`  |
//! | `intent`      | `classify_intent`      | `nmp_app_intent_classify`    |
//!
//! The layout ensures C2–C7 slices stay file-disjoint (no merge conflicts
//! from parallel branches touching different surfaces).

pub mod content;
pub mod intent;
pub mod nip19;
pub mod nip21;

// ── Shared error type ─────────────────────────────────────────────────────────

/// UniFFI-exported error for stateless fns that can fail.
///
/// `encode_profile` (NIP-19) never fails — it echoes the raw input on any
/// encode failure per D6 — and does NOT use this type. The other three
/// surfaces use it for decode/tokenize/classify failures.
#[derive(Debug, uniffi::Error)]
pub enum NmpError {
    /// The caller supplied a null, empty, or structurally invalid input.
    InvalidInput,
    /// The input could not be parsed as the expected NIP-19/21 entity.
    Unparseable,
    /// A NIP-19 `nsec` key was detected. The key is NEVER echoed back.
    NsecForbidden,
    /// The render-mode discriminant supplied to `tokenize_content` is unknown.
    InvalidMode,
    /// An internal encoding step failed (e.g. FlatBuffers write error).
    EncodeFailed,
}

impl std::fmt::Display for NmpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NmpError::InvalidInput => write!(f, "invalid input"),
            NmpError::Unparseable => write!(f, "unparseable"),
            NmpError::NsecForbidden => write!(f, "nsec forbidden: secret key rejected"),
            NmpError::InvalidMode => write!(f, "invalid render mode"),
            NmpError::EncodeFailed => write!(f, "encode failed"),
        }
    }
}

impl std::error::Error for NmpError {}
