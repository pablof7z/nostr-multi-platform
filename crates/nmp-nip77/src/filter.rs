//! Eligibility parsing for filters that can be reconciled exactly.

use std::fmt;

use nmp_core::store::{EventStore, StoreQuery};
use serde_json::Value;

use crate::reconciler::SyncedItem;

/// Parsed filter that can be represented exactly as local store queries.
#[derive(Clone, Debug)]
pub struct EligibleFilter {
    /// Original JSON value used in `NEG-OPEN`.
    pub value: Value,
    /// Hex pubkeys from `authors`.
    pub authors: Vec<String>,
    /// Explicit kind set.
    pub kinds: Vec<u32>,
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
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "authors" | "kinds" | "since" | "until" | "limit"
            ) {
                return Err(FilterEligibilityError::UnsupportedField(key.clone()));
            }
        }
        let authors = parse_string_array(object.get("authors"), "authors")?;
        let kinds = parse_kind_array(object.get("kinds"))?;
        if authors.is_empty() || kinds.is_empty() {
            return Err(FilterEligibilityError::EmptyDimension);
        }
        let since = parse_optional_u64(object.get("since"), "since")?;
        let until = parse_optional_u64(object.get("until"), "until")?;
        let limit = parse_optional_usize(object.get("limit"))?;
        Ok(Self {
            value,
            authors,
            kinds,
            since,
            until,
            limit,
        })
    }

    /// Author × kind product used by the large-filter gate.
    #[must_use]
    pub fn author_kind_pairs(&self) -> usize {
        self.authors.len().saturating_mul(self.kinds.len())
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
        let scan_limit = self.limit.unwrap_or(usize::MAX);
        for author_hex in &self.authors {
            let author = hex_to_32(author_hex)
                .ok_or_else(|| FilterEligibilityError::InvalidAuthor(author_hex.clone()))?;
            let query = StoreQuery::AuthorKind {
                author,
                kinds: self.kinds.clone(),
                since: self.since,
                until: self.until,
            };
            store
                .query_visit(&query, scan_limit, &mut |ev| {
                    out.push(SyncedItem {
                        created_at: ev.raw.created_at,
                        id: ev.raw.id_bytes(),
                    });
                    std::ops::ControlFlow::Continue(())
                })
                .map_err(|e| FilterEligibilityError::Store(e.to_string()))?;
        }
        if let Some(limit) = self.limit {
            out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
            out.truncate(limit);
        }
        Ok(out)
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
}

/// Reasons a filter cannot safely use NIP-77.
#[derive(Debug, Eq, PartialEq)]
pub enum FilterEligibilityError {
    /// JSON parse failed.
    MalformedJson,
    /// Filter must be a JSON object.
    NotObject,
    /// A field other than authors/kinds/since/until was present.
    UnsupportedField(String),
    /// `authors` or `kinds` was missing or empty.
    EmptyDimension,
    /// Field type was not accepted.
    InvalidField(&'static str),
    /// Author hex was malformed.
    InvalidAuthor(String),
    /// Store query failed.
    Store(String),
}

impl fmt::Display for FilterEligibilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedJson => f.write_str("malformed filter JSON"),
            Self::NotObject => f.write_str("filter must be an object"),
            Self::UnsupportedField(k) => write!(f, "unsupported filter field: {k}"),
            Self::EmptyDimension => f.write_str("authors and kinds must be non-empty"),
            Self::InvalidField(k) => write!(f, "invalid field: {k}"),
            Self::InvalidAuthor(a) => write!(f, "invalid author hex: {a}"),
            Self::Store(e) => write!(f, "store query failed: {e}"),
        }
    }
}

impl std::error::Error for FilterEligibilityError {}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn author(n: u8) -> String {
        format!("{n:02x}").repeat(32)
    }

    #[test]
    fn counts_author_kind_product() {
        let filter = EligibleFilter::parse(
            &serde_json::json!({
                "authors": [author(1), author(2), author(3)],
                "kinds": [0, 3],
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(filter.author_kind_pairs(), 6);
    }

    #[test]
    fn rejects_non_exact_filter_fields() {
        let err =
            EligibleFilter::parse(r##"{"authors":["aa"],"kinds":[1],"#e":["x"]}"##).unwrap_err();
        assert!(matches!(err, FilterEligibilityError::UnsupportedField(_)));
        assert!(EligibleFilter::parse(r#"{"ids":["aa"],"kinds":[1]}"#).is_err());
    }

    #[test]
    fn accepts_limit_and_can_build_live_only_filter() {
        let filter = EligibleFilter::parse(
            &serde_json::json!({
                "authors": [author(1)],
                "kinds": [1],
                "limit": 200,
            })
            .to_string(),
        )
        .unwrap();
        assert_eq!(filter.limit, Some(200));

        let live: Value = serde_json::from_str(&filter.live_only_filter_json()).unwrap();
        assert_eq!(live["limit"], Value::from(0));
        assert_eq!(live["kinds"], serde_json::json!([1]));
    }

    #[test]
    fn rejects_empty_dimensions() {
        assert!(matches!(
            EligibleFilter::parse(r#"{"authors":[],"kinds":[1]}"#),
            Err(FilterEligibilityError::EmptyDimension)
        ));
    }
}
