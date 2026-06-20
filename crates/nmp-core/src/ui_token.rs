//! Structured user-facing error/progress tokens (issue #1682, part of #1670).
//!
//! ## Codex ruling
//!
//! User-facing error/progress **prose is presentation**. Rust owns the error
//! **semantics** (a stable machine `code`) plus the **raw diagnostic detail**
//! (kept for logs, never the UI contract); the shells own the localized prose.
//!
//! A [`UiToken`] is the wire-stable contract between the two:
//!
//! - `code` — a stable, closed machine key the shell maps to localized copy.
//!   The token shape here is protocol-**neutral**: the key is a `&'static str`
//!   constant that the *owning* crate defines (mirroring the closed
//!   `error_category` keys in [`crate::kernel::closed_reason`]). This keeps
//!   `nmp-core` free of protocol nouns (D0) while every producer reuses one
//!   shape.
//! - `subject` — optional contextual value the shell interpolates into its
//!   localized template (a relay URL, an envelope label, …).
//! - `severity` — the display class; the shell may also derive styling from
//!   the `code` alone.
//! - `raw_detail` — the upstream diagnostic string (e.g. a `nostr` parse
//!   error). Preserved for logs/diagnostics; it is **not** the UI contract and
//!   is logged at the emit site rather than rendered as prose.
//!
//! ## How a token crosses the FFI wire
//!
//! A token rides the **existing** typed-FFI error channel — no new wire
//! surface is introduced. [`crate::Kernel::set_last_error_token`] writes:
//!
//! - the snapshot's `last_error_toast` ← [`UiToken::fallback_prose`] (English,
//!   so non-localizing shells and diagnostics still read a sentence), and
//! - `last_error_category` ← [`UiToken::code`] (the machine key the shell
//!   branches on to render localized prose).
//!
//! This reuses the proven `error_category` contract that the relay-CLOSED path
//! already established (`kernel::closed_reason::ERR_*`), generalizing it from
//! relay errors to every protocol-crate toast.
//!
//! ## Code registry (single human-readable index)
//!
//! The closed `code` set is distributed across producing crates. Today:
//!
//! - `nmp-core` (this module, [`codes`]): keyring / relay-processing / signer
//!   bootstrap errors emitted from the kernel + actor.
//! - `nmp-nip17` (`nmp_nip17::ui_codes`): DM send + gift-wrap failures.
//! - `nmp-nip47` (`nmp_nip47::ui_codes`): NWC connect / encrypt / sign / wallet
//!   errors.
//!
//! Shells localize every key in those sets; an unknown key falls back to the
//! token's English `fallback_prose`.

/// Display class for a [`UiToken`]. The shell chooses styling from this (and
/// may further specialize on the `code`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    /// A failure the user should notice (the default for error toasts).
    Error,
    /// A recoverable / informational warning.
    Warning,
    /// Neutral information.
    Info,
    /// An in-flight progress label (not yet a terminal outcome).
    Progress,
}

/// A structured user-facing error/progress token. See the module docs for the
/// wire contract and the rationale (issue #1682).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiToken {
    /// Stable machine code from the owning crate's closed key set.
    code: &'static str,
    /// Display class.
    severity: Severity,
    /// Optional contextual value the shell interpolates into its template.
    subject: Option<String>,
    /// Upstream diagnostic detail — for logs, never the UI contract.
    raw_detail: Option<String>,
    /// English prose for non-localizing shells / diagnostics. The producing
    /// crate builds this (it owns its prose), so `nmp-core` never matches on a
    /// protocol code to synthesize a sentence.
    fallback: String,
}

impl UiToken {
    /// An [`Severity::Error`] token with `code` and its English `fallback`.
    #[must_use]
    pub fn error(code: &'static str, fallback: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            subject: None,
            raw_detail: None,
            fallback: fallback.into(),
        }
    }

    /// A [`Severity::Warning`] token with `code` and its English `fallback`.
    #[must_use]
    pub fn warning(code: &'static str, fallback: impl Into<String>) -> Self {
        Self {
            code,
            severity: Severity::Warning,
            subject: None,
            raw_detail: None,
            fallback: fallback.into(),
        }
    }

    /// Attach a contextual subject for shell interpolation.
    #[must_use]
    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    /// Attach the upstream diagnostic detail (logged, never rendered as prose).
    #[must_use]
    pub fn with_detail(mut self, raw_detail: impl Into<String>) -> Self {
        self.raw_detail = Some(raw_detail.into());
        self
    }

    /// The stable machine code (crosses the wire as `last_error_category`).
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// The display class.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.severity
    }

    /// The contextual subject, if any.
    #[must_use]
    pub fn subject(&self) -> Option<&str> {
        self.subject.as_deref()
    }

    /// The upstream diagnostic detail, if any.
    #[must_use]
    pub fn raw_detail(&self) -> Option<&str> {
        self.raw_detail.as_deref()
    }

    /// English prose for non-localizing shells / diagnostics (crosses the wire
    /// as `last_error_toast`).
    #[must_use]
    pub fn fallback_prose(&self) -> &str {
        &self.fallback
    }
}

/// `nmp-core`-owned [`UiToken::code`] constants — kernel/actor toasts that are
/// substrate concepts (keyring, relay processing, signer bootstrap), not
/// protocol nouns. Each is a stable wire key; the shells localize them.
pub mod codes {
    /// Keychain/keyring write for an account failed (session may not persist).
    pub const KEYRING_WRITE_FAILED: &str = "core_keyring_write_failed";
    /// A relay event handler panicked and was contained (processing continues).
    pub const RELAY_PROCESSING_ERROR: &str = "core_relay_processing_error";
    /// A `bunker://` (NIP-46) URI was structurally invalid.
    pub const SIGNER_BUNKER_INVALID_URI: &str = "signer_bunker_invalid_uri";
    /// The NIP-46 signer broker was not initialised before a URI reached it.
    pub const SIGNER_BROKER_NOT_INITIALISED: &str = "signer_broker_not_initialised";
    /// The external (NIP-55) signer driver was not initialised on restore.
    pub const SIGNER_NIP55_DRIVER_NOT_INITIALISED: &str =
        "signer_nip55_driver_not_initialised";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_carries_all_fields() {
        let t = UiToken::error("x_failed", "could not x: boom")
            .with_subject("alice")
            .with_detail("boom");
        assert_eq!(t.code(), "x_failed");
        assert_eq!(t.severity(), Severity::Error);
        assert_eq!(t.subject(), Some("alice"));
        assert_eq!(t.raw_detail(), Some("boom"));
        assert_eq!(t.fallback_prose(), "could not x: boom");
    }

    #[test]
    fn warning_severity() {
        let t = UiToken::warning("x_warn", "heads up");
        assert_eq!(t.severity(), Severity::Warning);
    }
}
