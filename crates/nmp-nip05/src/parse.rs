//! Pure NIP-05 identifier shape parsing (no IO).

/// Parse a NIP-05 `name@domain` identifier into its `(name, domain)` parts.
///
/// PURE — no IO. Returns `None` when the input is not a valid NIP-05 shape
/// (missing/empty parts, illegal local-part charset, malformed domain). The
/// domain is lowercased; the `name` local-part is validated against the NIP-05
/// charset (`a-z0-9-_.`). The bare-`_` root identifier (`_@domain`) is accepted.
///
/// SHAPE ONLY — this never performs the `.well-known/nostr.json` lookup; that is
/// [`crate::ResolveNip05Command`] (the dispatch-layer IO step).
#[must_use]
pub fn parse_nip05(identifier: &str) -> Option<(String, String)> {
    // NIP-05: `<local-part>@<domain>`. Exactly one `@` separates the two
    // parts; either part empty is malformed.
    let (name, domain) = identifier.split_once('@')?;
    if name.is_empty() || domain.is_empty() {
        return None;
    }
    // The local-part charset is `a-z0-9-_.` (NIP-05). We do NOT lowercase the
    // local-part — it is the on-`nostr.json` key and must match verbatim — so a
    // case-sensitive identifier with uppercase letters is simply not a NIP-05
    // shape here. The bare `_` root identifier is admitted (it is a single
    // legal local-part character).
    if !is_valid_local_part(name) {
        return None;
    }
    // The domain is host-only (no scheme, no path, no port, no `@`). Lowercase
    // it — DNS is case-insensitive and the well-known URL is built from it.
    let domain = domain.to_ascii_lowercase();
    if !is_valid_domain(&domain) {
        return None;
    }
    Some((name.to_string(), domain))
}

/// NIP-05 local-part charset: ASCII `a-z`, `0-9`, `-`, `_`, `.`. Uppercase is
/// rejected (the key must match the `names` map verbatim). Empty was already
/// rejected by the caller.
fn is_valid_local_part(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

/// Minimal host validation: non-empty labels of `[a-z0-9-]` joined by `.`, at
/// least one dot (a bare hostname is not a resolvable NIP-05 domain), no
/// leading/trailing/double dots, no leading/trailing hyphen per label. This is
/// a shape guard, not a registry check — the HTTP layer is the real authority.
fn is_valid_domain(domain: &str) -> bool {
    if !domain.contains('.') {
        return false;
    }
    domain.split('.').all(|label| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    })
}

#[cfg(test)]
mod tests {
    use super::parse_nip05;

    #[test]
    fn name_at_domain() {
        assert_eq!(
            parse_nip05("alice@example.com"),
            Some(("alice".to_string(), "example.com".to_string()))
        );
    }

    #[test]
    fn root_identifier_underscore() {
        assert_eq!(
            parse_nip05("_@example.com"),
            Some(("_".to_string(), "example.com".to_string()))
        );
    }

    #[test]
    fn local_part_with_legal_punctuation() {
        assert_eq!(
            parse_nip05("a.b-c_d@sub.example.com"),
            Some(("a.b-c_d".to_string(), "sub.example.com".to_string()))
        );
    }

    #[test]
    fn domain_is_lowercased() {
        assert_eq!(
            parse_nip05("alice@Example.COM"),
            Some(("alice".to_string(), "example.com".to_string()))
        );
    }

    #[test]
    fn rejects_uppercase_local_part() {
        // The local-part must match the `names` key verbatim; uppercase is not
        // a NIP-05 shape (we do not silently lowercase it).
        assert_eq!(parse_nip05("Alice@example.com"), None);
    }

    #[test]
    fn rejects_missing_at() {
        assert_eq!(parse_nip05("aliceexample.com"), None);
    }

    #[test]
    fn rejects_empty_parts() {
        assert_eq!(parse_nip05("@example.com"), None);
        assert_eq!(parse_nip05("alice@"), None);
        assert_eq!(parse_nip05("@"), None);
        assert_eq!(parse_nip05(""), None);
    }

    #[test]
    fn rejects_multiple_at() {
        assert_eq!(parse_nip05("a@b@example.com"), None);
    }

    #[test]
    fn rejects_bare_hostname_domain() {
        assert_eq!(parse_nip05("alice@localhost"), None);
    }

    #[test]
    fn rejects_illegal_local_part_char() {
        assert_eq!(parse_nip05("ali ce@example.com"), None);
        assert_eq!(parse_nip05("alice!@example.com"), None);
    }

    #[test]
    fn rejects_malformed_domain() {
        assert_eq!(parse_nip05("alice@example..com"), None);
        assert_eq!(parse_nip05("alice@-example.com"), None);
        assert_eq!(parse_nip05("alice@.example.com"), None);
    }
}
