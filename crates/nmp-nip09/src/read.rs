//! Generic NIP-09 deletion read seam — parse a kind:5 event's tags.
//!
//! Any projection that processes kind:5 deletion events (e.g. `nmp-nip25`'s
//! `ReactionProjection::ingest_delete`) should call [`deletion_targets`] rather
//! than hand-parsing `e` tags. This keeps the tag-grammar interpretation
//! centralised in `nmp-nip09`.

/// Parsed targets claimed by a kind:5 deletion event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeletionTargets {
    /// Hex-64 event ids listed in `e` tags — the events being deleted.
    pub event_ids: Vec<String>,
    /// Kind integers listed in `k` tags (may be empty per NIP-09 §3).
    pub kinds: Vec<u32>,
}

/// Parse a kind:5 event's tags into the artifacts it claims to delete.
///
/// Unknown tags are silently ignored. Malformed `k` tag values that do not
/// parse as `u32` are dropped. This function never fails: the caller decides
/// what to do with an empty result.
#[must_use]
pub fn deletion_targets(tags: &[Vec<String>]) -> DeletionTargets {
    let mut event_ids = Vec::new();
    let mut kinds = Vec::new();
    for tag in tags {
        let Some(name) = tag.first() else { continue };
        match name.as_str() {
            "e" => {
                if let Some(id) = tag.get(1).filter(|v| !v.is_empty()) {
                    event_ids.push(id.clone());
                }
            }
            "k" => {
                if let Some(k_str) = tag.get(1) {
                    if let Ok(k) = k_str.parse::<u32>() {
                        kinds.push(k);
                    }
                }
            }
            _ => {}
        }
    }
    DeletionTargets { event_ids, kinds }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(name: &str, value: &str) -> Vec<String> {
        vec![name.to_string(), value.to_string()]
    }

    const ID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const ID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn deletion_targets_parses_e_tags() {
        let tags = vec![t("e", ID_A), t("e", ID_B)];
        let targets = deletion_targets(&tags);
        assert_eq!(targets.event_ids, vec![ID_A.to_string(), ID_B.to_string()]);
        assert!(targets.kinds.is_empty());
    }

    #[test]
    fn deletion_targets_parses_k_tags() {
        let tags = vec![t("e", ID_A), t("k", "7"), t("k", "1")];
        let targets = deletion_targets(&tags);
        assert_eq!(targets.event_ids, vec![ID_A.to_string()]);
        assert_eq!(targets.kinds, vec![7u32, 1u32]);
    }

    #[test]
    fn deletion_targets_ignores_unknown_tags() {
        let tags = vec![
            t("e", ID_A),
            t("p", "aaaa"),
            t("alt", "something"),
            t("h", "group-id"),
        ];
        let targets = deletion_targets(&tags);
        assert_eq!(targets.event_ids, vec![ID_A.to_string()]);
        assert!(targets.kinds.is_empty());
    }

    #[test]
    fn deletion_targets_drops_malformed_k_values() {
        let tags = vec![t("e", ID_A), t("k", "notanumber"), t("k", "7")];
        let targets = deletion_targets(&tags);
        assert_eq!(targets.kinds, vec![7u32]);
    }

    #[test]
    fn deletion_targets_empty_tags_returns_empty_result() {
        let targets = deletion_targets(&[]);
        assert_eq!(targets, DeletionTargets::default());
    }
}
