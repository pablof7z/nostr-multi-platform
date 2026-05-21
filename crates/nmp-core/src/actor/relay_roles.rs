//! Relay role parsing for `RelayEditRow.role`.

/// Fallback relay for client-initiated NIP-46 `nostrconnect://` handshakes
/// when the user has no configured write relay.
pub const NOSTRCONNECT_DEFAULT_RELAY_URL: &str = "wss://relay.damus.io";

/// True when `role` semantically includes `needle`.
///
/// `both` means read+write only; it does not imply indexer. Composite role
/// strings such as `both,indexer` are tokenized on commas, plus signs, and
/// whitespace.
pub(crate) fn has_role(role: &str, needle: &str) -> bool {
    let n = needle.trim().to_ascii_lowercase();
    role_tokens(role)
        .any(|token| token == n || (token == "both" && matches!(n.as_str(), "read" | "write")))
}

/// Normalize a relay role string into the stored `RelayEditRow.role` form.
pub(crate) fn canonical_relay_role(role: &str) -> Option<String> {
    let mut read = false;
    let mut write = false;
    let mut indexer = false;
    let mut saw_token = false;

    for token in role_tokens(role) {
        saw_token = true;
        match token.as_str() {
            "read" => read = true,
            "write" => write = true,
            "both" => {
                read = true;
                write = true;
            }
            "indexer" => indexer = true,
            _ => return None,
        }
    }

    if !saw_token {
        read = true;
        write = true;
    }

    let mut parts = Vec::new();
    match (read, write) {
        (true, true) => parts.push("both"),
        (true, false) => parts.push("read"),
        (false, true) => parts.push("write"),
        (false, false) => {}
    }
    if indexer {
        parts.push("indexer");
    }
    (!parts.is_empty()).then(|| parts.join(","))
}

fn role_tokens(role: &str) -> impl Iterator<Item = String> + '_ {
    role.split(|c: char| c == ',' || c == '+' || c.is_whitespace())
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
}

/// Choose the relay for a client-initiated NIP-46 `nostrconnect://` flow.
///
/// The relay is the first configured relay whose canonical role includes
/// write semantics. If none exists, use the substrate-owned fallback.
pub(crate) fn nostrconnect_relay_url<'a, I>(rows: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    rows.into_iter()
        .find(|(_, role)| has_role(role, "write"))
        .map(|(url, _)| url.to_string())
        .unwrap_or_else(|| NOSTRCONNECT_DEFAULT_RELAY_URL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nostrconnect_prefers_first_write_eligible_relay() {
        let rows = [
            ("wss://read.example", "read"),
            ("wss://write.example", "write"),
            ("wss://both.example", "both"),
        ];

        assert_eq!(
            nostrconnect_relay_url(rows),
            "wss://write.example",
            "first write-capable relay should own nostrconnect handshakes"
        );
    }

    #[test]
    fn nostrconnect_accepts_composite_role_tokens() {
        let rows = [
            ("wss://indexer.example", "indexer"),
            ("wss://composite.example", "both,indexer"),
        ];

        assert_eq!(
            nostrconnect_relay_url(rows),
            "wss://composite.example",
            "both,indexer semantically includes write"
        );
    }

    #[test]
    fn nostrconnect_falls_back_without_write_relay() {
        let rows = [
            ("wss://read.example", "read"),
            ("wss://indexer.example", "indexer"),
        ];

        assert_eq!(nostrconnect_relay_url(rows), NOSTRCONNECT_DEFAULT_RELAY_URL);
    }
}
