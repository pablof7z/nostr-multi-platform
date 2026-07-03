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

/// Return the full kind:3 tag set that results from adding a follow on
/// `target` to `current`, preserving every existing non-matching tag verbatim.
#[must_use]
pub fn tags_after_add(current: &[Vec<String>], target: &str) -> Vec<Vec<String>> {
    let mut tags = current.to_vec();
    let already_present = tags.iter().any(|tag| {
        tag.first().map(String::as_str) == Some("p")
            && tag.get(1).map(String::as_str) == Some(target)
    });
    if !already_present {
        tags.push(vec!["p".to_string(), target.to_string()]);
    }
    tags
}

/// Return the full kind:3 tag set that results from removing every `p` tag for
/// `target`, preserving all other tags and columns verbatim.
#[must_use]
pub fn tags_after_remove(current: &[Vec<String>], target: &str) -> Vec<Vec<String>> {
    current
        .iter()
        .filter(|tag| {
            !(tag.first().map(String::as_str) == Some("p")
                && tag.get(1).map(String::as_str) == Some(target))
        })
        .cloned()
        .collect()
}

/// Build the initial kind:3 tag set for a new account's app-supplied follows.
#[must_use]
pub fn initial_tags(follows: &[String]) -> Vec<Vec<String>> {
    follows
        .iter()
        .map(|pubkey| vec!["p".to_string(), pubkey.clone()])
        .collect()
}

fn is_hex_pubkey(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::{contact_follows, initial_tags, tags_after_add, tags_after_remove};

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

    #[test]
    fn tags_after_add_preserves_existing_tags_and_columns() {
        let existing = hex_pk(10);
        let new_target = hex_pk(11);
        let tags = vec![
            vec![
                "r".to_string(),
                "wss://relay".to_string(),
                "read".to_string(),
            ],
            vec![
                "p".to_string(),
                existing.clone(),
                "wss://hint".to_string(),
                "alice".to_string(),
            ],
        ];
        let edited = tags_after_add(&tags, &new_target);
        assert_eq!(
            edited,
            vec![
                vec![
                    "r".to_string(),
                    "wss://relay".to_string(),
                    "read".to_string()
                ],
                vec![
                    "p".to_string(),
                    existing,
                    "wss://hint".to_string(),
                    "alice".to_string(),
                ],
                vec!["p".to_string(), new_target],
            ]
        );
    }

    #[test]
    fn tags_after_add_is_idempotent() {
        let pk = hex_pk(12);
        let tags = vec![vec![
            "p".to_string(),
            pk.clone(),
            "wss://hint".to_string(),
            "alice".to_string(),
        ]];
        assert_eq!(tags_after_add(&tags, &pk), tags);
    }

    #[test]
    fn tags_after_remove_drops_matching_p_tags_of_any_arity() {
        let target = hex_pk(13);
        let kept = hex_pk(14);
        let tags = vec![
            vec!["r".to_string(), "wss://relay".to_string()],
            vec!["p".to_string(), target.clone()],
            vec![
                "p".to_string(),
                target.clone(),
                "wss://hint".to_string(),
                "alice".to_string(),
            ],
            vec!["p".to_string(), kept.clone(), "wss://other".to_string()],
        ];
        assert_eq!(
            tags_after_remove(&tags, &target),
            vec![
                vec!["r".to_string(), "wss://relay".to_string()],
                vec!["p".to_string(), kept, "wss://other".to_string()],
            ]
        );
    }

    #[test]
    fn initial_tags_builds_bare_p_tags_in_order() {
        let follows = vec![hex_pk(15), hex_pk(16)];
        assert_eq!(initial_tags(&follows), p_tags(&follows));
    }
}
