//! Rung 1 — secret-key detection (issue #1804).
//!
//! Detects an `nsec` / `nostr:nsec` (NIP-19 secret) or an `ncryptsec` (NIP-49
//! encrypted secret) so the classifier can refuse it as
//! `Rejection(SecretLike)`. The detector deliberately returns only a `bool`: the
//! caller's rejection carries NO copy of the input (a secret is never logged,
//! stored, or echoed). Pure — no IO.

/// True iff `input` is (or is a `nostr:`-prefixed) secret key. Never copies the
/// input.
pub(super) fn is_secret_like(input: &str) -> bool {
    let body = input.strip_prefix("nostr:").unwrap_or(input);
    // `ncryptsec` (NIP-49) is not a NIP-19 routing entity, so the decoder does
    // not recognize it — match its HRP prefix directly (case-insensitive).
    if body.to_ascii_lowercase().starts_with("ncryptsec1") {
        return true;
    }
    // `nsec` (and `nostr:nsec`) via the existing pure NIP-19 decoder.
    matches!(
        nmp_core::nip19::parse(body),
        Ok(nmp_core::nip19::Nip19Entity::Nsec(_))
    )
}
