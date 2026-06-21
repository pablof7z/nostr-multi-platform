//! Canonical address-coordinate identity for replaceable/addressable events.
//!
//! A parameterized-replaceable event (NIP-01 kinds 30000–39999) is identified
//! not by its event id but by its **address coordinate** `(kind, pubkey, d-tag)`.
//! The newest event at a coordinate supersedes older ones — versions collapse to
//! one row, they do not stack. NIP-18 generic reposts (kind:16) of such events,
//! NIP-23 long-form article feeds, NIP-68 picture feeds, and NIP-09 deletes that
//! carry an `a` tag all share this one identity.
//!
//! This module is the **single canonical place** that computes that identity
//! for the *feed/render* layer (issue #1740 step 5). Every feed-side consumer —
//! `nmp-content`'s long-form adapter, `nmp-nip68`, `nmp-nip01`'s op-feed delete
//! handling — formats and parses the coordinate here rather than each
//! re-deriving `format!("{kind}:{pubkey}:{d}")`. Keeping it in one function
//! means the wire string for the `a` tag and the feed row id can never silently
//! diverge. (The `nmp-store` kind:5 tombstone path independently parses `a`
//! tags at the storage layer; the two agree on the `kind:pubkey:d` form.)

use nmp_core::substrate::KernelEvent;

/// The NIP-01 parameterized-replaceable kind range, inclusive.
const PARAM_REPLACEABLE_RANGE: std::ops::RangeInclusive<u32> = 30_000..=39_999;

/// Return whether `kind` is a parameterized-replaceable (addressable) kind.
///
/// Only addressable kinds have a `d`-tag-bearing address coordinate; a non-
/// addressable kind is identified by event id alone.
#[must_use]
pub const fn is_addressable_kind(kind: u32) -> bool {
    *PARAM_REPLACEABLE_RANGE.start() <= kind && kind <= *PARAM_REPLACEABLE_RANGE.end()
}

/// Address coordinate of a replaceable/addressable event: `kind:pubkey:d-tag`.
///
/// This is the canonical wire/identity form shared by the NIP-01 `a` tag, the
/// feed row id, and the store's kind:5 tombstone key. The newest event at this
/// coordinate is the row; older versions collapse into it.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AddressCoordinate {
    /// Event kind (always addressable, 30000–39999).
    pub kind: u32,
    /// Author pubkey (hex).
    pub pubkey: String,
    /// `d` tag identifier.
    pub identifier: String,
}

impl AddressCoordinate {
    /// Construct a coordinate from its parts.
    #[must_use]
    pub fn new(kind: u32, pubkey: impl Into<String>, identifier: impl Into<String>) -> Self {
        Self {
            kind,
            pubkey: pubkey.into(),
            identifier: identifier.into(),
        }
    }

    /// Compute the coordinate of an addressable event, reading its `d` tag.
    ///
    /// Returns `None` for non-addressable kinds — those have no coordinate and
    /// must be identified by event id (never guess one). A missing `d` tag is
    /// treated as the empty identifier, matching NIP-01's default.
    #[must_use]
    pub fn from_event(event: &KernelEvent) -> Option<Self> {
        if !is_addressable_kind(event.kind) {
            return None;
        }
        let identifier = d_tag(&event.tags).unwrap_or_default();
        Some(Self::new(event.kind, event.author.clone(), identifier))
    }

    /// Parse a wire coordinate string (`kind:pubkey:d-tag`).
    ///
    /// Fails closed (`None`) when the kind is missing/non-numeric/non-addressable
    /// so an `a` tag that does not name an addressable coordinate is never
    /// fabricated into one. All three colon-separated components are
    /// **required**: `30023:pubkey:` denotes the empty-`d` default explicitly,
    /// while a two-component string like `30023:pubkey` is malformed and rejected
    /// rather than silently completed into an empty-`d` coordinate. The pubkey
    /// must be present; the identifier may be empty (NIP-01 default `d`).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        let mut parts = raw.splitn(3, ':');
        let kind: u32 = parts.next()?.parse().ok()?;
        if !is_addressable_kind(kind) {
            return None;
        }
        let pubkey = parts.next()?;
        if pubkey.is_empty() {
            return None;
        }
        // The third component must be present (possibly empty). Its absence is a
        // malformed coordinate, not an empty-`d` default.
        let identifier = parts.next()?;
        Some(Self::new(kind, pubkey, identifier))
    }

    /// Canonical wire string `kind:pubkey:d-tag`.
    #[must_use]
    pub fn to_wire(&self) -> String {
        format!("{}:{}:{}", self.kind, self.pubkey, self.identifier)
    }
}

/// Extract the first `d` tag value from a tag list.
fn d_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter().find_map(|tag| {
        if tag.first().is_some_and(|name| name == "d") {
            tag.get(1).cloned()
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: u32, author: &str, d: Option<&str>) -> KernelEvent {
        let mut tags = Vec::new();
        if let Some(d) = d {
            tags.push(vec!["d".to_string(), d.to_string()]);
        }
        KernelEvent {
            id: "id".to_string(),
            author: author.to_string(),
            kind,
            created_at: 1,
            tags,
            content: String::new(),
            relay_provenance: Vec::new(),
        }
    }

    #[test]
    fn addressable_range_is_30000_to_39999() {
        assert!(!is_addressable_kind(29_999));
        assert!(is_addressable_kind(30_000));
        assert!(is_addressable_kind(30_023));
        assert!(is_addressable_kind(39_999));
        assert!(!is_addressable_kind(40_000));
        assert!(!is_addressable_kind(1));
        assert!(!is_addressable_kind(20));
    }

    #[test]
    fn from_event_only_for_addressable_kinds() {
        assert!(AddressCoordinate::from_event(&event(1, "alice", None)).is_none());
        assert!(AddressCoordinate::from_event(&event(20, "alice", None)).is_none());
        let coord = AddressCoordinate::from_event(&event(30_023, "alice", Some("d1"))).unwrap();
        assert_eq!(coord.kind, 30_023);
        assert_eq!(coord.pubkey, "alice");
        assert_eq!(coord.identifier, "d1");
    }

    #[test]
    fn missing_d_tag_defaults_to_empty_identifier() {
        let coord = AddressCoordinate::from_event(&event(30_023, "alice", None)).unwrap();
        assert_eq!(coord.identifier, "");
        assert_eq!(coord.to_wire(), "30023:alice:");
    }

    #[test]
    fn wire_roundtrip() {
        let coord = AddressCoordinate::new(30_023, "alice", "my-article");
        assert_eq!(coord.to_wire(), "30023:alice:my-article");
        assert_eq!(AddressCoordinate::parse("30023:alice:my-article"), Some(coord));
    }

    #[test]
    fn parse_allows_colon_in_identifier() {
        let coord = AddressCoordinate::parse("30023:alice:has:colons").unwrap();
        assert_eq!(coord.identifier, "has:colons");
    }

    #[test]
    fn parse_fails_closed_on_non_addressable_or_malformed() {
        // Non-addressable kind: not a coordinate, never fabricate one.
        assert_eq!(AddressCoordinate::parse("1:alice:d"), None);
        assert_eq!(AddressCoordinate::parse("20:alice:d"), None);
        // Non-numeric kind.
        assert_eq!(AddressCoordinate::parse("nope:alice:d"), None);
        // Missing pubkey.
        assert_eq!(AddressCoordinate::parse("30023"), None);
        assert_eq!(AddressCoordinate::parse("30023::d"), None);
        // Two-component string is malformed: it must NOT be completed into an
        // empty-`d` coordinate (that would fabricate identity).
        assert_eq!(AddressCoordinate::parse("30023:alice"), None);
        // Explicit empty `d` (trailing colon) is the valid empty-`d` form.
        assert_eq!(
            AddressCoordinate::parse("30023:alice:"),
            Some(AddressCoordinate::new(30_023, "alice", ""))
        );
    }
}
