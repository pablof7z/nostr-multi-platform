//! NIP-01 `CLOSED` reason-prefix classifier.
//!
//! Per NIP-01 §A, a `["CLOSED", <sub_id>, <reason>]` frame's reason string is
//! a free-form message that MAY begin with a machine-readable prefix:
//!
//! - `auth-required:` — the relay requires NIP-42 AUTH before serving this REQ
//! - `restricted:`    — the REQ is denied by relay policy (paid-only, etc.)
//! - `blocked:`       — this client is blocked by the relay
//! - `shadowbanned:`  — relay-side flag; semantically equivalent to `blocked` for routing
//! - `rate-limited:`  — back off and retry later
//! - `error:`         — generic relay-side error
//! - `invalid:`       — malformed REQ shape
//! - `unsupported:`   — the relay does not implement the requested filter
//! - `pow:`           — the relay requires a higher `PoW` threshold (NIP-13)
//! - `duplicate:`     — this REQ is a duplicate of an existing subscription
//!
//! This module is pure policy: a `&str` reason → a [`CloseReason`] enum. The
//! kernel ingest path (`kernel/ingest/closed.rs`) is the only consumer and
//! decides which side-effect each variant triggers (auth pause, denied flag,
//! log + give up, etc.). Keeping classification separate from action means
//! the routing table can grow without re-touching the parser.
//!
//! D7 compliance: capability layer (the wire) delivers the CLOSED frame
//! verbatim; this classifier is a policy lookup the kernel applies.

// ── Typed FFI error contract — closed key set ───────────────────────────────
//
// `error_category` (on `RelayStatus` and the snapshot's `last_error_category`)
// carries one of these five stable keys so iOS can branch on error *class*
// programmatically instead of substring-matching English `last_error` prose.
// The set is CLOSED: adding a key is a deliberate FFI-contract change. These
// constants are the single source of truth — every callsite that stamps a
// category MUST use one of them, never an inline literal.

/// Relay demands NIP-42 AUTH (or rejected our AUTH event) — the user/host
/// can recover by authenticating.
pub(crate) const ERR_AUTH_REQUIRED: &str = "auth_required";
/// Temporary condition — back off and retry (rate-limited, connection drop).
pub(crate) const ERR_TRANSIENT: &str = "transient";
/// Non-recoverable relay-side error — retrying will not help.
pub(crate) const ERR_PERMANENT: &str = "permanent";
/// The event/REQ this client sent was structurally malformed.
pub(crate) const ERR_MALFORMED_EVENT: &str = "malformed_event";
/// Relay policy denied this client (restricted / blocked / shadowbanned).
pub(crate) const ERR_POLICY_DENIED: &str = "policy_denied";

/// Action category derived from a NIP-01 CLOSED reason prefix.
///
/// Unknown prefixes (or an empty/absent reason) fold to [`Self::Unknown`],
/// which the kernel handles the same way as [`Self::Error`]: log + give up.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CloseReason {
    /// `auth-required:` — pause REQs to this relay until NIP-42 AUTH lands.
    AuthRequired,
    /// `restricted:` — relay policy denies this REQ; do not retry.
    Restricted,
    /// `blocked:` — client is blocked by the relay; do not retry.
    Blocked,
    /// `shadowbanned:` — relay-side flag; treated as `Blocked` for routing.
    Shadowbanned,
    /// `rate-limited:` — relay is throttling; the reconnect/REQ worker
    /// should back off before retrying. Classifier reports; backoff
    /// state mutation is the worker's job (kept out of this module).
    RateLimited,
    /// `error:` — generic relay-side error; log + give up.
    Error,
    /// `invalid:` — REQ shape malformed; log + give up (likely a bug).
    Invalid,
    /// `unsupported:` — relay does not implement the filter; log + give up.
    Unsupported,
    /// `pow:` — relay demands higher `PoW` (NIP-13); treated as Error (NMP does
    /// not generate `PoW` events, so there is no recovery action available).
    Pow,
    /// `duplicate:` — relay says this sub overlaps an existing one; log only.
    Duplicate,
    /// No machine-readable prefix matched (or reason was empty). Folds to
    /// the same action policy as `Error`.
    Unknown,
}

impl CloseReason {
    /// Stable diagnostic key for surfaces (e.g. `RelayStatus.last_error`,
    /// log lines). Matches the NIP-01 prefix verbatim (sans trailing colon).
    pub(crate) fn as_key(self) -> &'static str {
        match self {
            Self::AuthRequired => "auth-required",
            Self::Restricted => "restricted",
            Self::Blocked => "blocked",
            Self::Shadowbanned => "shadowbanned",
            Self::RateLimited => "rate-limited",
            Self::Error => "error",
            Self::Invalid => "invalid",
            Self::Unsupported => "unsupported",
            Self::Pow => "pow",
            Self::Duplicate => "duplicate",
            Self::Unknown => "unknown",
        }
    }

    /// Map this CLOSED reason onto the typed FFI `error_category` key (the
    /// closed set above). `None` for `Duplicate` — a duplicate REQ is not an
    /// error and stamps no `last_error`, so it carries no category either.
    ///
    /// Mapping rationale:
    /// - `AuthRequired` → `auth_required` — recoverable by NIP-42 AUTH.
    /// - `Restricted | Blocked | Shadowbanned` → `policy_denied` — the relay
    ///   refuses this client by policy; recovery is a fresh socket / re-pay.
    /// - `RateLimited` → `transient` — back off and retry.
    /// - `Invalid` → `malformed_event` — the REQ shape this client sent was
    ///   malformed (NIP-01 §A).
    /// - `Error | Unsupported | Pow | Unknown` → `permanent` — retrying the
    ///   same REQ against the same relay will not help.
    #[must_use]
    pub(crate) fn error_category(self) -> Option<&'static str> {
        match self {
            Self::AuthRequired => Some(ERR_AUTH_REQUIRED),
            Self::Restricted | Self::Blocked | Self::Shadowbanned => Some(ERR_POLICY_DENIED),
            Self::RateLimited => Some(ERR_TRANSIENT),
            Self::Invalid => Some(ERR_MALFORMED_EVENT),
            Self::Error | Self::Unsupported | Self::Pow | Self::Unknown => Some(ERR_PERMANENT),
            Self::Duplicate => None,
        }
    }
}

/// Classify a NIP-01 CLOSED reason string by its leading prefix.
///
/// Matching is case-sensitive and prefix-only (the NIP defines the lowercase
/// prefix as a *prefix* of the reason, so longer messages like
/// `"auth-required: please AUTH"` still classify as `AuthRequired`).
///
/// Whitespace at the start of `reason` is trimmed because some relays emit
/// `" auth-required: ..."` (leading space). Returns [`CloseReason::Unknown`]
/// when no known prefix matches — the kernel treats this the same as
/// [`CloseReason::Error`] (log + give up).
pub(crate) fn classify(reason: &str) -> CloseReason {
    let trimmed = reason.trim_start();
    if trimmed.starts_with("auth-required:") {
        CloseReason::AuthRequired
    } else if trimmed.starts_with("restricted:") {
        CloseReason::Restricted
    } else if trimmed.starts_with("blocked:") {
        CloseReason::Blocked
    } else if trimmed.starts_with("shadowbanned:") {
        CloseReason::Shadowbanned
    } else if trimmed.starts_with("rate-limited:") {
        CloseReason::RateLimited
    } else if trimmed.starts_with("error:") {
        CloseReason::Error
    } else if trimmed.starts_with("invalid:") {
        CloseReason::Invalid
    } else if trimmed.starts_with("unsupported:") {
        CloseReason::Unsupported
    } else if trimmed.starts_with("pow:") {
        CloseReason::Pow
    } else if trimmed.starts_with("duplicate:") {
        CloseReason::Duplicate
    } else {
        CloseReason::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_each_nip01_prefix() {
        assert_eq!(
            classify("auth-required: please AUTH"),
            CloseReason::AuthRequired
        );
        assert_eq!(classify("restricted: paid only"), CloseReason::Restricted);
        assert_eq!(classify("blocked: spam"), CloseReason::Blocked);
        assert_eq!(classify("shadowbanned: sorry"), CloseReason::Shadowbanned);
        assert_eq!(
            classify("rate-limited: slow down"),
            CloseReason::RateLimited
        );
        assert_eq!(classify("error: internal"), CloseReason::Error);
        assert_eq!(classify("invalid: bad filter"), CloseReason::Invalid);
        assert_eq!(
            classify("unsupported: kinds out of range"),
            CloseReason::Unsupported
        );
        assert_eq!(classify("pow: need 24 bits"), CloseReason::Pow);
        assert_eq!(classify("duplicate: same sub"), CloseReason::Duplicate);
    }

    #[test]
    fn unknown_prefix_folds_to_unknown() {
        assert_eq!(classify("totally-made-up: foo"), CloseReason::Unknown);
        assert_eq!(classify("no colon here"), CloseReason::Unknown);
        assert_eq!(classify(""), CloseReason::Unknown);
    }

    #[test]
    fn leading_whitespace_is_tolerated() {
        // Real relays in the wild sometimes prepend a space — match anyway.
        assert_eq!(classify(" auth-required: x"), CloseReason::AuthRequired);
        assert_eq!(classify("\t rate-limited: x"), CloseReason::RateLimited);
    }

    #[test]
    fn prefix_must_include_trailing_colon() {
        // `"authoritative: foo"` must NOT classify as AuthRequired even
        // though it shares the `auth` stem — the trailing `:` is part of
        // the NIP-01 prefix grammar.
        assert_eq!(classify("authoritative: foo"), CloseReason::Unknown);
        assert_eq!(classify("error-ish: foo"), CloseReason::Unknown);
    }

    #[test]
    fn as_key_matches_nip01_prefix() {
        // The diagnostic key must match the NIP-01 prefix verbatim (no
        // colon). Pinning this so diag surfaces stay aligned with the spec.
        assert_eq!(CloseReason::AuthRequired.as_key(), "auth-required");
        assert_eq!(CloseReason::Restricted.as_key(), "restricted");
        assert_eq!(CloseReason::Blocked.as_key(), "blocked");
        assert_eq!(CloseReason::RateLimited.as_key(), "rate-limited");
        assert_eq!(CloseReason::Unknown.as_key(), "unknown");
    }
}
