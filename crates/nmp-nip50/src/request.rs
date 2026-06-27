//! NIP-50 search request: query + scope + targets, and the planner/interest
//! bridges. The planner/core substrate owns only the generic `search` filter
//! field; this crate owns the NIP-50 query scopes.

use std::collections::BTreeSet;

use nmp_core::substrate::ViewDependencies;
use nmp_kinds::{KIND_LONG_FORM_ARTICLE, KIND_PROFILE_METADATA};
use nmp_planner::interest::bounded_search_query;
use nmp_planner::InterestShape;
use serde::{Deserialize, Serialize};

pub const DEFAULT_MAX_SEARCH_HITS: usize = 200;
pub const HARD_MAX_SEARCH_HITS: usize = 500;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchScope {
    Users,
    LongForm,
    Kinds(BTreeSet<u32>),
    Custom(InterestShape),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchTargets {
    UserPreferred,
    Explicit(Vec<String>),
    AppDefault,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub scope: SearchScope,
    pub targets: SearchTargets,
    pub max_hits: usize,
}

impl SearchRequest {
    #[must_use]
    pub fn new(
        query: &str,
        scope: SearchScope,
        targets: SearchTargets,
        max_hits: Option<usize>,
    ) -> Option<Self> {
        Some(Self {
            query: bounded_search_query(query)?,
            scope,
            targets,
            max_hits: max_hits
                .unwrap_or(DEFAULT_MAX_SEARCH_HITS)
                .min(HARD_MAX_SEARCH_HITS),
        })
    }

    #[must_use]
    pub fn interest_shape(&self) -> InterestShape {
        let mut shape = match &self.scope {
            SearchScope::Users => InterestShape {
                kinds: BTreeSet::from([KIND_PROFILE_METADATA]),
                ..Default::default()
            },
            SearchScope::LongForm => InterestShape {
                kinds: BTreeSet::from([KIND_LONG_FORM_ARTICLE]),
                ..Default::default()
            },
            SearchScope::Kinds(kinds) => InterestShape {
                kinds: kinds.clone(),
                ..Default::default()
            },
            SearchScope::Custom(shape) => shape.clone(),
        };
        shape.search = Some(self.query.clone());
        shape.limit = Some(self.max_hits.min(u32::MAX as usize) as u32);
        shape
    }

    #[must_use]
    pub fn view_dependencies(&self) -> Option<ViewDependencies> {
        let kinds: Vec<u32> = match &self.scope {
            SearchScope::Users => vec![KIND_PROFILE_METADATA],
            SearchScope::LongForm => vec![KIND_LONG_FORM_ARTICLE],
            SearchScope::Kinds(kinds) => kinds.iter().copied().collect(),
            SearchScope::Custom(_) => return None,
        };
        Some(ViewDependencies {
            kinds,
            search: Some(self.query.clone()),
            limit: Some(self.max_hits.min(u32::MAX as usize) as u32),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_kinds::KIND_SHORT_TEXT_NOTE;

    #[test]
    fn request_builds_bounded_search_shape() {
        let request = SearchRequest::new(
            "  nostr rust  ",
            SearchScope::Kinds(BTreeSet::from([KIND_SHORT_TEXT_NOTE])),
            SearchTargets::UserPreferred,
            Some(10),
        )
        .expect("query");

        let shape = request.interest_shape();
        assert_eq!(shape.search.as_deref(), Some("nostr rust"));
        assert_eq!(shape.kinds, BTreeSet::from([KIND_SHORT_TEXT_NOTE]));
        assert_eq!(shape.limit, Some(10));
    }

    #[test]
    fn request_rejects_empty_query_and_caps_hits() {
        assert!(SearchRequest::new(
            "   ",
            SearchScope::Users,
            SearchTargets::UserPreferred,
            None
        )
        .is_none());

        let request = SearchRequest::new(
            "nostr",
            SearchScope::Users,
            SearchTargets::UserPreferred,
            Some(HARD_MAX_SEARCH_HITS + 1),
        )
        .expect("query");
        assert_eq!(request.max_hits, HARD_MAX_SEARCH_HITS);
    }
}
