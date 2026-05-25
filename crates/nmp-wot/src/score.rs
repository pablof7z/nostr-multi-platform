use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::interest::{is_hex_pubkey, KIND_CONTACT_LIST, KIND_MUTE_LIST};

const SELF_SCORE: i32 = 1_000;
const DIRECT_FOLLOW_SCORE: i32 = 100;
const SECOND_DEGREE_SCORE: i32 = 10;
const SELF_MUTE_SCORE: i32 = -1_000;
const FOLLOWED_MUTE_SCORE: i32 = -25;
const AUTO_HIDE_SCORE: i32 = -50;

/// Local client-side follow/mute graph used for web-of-trust decisions.
#[derive(Default, Debug)]
pub struct WotGraph {
    follows_by_author: BTreeMap<String, BTreeSet<String>>,
    mutes_by_author: BTreeMap<String, BTreeSet<String>>,
}

/// Result of scoring one candidate from one viewer's perspective.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TrustDecision {
    /// Signed trust score. Positive sorts earlier; sufficiently negative can
    /// be hidden by the caller.
    pub score: i32,
    /// True when the local policy recommends hiding the candidate by default.
    pub hide: bool,
    /// Human-readable reason bucket for diagnostics and tests.
    pub reason: &'static str,
}

impl WotGraph {
    /// Ingest a kind:3 contact-list event.
    pub fn ingest_follow_list(&mut self, author: &str, tags: &[Vec<String>]) {
        if !is_hex_pubkey(author) {
            return;
        }
        let follows = p_tags(tags);
        self.follows_by_author.insert(author.to_string(), follows);
    }

    /// Ingest a kind:10000 mute-list event.
    pub fn ingest_mute_list(&mut self, author: &str, tags: &[Vec<String>]) {
        if !is_hex_pubkey(author) {
            return;
        }
        let mutes = p_tags(tags);
        self.mutes_by_author.insert(author.to_string(), mutes);
    }

    /// Ingest a kernel event when it belongs to the WOT graph.
    pub fn ingest_event(&mut self, author: &str, kind: u32, tags: &[Vec<String>]) {
        match kind {
            KIND_CONTACT_LIST => self.ingest_follow_list(author, tags),
            KIND_MUTE_LIST => self.ingest_mute_list(author, tags),
            _ => {}
        }
    }

    /// Score `candidate` from `viewer`'s perspective.
    #[must_use]
    pub fn score(&self, viewer: &str, candidate: &str) -> TrustDecision {
        if viewer == candidate {
            return TrustDecision {
                score: SELF_SCORE,
                hide: false,
                reason: "self",
            };
        }

        let viewer_follows = self.follows_by_author.get(viewer);
        let viewer_mutes = self.mutes_by_author.get(viewer);
        if viewer_mutes.is_some_and(|mutes| mutes.contains(candidate)) {
            return TrustDecision {
                score: SELF_MUTE_SCORE,
                hide: true,
                reason: "muted-by-self",
            };
        }

        let direct = viewer_follows.is_some_and(|follows| follows.contains(candidate));
        if direct {
            return TrustDecision {
                score: DIRECT_FOLLOW_SCORE,
                hide: false,
                reason: "direct-follow",
            };
        }

        let mut score = 0;
        let mut second_degree = 0;
        let mut followed_mutes = 0;
        if let Some(follows) = viewer_follows {
            for followed in follows {
                if self
                    .follows_by_author
                    .get(followed)
                    .is_some_and(|their_follows| their_follows.contains(candidate))
                {
                    second_degree += 1;
                    score += SECOND_DEGREE_SCORE;
                }
                if self
                    .mutes_by_author
                    .get(followed)
                    .is_some_and(|their_mutes| their_mutes.contains(candidate))
                {
                    followed_mutes += 1;
                    score += FOLLOWED_MUTE_SCORE;
                }
            }
        }

        let hide = score <= AUTO_HIDE_SCORE;
        let reason = if hide {
            "muted-by-followed"
        } else if second_degree > 0 {
            "second-degree"
        } else if followed_mutes > 0 {
            "weak-negative"
        } else {
            "unknown"
        };

        TrustDecision {
            score,
            hide,
            reason,
        }
    }

    /// Count authors with known contact lists.
    #[must_use]
    pub fn follow_author_count(&self) -> usize {
        self.follows_by_author.len()
    }

    /// Count authors with known mute lists.
    #[must_use]
    pub fn mute_author_count(&self) -> usize {
        self.mutes_by_author.len()
    }
}

fn p_tags(tags: &[Vec<String>]) -> BTreeSet<String> {
    tags.iter()
        .filter_map(|tag| {
            if tag.first().is_some_and(|name| name == "p") {
                tag.get(1).filter(|value| is_hex_pubkey(value)).cloned()
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn author(n: u16) -> String {
        format!("{n:064x}")
    }

    fn p(pubkey: &str) -> Vec<String> {
        vec!["p".to_string(), pubkey.to_string()]
    }

    #[test]
    fn direct_follows_beat_second_degree() {
        let me = author(1);
        let direct = author(2);
        let indirect = author(3);

        let mut graph = WotGraph::default();
        graph.ingest_follow_list(&me, &[p(&direct)]);
        graph.ingest_follow_list(&direct, &[p(&indirect)]);

        assert_eq!(graph.score(&me, &direct).score, DIRECT_FOLLOW_SCORE);
        assert_eq!(graph.score(&me, &indirect).score, SECOND_DEGREE_SCORE);
    }

    #[test]
    fn many_followed_mutes_hide_unfollowed_candidate() {
        let me = author(1);
        let candidate = author(9);
        let alice = author(2);
        let bob = author(3);

        let mut graph = WotGraph::default();
        graph.ingest_follow_list(&me, &[p(&alice), p(&bob)]);
        graph.ingest_mute_list(&alice, &[p(&candidate)]);
        graph.ingest_mute_list(&bob, &[p(&candidate)]);

        let decision = graph.score(&me, &candidate);
        assert_eq!(decision.score, -50);
        assert!(decision.hide);
        assert_eq!(decision.reason, "muted-by-followed");
    }

    #[test]
    fn self_mute_overrides_everything() {
        let me = author(1);
        let candidate = author(2);

        let mut graph = WotGraph::default();
        graph.ingest_follow_list(&me, &[p(&candidate)]);
        graph.ingest_mute_list(&me, &[p(&candidate)]);

        assert_eq!(graph.score(&me, &candidate).reason, "muted-by-self");
    }
}
