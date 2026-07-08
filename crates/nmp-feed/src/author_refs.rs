//! [`FeedAuthorRefs`] — the set of refs a feed projection DECLARES for
//! auto-resolution.
//!
//! ADR-0070 D7 (#1671): the coverage hole is "a row carries an author (or a
//! render target) the shell forgot to resolve, so it renders blank." This trait
//! lets the kernel auto-resolve every ref a feed will render, through the SAME
//! `resolve_ref` declaration path, by extracting that ref set from the engine's
//! projection surface ([`RootFeedSnapshot`]).
//!
//! DECLARES, NEVER DEMANDS. The feed exposes the refs its visible rows carry —
//! author keys AND render-target ids (a repost's target, a quote's target). The
//! kernel's D7 lane declares them for resolution. The feed itself NEVER calls
//! `resolve_ref`: doing so would tie target liveness to window churn (refcount
//! coupling). #3082 point 5.
//!
//! The feed substrate is kind-agnostic (D0): it does not know what an "author"
//! or a "target" is protocol-wise. The per-instance [`CardAuthors`] trait
//! (implemented by the concrete card type) yields the raw keys; this module only
//! folds them over the visible window.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::snapshot::{RootCard, RootFeedSnapshot};

/// A render card's contributed refs (D7). Implemented by the concrete card
/// type. Raw protocol keys/ids — no display encoding.
pub trait CardAuthors {
    /// Author keys this card will RENDER (primary author, plus e.g. a
    /// re-surfacing reposter). Deduped downstream.
    fn rendered_author_keys(&self) -> Vec<String>;

    /// Render-target ref ids this card DECLARES for auto-resolution (e.g. a
    /// repost/quote target event id). Default: none.
    ///
    /// These are DECLARED through the same D7 lane as authors; the feed never
    /// demands them via `resolve_ref` (refcount coupling; #3082 point 5).
    fn rendered_target_refs(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The set of refs a feed projection DECLARES for its visible rows (D7). The
/// kernel auto-resolves exactly this set per snapshot tick, so a shell cannot
/// silently forget to resolve a visible author or render target.
pub trait FeedAuthorRefs {
    /// Every ref key carried by the visible-window rows: each card's
    /// [`CardAuthors::rendered_author_keys`] plus its
    /// [`CardAuthors::rendered_target_refs`]. Deduped (a `BTreeSet`).
    fn visible_author_keys(&self) -> BTreeSet<String>;
}

impl<C> FeedAuthorRefs for RootFeedSnapshot<C>
where
    C: Clone + Serialize + CardAuthors,
{
    fn visible_author_keys(&self) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for card in &self.cards {
            collect_card_refs(card, &mut keys);
        }
        keys
    }
}

fn collect_card_refs<C>(card: &RootCard<C>, keys: &mut BTreeSet<String>)
where
    C: Clone + Serialize + CardAuthors,
{
    for key in card.card.rendered_author_keys() {
        if !key.is_empty() {
            keys.insert(key);
        }
    }
    for target in card.card.rendered_target_refs() {
        if !target.is_empty() {
            keys.insert(target);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, serde::Serialize)]
    struct FakeCard {
        primary: String,
        embedded: Option<String>,
        target: Option<String>,
    }

    impl CardAuthors for FakeCard {
        fn rendered_author_keys(&self) -> Vec<String> {
            let mut v = vec![self.primary.clone()];
            if let Some(e) = &self.embedded {
                v.push(e.clone());
            }
            v
        }

        fn rendered_target_refs(&self) -> Vec<String> {
            self.target.clone().into_iter().collect()
        }
    }

    fn row(primary: &str, embedded: Option<&str>, target: Option<&str>) -> RootCard<FakeCard> {
        RootCard {
            card: FakeCard {
                primary: primary.to_string(),
                embedded: embedded.map(str::to_string),
                target: target.map(str::to_string),
            },
        }
    }

    #[test]
    fn extracts_authors_and_target_refs() {
        let snapshot = RootFeedSnapshot {
            cards: vec![row("alice", Some("reposter"), Some("target1")), row("bob", None, None)],
            page: None,
            metrics: None,
        };
        let keys = snapshot.visible_author_keys();
        let expected: BTreeSet<String> = ["alice", "reposter", "target1", "bob"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn dedupes_and_skips_empty() {
        let snapshot = RootFeedSnapshot {
            cards: vec![row("alice", None, None), row("alice", Some("alice"), None), row("", None, None)],
            page: None,
            metrics: None,
        };
        let keys = snapshot.visible_author_keys();
        assert_eq!(keys.len(), 1);
        assert!(keys.contains("alice"));
    }
}
