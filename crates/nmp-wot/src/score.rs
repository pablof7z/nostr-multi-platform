use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::graph::{is_hex_pubkey, Pubkey, SignalGraph};

/// Tuning for a bounded client-side trust calculation.
#[derive(Clone, Debug, PartialEq)]
pub struct TrustConfig {
    /// Maximum follow hops from the viewer, including direct follows at depth 1.
    pub max_depth: u8,
    /// Positive trust retained each time the score crosses a follow edge.
    pub follow_decay: f64,
    /// Score assigned to authors the viewer follows directly.
    pub direct_follow_score: f64,
    /// Negative weight contributed by a trusted author's public mute.
    pub community_mute_penalty: f64,
    /// Score at or below this value is hidden unless directly followed.
    pub hide_threshold: f64,
}

impl Default for TrustConfig {
    fn default() -> Self {
        Self {
            max_depth: 3,
            follow_decay: 0.65,
            direct_follow_score: 1.0,
            community_mute_penalty: 0.35,
            hide_threshold: -0.25,
        }
    }
}

/// Visibility decision derived from the score and the viewer's direct signals.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TrustDecision {
    Show,
    Hide,
}

/// Score explanation for one pubkey from one viewer's perspective.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrustScore {
    pub pubkey: Pubkey,
    pub score: f64,
    pub positive: f64,
    pub negative: f64,
    pub distance: Option<u8>,
    pub followed_by_viewer: bool,
    pub muted_by_viewer: bool,
    pub muted_by_trusted_count: usize,
    pub decision: TrustDecision,
}

/// Materialized trust index for one viewer.
#[derive(Clone, Debug)]
pub struct TrustIndex {
    viewer: Pubkey,
    config: TrustConfig,
    scores: BTreeMap<Pubkey, TrustScore>,
}

impl SignalGraph {
    #[must_use]
    pub fn compute_trust(&self, viewer: &str, config: TrustConfig) -> TrustIndex {
        if !is_hex_pubkey(viewer) {
            return TrustIndex {
                viewer: viewer.to_string(),
                config,
                scores: BTreeMap::new(),
            };
        }

        let direct_follows = self.follows_of(viewer).cloned().unwrap_or_default();
        let direct_mutes = self.mutes_of(viewer).cloned().unwrap_or_default();
        let (positive, distance) = self.positive_scores(viewer, &direct_follows, &config);
        let (negative, muted_by_trusted_count) =
            self.negative_scores(viewer, &direct_follows, &direct_mutes, &positive, &config);

        let mut pubkeys = BTreeSet::new();
        pubkeys.insert(viewer.to_string());
        pubkeys.extend(positive.keys().cloned());
        pubkeys.extend(negative.keys().cloned());
        pubkeys.extend(direct_follows.iter().cloned());
        pubkeys.extend(direct_mutes.iter().cloned());

        let scores = pubkeys
            .into_iter()
            .map(|pubkey| {
                let followed_by_viewer = direct_follows.contains(&pubkey);
                let muted_by_viewer = direct_mutes.contains(&pubkey);
                let positive = positive.get(&pubkey).copied().unwrap_or(0.0);
                let negative = negative.get(&pubkey).copied().unwrap_or(0.0);
                let score = positive - negative;
                let decision =
                    if muted_by_viewer || (!followed_by_viewer && score <= config.hide_threshold) {
                        TrustDecision::Hide
                    } else {
                        TrustDecision::Show
                    };
                let trust = TrustScore {
                    pubkey: pubkey.clone(),
                    score,
                    positive,
                    negative,
                    distance: distance.get(&pubkey).copied(),
                    followed_by_viewer,
                    muted_by_viewer,
                    muted_by_trusted_count: muted_by_trusted_count
                        .get(&pubkey)
                        .copied()
                        .unwrap_or(0),
                    decision,
                };
                (pubkey, trust)
            })
            .collect();

        TrustIndex {
            viewer: viewer.to_string(),
            config,
            scores,
        }
    }

    fn positive_scores(
        &self,
        viewer: &str,
        direct_follows: &BTreeSet<Pubkey>,
        config: &TrustConfig,
    ) -> (BTreeMap<Pubkey, f64>, BTreeMap<Pubkey, u8>) {
        let mut positive = BTreeMap::new();
        let mut distance = BTreeMap::new();
        positive.insert(viewer.to_string(), config.direct_follow_score);
        distance.insert(viewer.to_string(), 0);

        let mut queue = VecDeque::new();
        for follow in direct_follows {
            positive.insert(follow.clone(), config.direct_follow_score);
            distance.insert(follow.clone(), 1);
            queue.push_back(follow.clone());
        }

        while let Some(source) = queue.pop_front() {
            let source_depth = distance.get(&source).copied().unwrap_or(config.max_depth);
            if source_depth >= config.max_depth {
                continue;
            }
            let Some(follows) = self.follows_of(&source) else {
                continue;
            };
            if follows.is_empty() {
                continue;
            }

            let source_score = positive.get(&source).copied().unwrap_or(0.0);
            let contribution = source_score * config.follow_decay / (follows.len() as f64).sqrt();
            for target in follows {
                let next_depth = source_depth + 1;
                if distance.get(target).is_none_or(|known| next_depth < *known) {
                    distance.insert(target.clone(), next_depth);
                    queue.push_back(target.clone());
                }
                let entry = positive.entry(target.clone()).or_insert(0.0);
                *entry = (*entry + contribution).min(config.direct_follow_score);
            }
        }

        (positive, distance)
    }

    fn negative_scores(
        &self,
        viewer: &str,
        direct_follows: &BTreeSet<Pubkey>,
        direct_mutes: &BTreeSet<Pubkey>,
        positive: &BTreeMap<Pubkey, f64>,
        config: &TrustConfig,
    ) -> (BTreeMap<Pubkey, f64>, BTreeMap<Pubkey, usize>) {
        let mut negative = BTreeMap::new();
        let mut muted_by_trusted_count = BTreeMap::new();
        for target in direct_mutes {
            negative.insert(target.clone(), config.direct_follow_score);
        }

        for (muter, muter_score) in positive {
            if muter == viewer || *muter_score <= 0.0 {
                continue;
            }
            let Some(mutes) = self.mutes_of(muter) else {
                continue;
            };
            for target in mutes {
                if target == viewer || direct_follows.contains(target) {
                    continue;
                }
                *negative.entry(target.clone()).or_insert(0.0) +=
                    muter_score * config.community_mute_penalty;
                *muted_by_trusted_count.entry(target.clone()).or_insert(0) += 1;
            }
        }

        (negative, muted_by_trusted_count)
    }
}

impl TrustIndex {
    #[must_use]
    pub fn viewer(&self) -> &str {
        &self.viewer
    }

    #[must_use]
    pub fn config(&self) -> &TrustConfig {
        &self.config
    }

    #[must_use]
    pub fn score_for(&self, pubkey: &str) -> TrustScore {
        self.scores
            .get(pubkey)
            .cloned()
            .unwrap_or_else(|| TrustScore {
                pubkey: pubkey.to_string(),
                score: 0.0,
                positive: 0.0,
                negative: 0.0,
                distance: None,
                followed_by_viewer: false,
                muted_by_viewer: false,
                muted_by_trusted_count: 0,
                decision: TrustDecision::Show,
            })
    }

    #[must_use]
    pub fn should_hide(&self, pubkey: &str) -> bool {
        self.score_for(pubkey).decision == TrustDecision::Hide
    }

    #[must_use]
    pub fn scores(&self) -> impl Iterator<Item = &TrustScore> {
        self.scores.values()
    }
}
