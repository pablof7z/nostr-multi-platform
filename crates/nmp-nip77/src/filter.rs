//! Eligibility parsing for NIP-01 filters that can be reconciled exactly.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::ops::ControlFlow;

use nmp_coverage_gate::ResultSurface;
use nmp_store::{EventStore, StoreQuery, StoredEvent};
use serde_json::Value;

use crate::reconciler::SyncedItem;

/// Parsed NIP-01 filter plus the exact local matching machinery NIP-77 needs.
#[derive(Clone, Debug)]
pub struct EligibleFilter {
    /// Original JSON value used in `NEG-OPEN`.
    pub value: Value,
    /// Exact event ids from `ids`.
    pub ids: Vec<String>,
    /// Hex pubkeys from `authors`.
    pub authors: Vec<String>,
    /// Explicit kind set. Empty means wildcard.
    pub kinds: Vec<u32>,
    /// Generic single-letter tag filters without the leading `#`.
    pub tags: BTreeMap<String, Vec<String>>,
    /// Optional lower timestamp bound.
    pub since: Option<u64>,
    /// Optional upper timestamp bound.
    pub until: Option<u64>,
    /// Optional maximum number of newest matching events.
    pub limit: Option<usize>,
}

impl EligibleFilter {
    /// Parse and validate a NIP-01 filter JSON object.
    pub fn parse(filter_json: &str) -> Result<Self, FilterEligibilityError> {
        let value: Value =
            serde_json::from_str(filter_json).map_err(|_| FilterEligibilityError::MalformedJson)?;
        let object = value.as_object().ok_or(FilterEligibilityError::NotObject)?;
        let mut tags = BTreeMap::new();
        for key in object.keys() {
            match key.as_str() {
                "ids" | "authors" | "kinds" | "since" | "until" | "limit" => {}
                "search" => return Err(FilterEligibilityError::SearchUnsupported),
                other => {
                    let Some(tag_key) = other.strip_prefix('#') else {
                        return Err(FilterEligibilityError::UnsupportedField(key.clone()));
                    };
                    let mut chars = tag_key.chars();
                    let (Some(c), None) = (chars.next(), chars.next()) else {
                        return Err(FilterEligibilityError::UnsupportedField(key.clone()));
                    };
                    if !c.is_ascii_alphabetic() {
                        return Err(FilterEligibilityError::UnsupportedField(key.clone()));
                    }
                    tags.insert(
                        tag_key.to_string(),
                        parse_string_array(object.get(key), "tag")?,
                    );
                }
            }
        }
        let ids = parse_string_array(object.get("ids"), "ids")?;
        let authors = parse_string_array(object.get("authors"), "authors")?;
        let kinds = parse_kind_array(object.get("kinds"))?;
        let since = parse_optional_u64(object.get("since"), "since")?;
        let until = parse_optional_u64(object.get("until"), "until")?;
        let limit = parse_optional_usize(object.get("limit"))?;
        Ok(Self {
            value,
            ids,
            authors,
            kinds,
            tags,
            since,
            until,
            limit,
        })
    }

    /// Static upper bound for this filter's possible result set.
    #[must_use]
    pub fn result_surface(&self) -> ResultSurface {
        let mut known = if self.ids.is_empty() {
            None
        } else {
            Some(self.ids.len())
        };

        if self.is_replaceable_author_key() {
            known = min_known(known, self.authors.len().saturating_mul(self.kinds.len()));
        }

        if let Some(d_values) = self.tags.get("d") {
            if self.is_addressable_author_d_key() {
                known = min_known(
                    known,
                    self.authors
                        .len()
                        .saturating_mul(self.kinds.len())
                        .saturating_mul(d_values.len()),
                );
            }
        }

        if let Some(limit) = self.limit {
            known = min_known(known, limit);
        }

        known.map_or(ResultSurface::Unbounded, ResultSurface::KnownMax)
    }

    /// Read matching local event ids from the store.
    pub fn local_items(
        &self,
        store: &dyn EventStore,
    ) -> Result<Vec<SyncedItem>, FilterEligibilityError> {
        if self.limit == Some(0) {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        let mut seen = HashSet::new();

        if !self.ids.is_empty() {
            for id_hex in &self.ids {
                let Some(id) = hex_to_32(id_hex) else {
                    continue;
                };
                let Some(ev) = store
                    .get_by_id(&id)
                    .map_err(|e| FilterEligibilityError::Store(e.to_string()))?
                else {
                    continue;
                };
                self.push_if_match(&ev, &mut seen, &mut out);
            }
            self.apply_limit(out)
        } else {
            let queries = self.store_queries();
            if queries.is_empty() {
                return Err(FilterEligibilityError::NoLocalQuery);
            }
            for query in queries {
                store
                    .query_visit(&query, usize::MAX, &mut |ev| {
                        self.push_if_match(ev, &mut seen, &mut out);
                        ControlFlow::Continue(())
                    })
                    .map_err(|e| FilterEligibilityError::Store(e.to_string()))?;
            }
            self.apply_limit(out)
        }
    }

    /// Return a copy of this filter with its `since` lower bound removed.
    ///
    /// NIP-77 set reconciliation should cover the full historical window. A
    /// watermark-floor `since` is a REQ optimization, not proof that the client
    /// has every below-floor event. `until` and `limit` remain part of the
    /// requested set, so they are preserved.
    #[must_use]
    pub fn unfloored(&self) -> Self {
        let mut value = self.value.clone();
        if let Some(object) = value.as_object_mut() {
            object.remove("since");
        }
        Self {
            value,
            ids: self.ids.clone(),
            authors: self.authors.clone(),
            kinds: self.kinds.clone(),
            tags: self.tags.clone(),
            since: None,
            until: self.until,
            limit: self.limit,
        }
    }

    /// Return the same filter as a live-only NIP-01 subscription.
    ///
    /// NIP-77 performs the stored-set reconciliation while this paired REQ asks
    /// the relay only for events that arrive after the live subscription opens.
    pub fn live_only_filter_json(&self) -> String {
        let mut value = self.value.clone();
        if let Some(object) = value.as_object_mut() {
            object.insert("limit".to_string(), Value::from(0));
        }
        serde_json::to_string(&value).unwrap_or_else(|_| r#"{"limit":0}"#.to_string())
    }

    fn is_replaceable_author_key(&self) -> bool {
        !self.authors.is_empty()
            && !self.kinds.is_empty()
            && self
                .kinds
                .iter()
                .all(|k| matches!(*k, 0 | 3 | 10_000..=19_999))
    }

    fn is_addressable_author_d_key(&self) -> bool {
        !self.authors.is_empty()
            && !self.kinds.is_empty()
            && self.tags.get("d").is_some_and(|values| !values.is_empty())
            && self.kinds.iter().all(|k| (30_000..=39_999).contains(k))
    }

    fn store_queries(&self) -> Vec<StoreQuery> {
        let kinds = self.kinds.clone();
        if let Some(values) = self.tags.get("e") {
            let queries: Vec<_> = values
                .iter()
                .filter_map(|target| {
                    hex_to_32(target).map(|target| StoreQuery::Etag {
                        target,
                        kinds: kinds.clone(),
                    })
                })
                .collect();
            if !queries.is_empty() {
                return queries;
            }
        }
        if let Some(values) = self.tags.get("p") {
            let queries: Vec<_> = values
                .iter()
                .filter_map(|target| {
                    hex_to_32(target).map(|target| StoreQuery::Ptag {
                        target,
                        kinds: kinds.clone(),
                    })
                })
                .collect();
            if !queries.is_empty() {
                return queries;
            }
        }
        if let Some(values) = self.tags.get("d") {
            let addressable_kinds: Vec<u32> = self
                .kinds
                .iter()
                .copied()
                .filter(|k| (30_000..=39_999).contains(k))
                .collect();
            let queries: Vec<_> = addressable_kinds
                .iter()
                .flat_map(|kind| {
                    values.iter().map(|d_tag| StoreQuery::KindDtag {
                        kind: *kind,
                        d_tag: d_tag.as_bytes().to_vec(),
                        since: self.since,
                        until: self.until,
                    })
                })
                .collect();
            if !queries.is_empty() {
                return queries;
            }
        }

        let authors: BTreeSet<_> = self.authors.iter().filter_map(|a| hex_to_32(a)).collect();
        match (authors.len(), self.kinds.is_empty()) {
            (1, false) => {
                let Some(author) = authors.iter().next().copied() else {
                    return Vec::new();
                };
                vec![StoreQuery::AuthorKind {
                    author,
                    kinds,
                    since: self.since,
                    until: self.until,
                }]
            }
            (2.., false) => vec![StoreQuery::AuthorsKind {
                authors,
                kinds,
                since: self.since,
                until: self.until,
            }],
            _ => vec![StoreQuery::KindTime {
                kinds,
                since: self.since,
                until: self.until,
            }],
        }
    }

    fn push_if_match(
        &self,
        ev: &StoredEvent,
        seen: &mut HashSet<[u8; 32]>,
        out: &mut Vec<SyncedItem>,
    ) {
        if !self.matches(ev) {
            return;
        }
        let Some(id) = ev.raw.id_bytes() else {
            return;
        };
        if seen.insert(id) {
            out.push(SyncedItem {
                created_at: ev.raw.created_at,
                id,
            });
        }
    }

    fn matches(&self, ev: &StoredEvent) -> bool {
        if !self.ids.is_empty() && !self.ids.contains(&ev.raw.id) {
            return false;
        }
        if !self.authors.is_empty() && !self.authors.contains(&ev.raw.pubkey) {
            return false;
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&ev.raw.kind) {
            return false;
        }
        if self.since.is_some_and(|since| ev.raw.created_at < since) {
            return false;
        }
        if self.until.is_some_and(|until| ev.raw.created_at > until) {
            return false;
        }
        for (tag_key, wanted) in &self.tags {
            if wanted.is_empty() {
                continue;
            }
            let satisfied = ev.raw.tags.iter().any(|row| {
                row.first().is_some_and(|key| key == tag_key)
                    && row.get(1).is_some_and(|value| wanted.contains(value))
            });
            if !satisfied {
                return false;
            }
        }
        true
    }

    fn apply_limit(
        &self,
        mut out: Vec<SyncedItem>,
    ) -> Result<Vec<SyncedItem>, FilterEligibilityError> {
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        if let Some(limit) = self.limit {
            out.truncate(limit);
        }
        Ok(out)
    }
}

/// Reasons a filter cannot safely use NIP-77.
#[derive(Debug, Eq, PartialEq)]
pub enum FilterEligibilityError {
    /// JSON parse failed.
    MalformedJson,
    /// Filter must be a JSON object.
    NotObject,
    /// A field outside the NIP-01 structural filter set was present.
    UnsupportedField(String),
    /// NIP-50 search is relay-evaluated and has no exact structural local set.
    SearchUnsupported,
    /// Field type was not accepted.
    InvalidField(&'static str),
    /// No store query can produce an exact local candidate set.
    NoLocalQuery,
    /// Store query failed.
    Store(String),
}

impl fmt::Display for FilterEligibilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedJson => f.write_str("malformed filter JSON"),
            Self::NotObject => f.write_str("filter must be an object"),
            Self::UnsupportedField(k) => write!(f, "unsupported filter field: {k}"),
            Self::SearchUnsupported => f.write_str("search filters are not NIP-77 eligible"),
            Self::InvalidField(k) => write!(f, "invalid field: {k}"),
            Self::NoLocalQuery => f.write_str("no exact local store query"),
            Self::Store(e) => write!(f, "store query failed: {e}"),
        }
    }
}

impl std::error::Error for FilterEligibilityError {}

fn min_known(current: Option<usize>, candidate: usize) -> Option<usize> {
    Some(current.map_or(candidate, |current| current.min(candidate)))
}

fn parse_string_array(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Vec<String>, FilterEligibilityError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or(FilterEligibilityError::InvalidField(field))?;
    array
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or(FilterEligibilityError::InvalidField(field))
        })
        .collect()
}

fn parse_kind_array(value: Option<&Value>) -> Result<Vec<u32>, FilterEligibilityError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or(FilterEligibilityError::InvalidField("kinds"))?;
    array
        .iter()
        .map(|v| {
            let n = v
                .as_u64()
                .ok_or(FilterEligibilityError::InvalidField("kinds"))?;
            u32::try_from(n).map_err(|_| FilterEligibilityError::InvalidField("kinds"))
        })
        .collect()
}

fn parse_optional_u64(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Option<u64>, FilterEligibilityError> {
    value
        .map(|v| {
            v.as_u64()
                .ok_or(FilterEligibilityError::InvalidField(field))
        })
        .transpose()
}

fn parse_optional_usize(value: Option<&Value>) -> Result<Option<usize>, FilterEligibilityError> {
    value
        .map(|v| {
            let n = v
                .as_u64()
                .ok_or(FilterEligibilityError::InvalidField("limit"))?;
            usize::try_from(n).map_err(|_| FilterEligibilityError::InvalidField("limit"))
        })
        .transpose()
}

fn hex_to_32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in s.as_bytes().chunks(2).enumerate() {
        out[i] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
