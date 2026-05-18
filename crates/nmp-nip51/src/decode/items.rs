//! Typed list-item extraction from raw NIP-51 tags.
//!
//! NIP-51 list entries are ordinary tags whose first column is the entry kind
//! (`p` pubkey, `e` event, `a` address coordinate, `t` hashtag, `r`/`relay`
//! relay url, `word` muted word). This module turns the raw `Vec<Vec<String>>`
//! into the typed vectors [`crate::decode::ListRecord`] exposes — once, at
//! decode time (D8: no re-parsing in view hot paths).

use serde::{Deserialize, Serialize};

/// A relay reference. NIP-65 / kind-10002 relay entries are
/// `["r", <url>, "read"|"write"?]`; the optional third column is the
/// read/write marker. We preserve it verbatim (`Option<String>`) rather than
/// collapsing to a bare URL so kind 10002 round-trips losslessly — silent
/// coercion is the §9 anti-pattern this crate is built to avoid.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RelayEntry {
    /// Relay websocket URL (tag column 1).
    pub url: String,
    /// `"read"` / `"write"` marker (tag column 2) when present; `None` means
    /// the relay is both read and write per NIP-65.
    pub marker: Option<String>,
}

/// Public, typed projection of a list's tag entries. Private entries live
/// encrypted in the event content and are intentionally NOT represented here
/// (see [`crate::decode::ListRecord::encrypted_payload`]).
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListItems {
    /// `p` tag values — pubkeys (column 1 only; relay hints in column 2 are
    /// preserved in the raw `tags` and intentionally not re-modelled here).
    pub pubkeys: Vec<String>,
    /// `e` tag values — event ids.
    pub events: Vec<String>,
    /// `a` tag values — `kind:pubkey:d` address coordinates (verbatim).
    pub addresses: Vec<String>,
    /// `t` tag values — hashtags.
    pub hashtags: Vec<String>,
    /// `r` and `relay` tag values — relay URLs with optional read/write marker.
    pub relays: Vec<RelayEntry>,
    /// `word` tag values — muted words (mute-list specific, but extracted
    /// uniformly).
    pub words: Vec<String>,
}

impl ListItems {
    /// Extract every public list entry from `tags`. Order within each vector
    /// follows tag order in the event (stable, deterministic — diff-friendly
    /// for SwiftUI). Tags with no value column are skipped, never panicking.
    #[must_use]
    pub fn from_tags(tags: &[Vec<String>]) -> Self {
        let mut items = Self::default();
        for tag in tags {
            let Some(key) = tag.first().map(String::as_str) else {
                continue;
            };
            match key {
                "p" => push_value(&mut items.pubkeys, tag),
                "e" => push_value(&mut items.events, tag),
                "a" => push_value(&mut items.addresses, tag),
                "t" => push_value(&mut items.hashtags, tag),
                "word" => push_value(&mut items.words, tag),
                "r" | "relay" => {
                    if let Some(url) = tag.get(1) {
                        items.relays.push(RelayEntry {
                            url: url.clone(),
                            marker: tag.get(2).cloned(),
                        });
                    }
                }
                _ => {}
            }
        }
        items
    }
}

/// Push the value column (index 1) of `tag` onto `dst` if present.
fn push_value(dst: &mut Vec<String>, tag: &[String]) {
    if let Some(value) = tag.get(1) {
        dst.push(value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_each_item_kind() {
        let tags = vec![
            vec!["p".into(), "pk1".into()],
            vec!["e".into(), "ev1".into()],
            vec!["a".into(), "30000:pk:d".into()],
            vec!["t".into(), "rust".into()],
            vec!["word".into(), "spam".into()],
            vec!["r".into(), "wss://relay.one".into(), "write".into()],
            vec!["relay".into(), "wss://relay.two".into()],
        ];
        let items = ListItems::from_tags(&tags);
        assert_eq!(items.pubkeys, vec!["pk1"]);
        assert_eq!(items.events, vec!["ev1"]);
        assert_eq!(items.addresses, vec!["30000:pk:d"]);
        assert_eq!(items.hashtags, vec!["rust"]);
        assert_eq!(items.words, vec!["spam"]);
        assert_eq!(
            items.relays,
            vec![
                RelayEntry {
                    url: "wss://relay.one".into(),
                    marker: Some("write".into())
                },
                RelayEntry {
                    url: "wss://relay.two".into(),
                    marker: None
                },
            ]
        );
    }

    #[test]
    fn skips_value_less_tags_without_panic() {
        let tags = vec![vec!["p".into()], vec!["r".into()], vec!["e".into()]];
        let items = ListItems::from_tags(&tags);
        assert!(items.pubkeys.is_empty());
        assert!(items.relays.is_empty());
        assert!(items.events.is_empty());
    }

    #[test]
    fn ignores_unknown_tag_keys() {
        let tags = vec![
            vec!["d".into(), "ident".into()],
            vec!["title".into(), "My List".into()],
            vec!["custom".into(), "x".into()],
        ];
        let items = ListItems::from_tags(&tags);
        assert_eq!(items, ListItems::default());
    }
}
