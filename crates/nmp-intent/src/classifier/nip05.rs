//! Rung 4 — NIP-05 SHAPE detection (issue #1804).
//!
//! Recognizes a `name@domain` / `_@domain` identifier by SHAPE only — no HTTP,
//! no `.well-known/nostr.json` fetch (that is the dispatch layer's job). Pure.
//!
//! Deliberately conservative: a false negative just falls through to free-text
//! search; a false positive would mis-route a query to a reverse-lookup fetch.

/// SHAPE-only NIP-05 detection. Returns the canonical identifier (the trimmed
/// input) when the shape matches, else `None`. No IO.
pub(super) fn nip05_shape(input: &str) -> Option<String> {
    let (local, domain) = input.split_once('@')?;
    if local.is_empty() || domain.is_empty() {
        return None;
    }
    if !local.chars().all(is_nip05_local_char) {
        return None;
    }
    if !is_domain_shape(domain) {
        return None;
    }
    Some(input.to_string())
}

/// NIP-05 local-part charset (`a-z A-Z 0-9 - _ .`). The `_` root identifier is a
/// single-char local part and passes here.
fn is_nip05_local_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
}

/// A domain shape: at least two dot-separated labels, each a valid DNS label,
/// and a final label (TLD) of ≥2 ASCII letters. SHAPE only.
fn is_domain_shape(domain: &str) -> bool {
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    if labels.iter().any(|l| !is_domain_label(l)) {
        return false;
    }
    let tld = labels[labels.len() - 1];
    tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

/// One DNS label: non-empty, ASCII alphanumeric with internal (non-edge)
/// hyphens.
fn is_domain_label(label: &str) -> bool {
    if label.is_empty() || label.starts_with('-') || label.ends_with('-') {
        return false;
    }
    label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}
