//! NIP-02 contact-list tag parsing.

/// Derive the full follow set from a contact-list event's tags.
///
/// Keeps every valid 64-hex `p` tag in document order. Duplicates are
/// preserved because the follow-list row is a wire artifact; consumers that need
/// set semantics can deduplicate at their boundary.
#[must_use]
pub(crate) fn contact_follows(tags: &[Vec<String>]) -> Vec<String> {
    tags.iter()
        .filter_map(|tag| {
            if tag.first().map(String::as_str) == Some("p") {
                tag.get(1).filter(|value| is_hex_pubkey(value)).cloned()
            } else {
                None
            }
        })
        .collect()
}

fn is_hex_pubkey(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::contact_follows;

    fn hex_pk(i: usize) -> String {
        format!(
            "{:016x}{}",
            i as u64, "0123456789abcdef0123456789abcdef0123456789abcdef"
        )
    }

    fn p_tags(follows: &[String]) -> Vec<Vec<String>> {
        follows
            .iter()
            .map(|pk| vec!["p".to_string(), pk.clone()])
            .collect()
    }

    #[test]
    fn contact_follows_is_uncapped_and_in_order() {
        let follows: Vec<String> = (0..600).map(hex_pk).collect();
        let extracted = contact_follows(&p_tags(&follows));
        assert_eq!(extracted.len(), 600);
        assert_eq!(extracted, follows);
    }

    #[test]
    fn contact_follows_below_threshold_returns_all() {
        let follows: Vec<String> = (0..3).map(hex_pk).collect();
        assert_eq!(contact_follows(&p_tags(&follows)), follows);
    }

    #[test]
    fn contact_follows_skips_non_hex_p_values() {
        let valid_a = hex_pk(1);
        let valid_b = hex_pk(2);
        let tags = vec![
            vec!["p".to_string(), "not-hex".to_string()],
            vec!["p".to_string(), valid_a.clone()],
            vec!["p".to_string(), "tooshort".to_string()],
            vec!["p".to_string(), valid_b.clone()],
        ];
        assert_eq!(contact_follows(&tags), vec![valid_a, valid_b]);
    }

    #[test]
    fn contact_follows_ignores_non_p_tags() {
        let pk = hex_pk(7);
        let tags = vec![
            vec!["e".to_string(), hex_pk(99)],
            vec!["p".to_string(), pk.clone()],
            vec!["t".to_string(), "topic".to_string()],
        ];
        assert_eq!(contact_follows(&tags), vec![pk]);
    }

    #[test]
    fn contact_follows_preserves_duplicate_slots_no_dedup() {
        let pk = hex_pk(3);
        let tags = vec![
            vec!["p".to_string(), pk.clone()],
            vec!["p".to_string(), pk.clone()],
        ];
        assert_eq!(contact_follows(&tags), vec![pk.clone(), pk]);
    }
}
