//! Rung 1 — secret-key detection (issue #1804, hardened in #1882).
//!
//! Detects an `nsec` / `nostr:nsec` (NIP-19 secret) or an `ncryptsec` (NIP-49
//! encrypted secret) so the classifier can refuse it as
//! `Rejection(SecretLike)`. The detector deliberately returns only a `bool`: the
//! caller's rejection carries NO copy of the input (a secret is never logged,
//! stored, or echoed). Pure — no IO.
//!
//! Detection is **prefix-based**, not parse-based. A typoed / partial
//! `nsec1…` (e.g. captured mid-typing) is NOT a structurally-valid NIP-19
//! entity, so a decode-based check would let it fall through to the free-text
//! rung and be copied verbatim into a `TextQuery.request_json` — leaking a
//! (malformed but still secret-bearing) key. Matching the HRP prefix before any
//! decode guarantees every `nsec1…` / `ncryptsec1…` string is rejected and
//! never echoed (#1882).

/// True iff `input` is (or is a `nostr:`-prefixed) secret key — matched by HRP
/// prefix so a malformed/partial secret is still caught. Never copies the input.
pub(super) fn is_secret_like(input: &str) -> bool {
    // Lowercase the whole input first so the `nostr:` scheme, the `nsec` HRP,
    // and the `ncryptsec` HRP are all matched case-insensitively (bech32 HRPs
    // are lowercase, but a hand-typed value may use any case).
    let lower = input.to_ascii_lowercase();
    let body = lower.strip_prefix("nostr:").unwrap_or(lower.as_str());
    // `ncryptsec` (NIP-49) encrypted secret, then `nsec` (NIP-19) secret. We
    // reject on the bech32 HRP prefix WITHOUT decoding: a valid secret always
    // starts with one of these prefixes, and a *malformed* one (failed decode)
    // must be rejected too so it is never forwarded to free-text and echoed.
    body.starts_with("ncryptsec1") || body.starts_with("nsec1")
}
