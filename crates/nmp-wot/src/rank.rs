use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::graph::Pubkey;
use crate::score::{TrustIndex, TrustScore};

/// Pubkey plus its materialized trust score.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScoredPubkey {
    pub pubkey: Pubkey,
    pub trust: TrustScore,
}

impl TrustIndex {
    #[must_use]
    pub fn rank_pubkeys<I, S>(&self, pubkeys: I) -> Vec<ScoredPubkey>
    where
        I: IntoIterator<Item = S>,
        S: Into<Pubkey>,
    {
        let mut scored = pubkeys
            .into_iter()
            .map(|pubkey| {
                let pubkey = pubkey.into();
                let trust = self.score_for(&pubkey);
                ScoredPubkey { pubkey, trust }
            })
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| self.compare_scores(&a.trust, &b.trust));
        scored
    }

    pub fn sort_by_author<T, F>(&self, items: &mut [T], author: F)
    where
        F: Fn(&T) -> &str,
    {
        items.sort_by(|a, b| self.compare_authors(author(a), author(b)));
    }

    #[must_use]
    pub fn visible_items<T, F>(&self, items: impl IntoIterator<Item = T>, author: F) -> Vec<T>
    where
        F: Fn(&T) -> &str,
    {
        items
            .into_iter()
            .filter(|item| !self.should_hide(author(item)))
            .collect()
    }

    #[must_use]
    pub fn compare_authors(&self, left: &str, right: &str) -> Ordering {
        self.compare_scores(&self.score_for(left), &self.score_for(right))
    }

    fn compare_scores(&self, left: &TrustScore, right: &TrustScore) -> Ordering {
        left.decision
            .hide_rank()
            .cmp(&right.decision.hide_rank())
            .then_with(|| right.followed_by_viewer.cmp(&left.followed_by_viewer))
            .then_with(|| right.score.total_cmp(&left.score))
            .then_with(|| distance_rank(left).cmp(&distance_rank(right)))
            .then_with(|| left.pubkey.cmp(&right.pubkey))
    }
}

fn distance_rank(score: &TrustScore) -> u8 {
    score.distance.unwrap_or(u8::MAX)
}

trait DecisionRank {
    fn hide_rank(self) -> u8;
}

impl DecisionRank for crate::score::TrustDecision {
    fn hide_rank(self) -> u8 {
        match self {
            Self::Show => 0,
            Self::Hide => 1,
        }
    }
}
