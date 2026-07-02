//! [`FeedAuthorRefs`] — the set of author keys a feed projection will RENDER.
//!
//! ADR-0070 D7 (#1671): the coverage hole is "a row carries an author the shell
//! forgot to resolve, so it renders a blank avatar." This trait lets the kernel
//! auto-resolve every author a feed will render, through the SAME `resolve_ref`
//! path, by extracting that author set from the engine's projection surface
//! ([`RootFeedSnapshot`]).
//!
//! The engine is substrate-generic (D0): it does not know what an "author" is
//! protocol-wise. So the extraction is split — the engine-generic shape
//! ([`FeedAuthorRefs`] over [`RootFeedSnapshot`]) folds over the cards and
//! attributions, delegating to two per-instance traits ([`CardAuthors`] /
//! [`AttributionAuthors`]) the protocol crate implements for its concrete `C`
//! and `A`. No protocol-named token appears here — only the structural fact
//! "a render row exposes some author keys."

use std::collections::BTreeSet;

use serde::Serialize;

use super::card::{RootCard, RootFeedSnapshot};

/// A render card's contributed author keys (D7).
///
/// Implemented by a protocol crate for its concrete card type. A card may carry
/// more than one rendered author (e.g. a primary author plus a re-surfacing
/// attribution embedded on the card), so this yields a list.
pub trait CardAuthors {
    /// Author keys this card will RENDER. Raw protocol keys (no display
    /// encoding) — the same keys the kernel resolves.
    fn rendered_author_keys(&self) -> Vec<String>;
}

/// An attribution payload's contributed author key (D7).
///
/// Implemented by a protocol crate for its concrete attribution type. Each
/// attribution names exactly one rendered author.
pub trait AttributionAuthors {
    /// Author key this attribution will RENDER.
    fn rendered_author_key(&self) -> String;
}

/// The set of author keys a feed projection will RENDER for its visible rows
/// (D7). The kernel auto-resolves exactly this set per snapshot tick, so a
/// shell cannot silently forget to resolve a visible author.
pub trait FeedAuthorRefs {
    /// Every author key carried by the visible-window rows: each card's
    /// [`CardAuthors::rendered_author_keys`] plus each attribution's
    /// [`AttributionAuthors::rendered_author_key`]. Deduped (a `BTreeSet`), so
    /// one author appearing on N rows costs ONE resolver slot.
    fn visible_author_keys(&self) -> BTreeSet<String>;
}

impl<C, A> FeedAuthorRefs for RootFeedSnapshot<C, A>
where
    C: Clone + Serialize + CardAuthors,
    A: Clone + Serialize + AttributionAuthors,
{
    fn visible_author_keys(&self) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for card in &self.cards {
            collect_card_authors(card, &mut keys);
        }
        keys
    }
}

/// Fold one row (card + its attributions) into `keys`. Extracted so the
/// `()`-attribution feed (the flat author/thread feed, which carries no
/// attribution list) can reuse the card half via the blanket impl below.
fn collect_card_authors<C, A>(card: &RootCard<C, A>, keys: &mut BTreeSet<String>)
where
    C: Clone + Serialize + CardAuthors,
    A: Clone + Serialize + AttributionAuthors,
{
    for key in card.card.rendered_author_keys() {
        if !key.is_empty() {
            keys.insert(key);
        }
    }
    for attribution in &card.attribution {
        let key = attribution.rendered_author_key();
        if !key.is_empty() {
            keys.insert(key);
        }
    }
}

/// The flat author/thread feed carries no attribution payload (`A = ()`); only
/// the card half contributes. A unit attribution names no author.
impl AttributionAuthors for () {
    fn rendered_author_key(&self) -> String {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, serde::Serialize)]
    struct FakeCard {
        primary: String,
        embedded: Option<String>,
    }

    impl CardAuthors for FakeCard {
        fn rendered_author_keys(&self) -> Vec<String> {
            let mut v = vec![self.primary.clone()];
            if let Some(e) = &self.embedded {
                v.push(e.clone());
            }
            v
        }
    }

    #[derive(Clone, serde::Serialize)]
    struct FakeAttr {
        author: String,
    }

    impl AttributionAuthors for FakeAttr {
        fn rendered_author_key(&self) -> String {
            self.author.clone()
        }
    }

    fn row(primary: &str, embedded: Option<&str>, attrs: &[&str]) -> RootCard<FakeCard, FakeAttr> {
        RootCard {
            card: FakeCard {
                primary: primary.to_string(),
                embedded: embedded.map(str::to_string),
            },
            attribution: attrs
                .iter()
                .map(|a| FakeAttr {
                    author: a.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn extracts_primary_embedded_and_attribution_authors() {
        let snapshot = RootFeedSnapshot {
            cards: vec![
                row("alice", Some("reposter"), &["replier1", "replier2"]),
                row("bob", None, &[]),
            ],
            page: None,
            metrics: None,
        };
        let keys = snapshot.visible_author_keys();
        let expected: BTreeSet<String> = ["alice", "reposter", "replier1", "replier2", "bob"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn dedupes_one_author_across_many_rows() {
        let snapshot = RootFeedSnapshot {
            cards: vec![
                row("alice", None, &["alice"]),
                row("alice", Some("alice"), &[]),
            ],
            page: None,
            metrics: None,
        };
        let keys = snapshot.visible_author_keys();
        assert_eq!(keys.len(), 1, "one author on N rows ⇒ one resolver key");
        assert!(keys.contains("alice"));
    }

    #[test]
    fn empty_keys_are_skipped() {
        let snapshot = RootFeedSnapshot {
            cards: vec![row("", None, &[""])],
            page: None,
            metrics: None,
        };
        assert!(snapshot.visible_author_keys().is_empty());
    }

    #[test]
    fn unit_attribution_contributes_no_author() {
        let card: RootCard<FakeCard, ()> = RootCard {
            card: FakeCard {
                primary: "alice".to_string(),
                embedded: None,
            },
            attribution: vec![(), ()],
        };
        let snapshot = RootFeedSnapshot {
            cards: vec![card],
            page: None,
            metrics: None,
        };
        let keys = snapshot.visible_author_keys();
        assert_eq!(keys.len(), 1);
        assert!(keys.contains("alice"));
    }
}
