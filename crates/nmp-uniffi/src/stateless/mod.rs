//! Stateless (no-kernel-IO) UniFFI helpers — M14-C1 per-surface layout.
//!
//! Each sub-module owns one surface and carries parity tests that verify
//! equivalence with the migrated or retained native path. All four functions
//! migrated here are synchronous and side-effect-free:
//!
//! | Module        | UniFFI fn              | Retired/retained C-ABI       |
//! |---------------|------------------------|------------------------------|
//! | `nip19`       | `encode_profile`       | retired `nmp_app_encode_profile` |
//! | `nip21`       | `decode_nostr_uri`     | Typed NIP-21/NIP-19 decode   |
//! | `content`     | `tokenize_content`     | Typed content tokenization   |
//! | `intent`      | `classify_intent`      | retired `nmp_app_intent_classify` |
//!
//! The layout ensures C2–C7 slices stay file-disjoint (no merge conflicts
//! from parallel branches touching different surfaces).

pub mod content;
pub mod intent;
pub mod nip19;
pub mod nip21;

// ── Shared error type ─────────────────────────────────────────────────────────

/// UniFFI-exported error for fns that can fail.
///
/// `encode_profile` (NIP-19) never fails — it echoes the raw input on any
/// encode failure per D6 — and does NOT use this type. The other three
/// stateless surfaces and the M14-C2 identity/signer surfaces use it for
/// decode/tokenize/classify failures and configuration-phase errors.
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
    /// A pre-start configuration call was made after the runtime had already
    /// started (M14-C2: maps from `NmpConfigStatus::AlreadyStarted`).
    AlreadyStarted,
    /// A feed session could not be opened: the scope is not wired by the
    /// default compiler, the session registry is unavailable (poisoned lock),
    /// or the compiler returned another typed failure. Distinct from
    /// `InvalidInput` (which covers JSON parse / primary-kind validation errors
    /// that fire BEFORE the compiler runs).
    FeedOpenFailed,
    /// An internal mutex was poisoned (another thread panicked while holding
    /// it). Maps from `NmpConfigStatus::Unavailable` and
    /// `IncrementalApplyError::RegistryUnavailable` (M14-C6).
    RegistryUnavailable,
}

impl std::fmt::Display for NmpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NmpError::InvalidInput => write!(f, "invalid input"),
            NmpError::Unparseable => write!(f, "unparseable"),
            NmpError::NsecForbidden => write!(f, "nsec forbidden: secret key rejected"),
            NmpError::InvalidMode => write!(f, "invalid render mode"),
            NmpError::EncodeFailed => write!(f, "encode failed"),
            NmpError::AlreadyStarted => {
                write!(f, "already started: configuration after runtime start")
            }
            NmpError::FeedOpenFailed => {
                write!(
                    f,
                    "feed open failed: scope not wired or registry unavailable"
                )
            }
            NmpError::RegistryUnavailable => {
                write!(f, "registry unavailable: internal mutex poisoned")
            }
        }
    }
}

impl std::error::Error for NmpError {}
