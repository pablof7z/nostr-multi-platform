//! `GroupEventsQuery` — a consumer's declaration of *which group AND which
//! kinds* it wants to read.
//!
//! NIP-29 owns ONLY the `["h", local_id]` group-routing concern. A group read
//! model is the set of events h-tagged into a group; the **consumer** chooses
//! the kind set. This module is the single source of that selection: it builds
//! both the relay-interest `filter_json` (the NIP-01 `REQ` the relay sees) and
//! the projection's accept predicate from the same normalized query, so the two
//! can never drift.
//!
//! Design: GitHub issue #2187 (replaces the kind-hardcoded `chat_filter_json`
//! on `GroupId`; kind policy must never live on the identity type).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;

/// The consumer-declared kind set for a group read model.
///
/// - [`GroupEventKinds::All`] — every h-tagged group event regardless of kind.
/// - [`GroupEventKinds::Specific`] — only the listed kinds (e.g. `{9, 11}` for a
///   chat view). The set is a [`BTreeSet`] so the wire `kinds` array is emitted
///   in a deterministic ascending order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupEventKinds {
    /// All h-tagged group events — no kind constraint.
    All,
    /// Only the listed kinds.
    Specific(BTreeSet<u32>),
}

impl GroupEventKinds {
    /// Build from a flat kind list. **Empty = [`GroupEventKinds::All`]** (all
    /// h-tagged group events); a non-empty list becomes
    /// [`GroupEventKinds::Specific`].
    ///
    /// This is the door's normalization rule: the C-ABI / FFI contract treats an
    /// empty `kinds` array as "all kinds", while a MISSING `kinds` field is
    /// rejected upstream (a `GroupId`-only payload must not silently widen into
    /// a broad read).
    #[must_use]
    pub fn from_vec(kinds: Vec<u32>) -> Self {
        if kinds.is_empty() {
            Self::All
        } else {
            Self::Specific(kinds.into_iter().collect())
        }
    }

    /// Whether `kind` is admitted by this selection.
    #[must_use]
    pub fn matches(&self, kind: u32) -> bool {
        match self {
            Self::All => true,
            Self::Specific(kinds) => kinds.contains(&kind),
        }
    }
}

/// A consumer's normalized declaration of a NIP-29 group read model: a
/// [`GroupId`] plus the [`GroupEventKinds`] it wants.
///
/// One value feeds BOTH sides of the read:
/// - [`GroupEventsQuery::filter_json`] — the NIP-01 relay-interest filter.
/// - [`GroupEventsQuery::accepts_kind`] — the projection's per-event kind gate
///   (the projection still kind-gates because cache replay / store hydration /
///   test injection can deliver other same-`h` kinds the filter did not request).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupEventsQuery {
    group: GroupId,
    kinds: GroupEventKinds,
}

impl GroupEventsQuery {
    /// Construct from a typed [`GroupId`] and a [`GroupEventKinds`] selection.
    #[must_use]
    pub fn new(group: GroupId, kinds: GroupEventKinds) -> Self {
        Self { group, kinds }
    }

    /// Construct from a [`GroupId`] and a flat kind list, applying the door's
    /// empty-means-all normalization (see [`GroupEventKinds::from_vec`]).
    #[must_use]
    pub fn from_kinds(group: GroupId, kinds: Vec<u32>) -> Self {
        Self::new(group, GroupEventKinds::from_vec(kinds))
    }

    /// The group this query reads.
    #[must_use]
    pub fn group(&self) -> &GroupId {
        &self.group
    }

    /// The declared kind selection.
    #[must_use]
    pub fn kinds(&self) -> &GroupEventKinds {
        &self.kinds
    }

    /// Whether `kind` is admitted by this query's kind selection.
    #[must_use]
    pub fn accepts_kind(&self, kind: u32) -> bool {
        self.kinds.matches(kind)
    }

    /// NIP-01 `REQ` filter JSON for this group read model.
    ///
    /// - [`GroupEventKinds::Specific`] → `{"kinds":[…],"#h":["<local_id>"]}`
    /// - [`GroupEventKinds::All`] → `{"#h":["<local_id>"]}` (no `kinds` key)
    ///
    /// The client-side host-relay pin is NOT part of this filter — the wire
    /// `REQ` a relay sees carries only `kinds` + `#h`. The composition root
    /// (`nmp-ffi`) attaches the pin as a separate `relay_pin` argument when it
    /// opens the observed interest. This keeps `nmp-nip29` NmpApp-free (D0): it
    /// produces the wire filter shape; routing is the composer's concern.
    #[must_use]
    pub fn filter_json(&self) -> String {
        let mut map = serde_json::Map::new();
        if let GroupEventKinds::Specific(kinds) = &self.kinds {
            let kinds: Vec<u32> = kinds.iter().copied().collect();
            map.insert("kinds".to_string(), serde_json::json!(kinds));
        }
        map.insert(
            "#h".to_string(),
            serde_json::json!([self.group.local_id]),
        );
        serde_json::Value::Object(map).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> GroupId {
        GroupId::new("wss://groups.example.com", "room-a")
    }

    #[test]
    fn empty_kinds_is_all() {
        assert_eq!(GroupEventKinds::from_vec(vec![]), GroupEventKinds::All);
    }

    #[test]
    fn non_empty_kinds_is_specific() {
        assert_eq!(
            GroupEventKinds::from_vec(vec![11, 9, 9]),
            GroupEventKinds::Specific([9, 11].into_iter().collect())
        );
    }

    #[test]
    fn all_matches_every_kind() {
        let k = GroupEventKinds::All;
        assert!(k.matches(9));
        assert!(k.matches(1111));
        assert!(k.matches(40000));
    }

    #[test]
    fn specific_matches_only_listed_kinds() {
        let k = GroupEventKinds::from_vec(vec![9, 11]);
        assert!(k.matches(9));
        assert!(k.matches(11));
        assert!(!k.matches(1111));
        assert!(!k.matches(7));
    }

    #[test]
    fn specific_filter_json_carries_kinds_and_h_in_order() {
        let q = GroupEventsQuery::from_kinds(group(), vec![11, 9]);
        let v: serde_json::Value = serde_json::from_str(&q.filter_json()).unwrap();
        // BTreeSet orders the kinds ascending and dedups.
        assert_eq!(v["kinds"], serde_json::json!([9, 11]));
        assert_eq!(v["#h"], serde_json::json!(["room-a"]));
        assert!(v.get("relay_pin").is_none());
        assert!(nmp_planner::InterestShape::from_filter_json(&q.filter_json()).is_some());
    }

    #[test]
    fn all_filter_json_omits_kinds() {
        let q = GroupEventsQuery::new(group(), GroupEventKinds::All);
        let v: serde_json::Value = serde_json::from_str(&q.filter_json()).unwrap();
        assert!(v.get("kinds").is_none());
        assert_eq!(v["#h"], serde_json::json!(["room-a"]));
        assert!(nmp_planner::InterestShape::from_filter_json(&q.filter_json()).is_some());
    }

    #[test]
    fn accepts_kind_delegates_to_selection() {
        let q = GroupEventsQuery::from_kinds(group(), vec![9, 11]);
        assert!(q.accepts_kind(9));
        assert!(!q.accepts_kind(1111));
        assert_eq!(q.group(), &group());
    }
}
