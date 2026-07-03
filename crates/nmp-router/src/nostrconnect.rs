//! Router-owned relay selection for client-initiated NIP-46 bootstrap.

/// Choose the relay for a client-initiated NIP-46 `nostrconnect://` flow from
/// configured relay rows.
///
/// Returns the first write-capable relay URL. The composition root remains
/// responsible for applying any host-registered fallback when this returns
/// `None`.
pub fn bootstrap_relay_url<'a, I, F>(rows: I, mut is_write_capable: F) -> Option<String>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
    F: FnMut(&str) -> bool,
{
    rows.into_iter()
        .find(|(_, role)| is_write_capable(role))
        .map(|(url, _)| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_capable(role: &str) -> bool {
        role.split(|c: char| c == ',' || c == '+' || c.is_whitespace())
            .map(str::to_ascii_lowercase)
            .any(|token| token == "write" || token == "both")
    }

    #[test]
    fn prefers_first_write_eligible_relay() {
        let rows = [
            ("wss://read.example", "read"),
            ("wss://write.example", "write"),
            ("wss://both.example", "both"),
        ];

        assert_eq!(
            bootstrap_relay_url(rows, write_capable),
            Some("wss://write.example".to_string())
        );
    }

    #[test]
    fn accepts_composite_role_tokens() {
        let rows = [
            ("wss://indexer.example", "indexer"),
            ("wss://composite.example", "both,indexer"),
        ];

        assert_eq!(
            bootstrap_relay_url(rows, write_capable),
            Some("wss://composite.example".to_string())
        );
    }

    #[test]
    fn returns_none_without_write_relay() {
        let rows = [
            ("wss://read.example", "read"),
            ("wss://indexer.example", "indexer"),
        ];

        assert_eq!(bootstrap_relay_url(rows, write_capable), None);
    }
}
