//! Read-interest shapes for NIP-87 mint discovery (issue #2880).
//!
//! Mirrors `nmp-nip57::interests`/`nmp-nip17`'s per-crate convention: a
//! protocol/product crate owns its own subscription shapes rather than
//! folding into the kernel's framework-default bootstrap self-kinds list
//! (`nmp-core`'s `SELF_KINDS_TAILING`), which stays product-noun-free (D0) —
//! `nmp-core` must not learn NIP-87/WoT kind numbers.

use std::collections::BTreeSet;

use nmp_planner::InterestShape;

use nmp_nip87::{KIND_MINT_ANNOUNCE, KIND_MINT_RECOMMEND};
use nmp_wot::{KIND_CONTACT_LIST, KIND_MUTE_LIST};

/// NIP-87 mint discovery: all kind:38172 announcements + kind:38000
/// recommendations, no author filter. Discovery is a whole-kind public read —
/// the reading account narrows recommendations to its web of trust *in Rust*
/// at aggregation time (see `discovery.rs`), never at the relay filter, the
/// same coarse-relay/precise-Rust split other NMP protocol crates use for
/// public global reads.
#[must_use]
pub fn mint_discovery_shape() -> InterestShape {
    InterestShape {
        kinds: BTreeSet::from([KIND_MINT_ANNOUNCE, KIND_MINT_RECOMMEND]),
        ..Default::default()
    }
}

/// The active account's own follow list (kind:3) and mute list (kind:10000) —
/// the raw material the discovery store folds into an `nmp-wot::WotGraph` to
/// score mint recommenders. `authors`=self keeps this a narrow, self-scoped
/// read; it feeds direct-follow and self-mute scoring. (Deeper
/// follows-of-follows enrichment can grow later without changing this shape —
/// the aggregation degrades gracefully on whatever graph is present.)
#[must_use]
pub fn mint_discovery_trust_graph_shape(pubkey: &str) -> InterestShape {
    InterestShape {
        authors: BTreeSet::from([pubkey.to_string()]),
        kinds: BTreeSet::from([KIND_CONTACT_LIST, KIND_MUTE_LIST]),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn mint_discovery_shape_is_whole_kind_no_author_or_tag_filter() {
        let shape = mint_discovery_shape();
        assert!(shape.kinds.contains(&KIND_MINT_ANNOUNCE));
        assert!(shape.kinds.contains(&KIND_MINT_RECOMMEND));
        assert!(
            shape.authors.is_empty(),
            "discovery is a global read; WoT scoping happens in Rust"
        );
        assert!(shape.tags.is_empty());
    }

    #[test]
    fn mint_discovery_trust_graph_shape_is_self_scoped() {
        let shape = mint_discovery_trust_graph_shape(PK);
        assert_eq!(shape.authors, BTreeSet::from([PK.to_string()]));
        assert!(shape.kinds.contains(&KIND_CONTACT_LIST));
        assert!(shape.kinds.contains(&KIND_MUTE_LIST));
    }
}
