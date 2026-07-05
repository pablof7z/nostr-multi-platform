use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::interest::{is_hex_pubkey, KIND_CONTACT_LIST, KIND_MUTE_LIST};
use nmp_nip51::mute_pubkeys_from_tags;

/// Trust score awarded to the viewer for itself. Always sorts first and is
/// never hidden.
pub const SELF_SCORE: i32 = 1_000;
/// Trust score for a pubkey the viewer directly follows (kind:3 `p` tag).
pub const DIRECT_FOLLOW_SCORE: i32 = 100;
/// Trust contribution per follow-of-a-follow edge (summed across the viewer's
/// follows who also follow the candidate).
pub const SECOND_DEGREE_SCORE: i32 = 10;
/// Trust score for a pubkey the viewer has muted (kind:10000 `p` tag). Always
/// hides regardless of any caller threshold.
pub const SELF_MUTE_SCORE: i32 = -1_000;
/// Trust contribution per "muted by someone the viewer follows" edge.
pub const FOLLOWED_MUTE_SCORE: i32 = -25;
/// Default auto-hide threshold used by [`WotGraph::score`]: a candidate whose
/// score is at or below this value is hidden by the default policy.
///
/// Exposed as NMP-owned policy so app crates can build stricter or looser
/// presets (passed to [`WotGraph::score_with_minimum_score`] /
/// [`WotGraph::batch_score_with_minimum_score`]) by referencing the authoritative
/// tier constants above rather than cloning magic numbers locally.
pub const DEFAULT_AUTO_HIDE_SCORE: i32 = -50;

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
    /// True when this decision was computed from a caller-supplied
    /// `fallback_root` instead of the actual viewer, because the viewer's own
    /// graph was cold (see [`WotGraph::has_follows`] and
    /// [`WotGraph::score_rooted`]). Always `false` for [`WotGraph::score`] and
    /// [`WotGraph::score_with_minimum_score`], which never consult a fallback
    /// root. Callers can use this to label a result "suggested via a trust
    /// seed" versus "trusted by people you follow".
    pub rooted_at_fallback: bool,
}

/// Small read-model diagnostic for callers that need to distinguish an empty
/// graph from a populated graph whose candidate is merely unknown.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WotGraphStats {
    /// Distinct authors with known contact lists.
    pub follow_authors: usize,
    /// Distinct authors with known mute lists.
    pub mute_authors: usize,
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
    ///
    /// Delegates pubkey extraction to [`nmp_nip51::mute_pubkeys_from_tags`],
    /// the canonical shared parser for kind:10000 `p` tags (GitHub issue #964
    /// consolidation). `MuteListProjection` (timeline suppression) drives
    /// through the same function, so both consumers always ingest identical
    /// pubkey sets from the same event.
    pub fn ingest_mute_list(&mut self, author: &str, tags: &[Vec<String>]) {
        if !is_hex_pubkey(author) {
            return;
        }
        let mutes = mute_pubkeys_from_tags(tags);
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
        let scored = self.score_parts(viewer, candidate);
        let hide = scored.self_muted || scored.score <= DEFAULT_AUTO_HIDE_SCORE;
        TrustDecision {
            score: scored.score,
            hide,
            reason: scored.reason,
            rooted_at_fallback: false,
        }
    }

    /// Score `candidate`, hiding anything below `minimum_score`.
    ///
    /// This is the configurable-threshold variant apps use for product presets
    /// such as "close" or "open". A self-mute always hides, regardless of the
    /// threshold; otherwise the threshold is an inclusive pass floor
    /// (`score < minimum_score` hides).
    #[must_use]
    pub fn score_with_minimum_score(
        &self,
        viewer: &str,
        candidate: &str,
        minimum_score: i32,
    ) -> TrustDecision {
        let scored = self.score_parts(viewer, candidate);
        let hide = scored.self_muted || scored.score < minimum_score;
        TrustDecision {
            score: scored.score,
            hide,
            reason: scored.reason,
            rooted_at_fallback: false,
        }
    }

    /// Score a batch of candidates using the default hide policy.
    #[must_use]
    pub fn batch_score<'a, I>(&self, viewer: &str, candidates: I) -> Vec<TrustDecision>
    where
        I: IntoIterator<Item = &'a str>,
    {
        candidates
            .into_iter()
            .map(|candidate| self.score(viewer, candidate))
            .collect()
    }

    /// Score a batch of candidates with a configurable minimum-score floor.
    #[must_use]
    pub fn batch_score_with_minimum_score<'a, I>(
        &self,
        viewer: &str,
        candidates: I,
        minimum_score: i32,
    ) -> Vec<TrustDecision>
    where
        I: IntoIterator<Item = &'a str>,
    {
        candidates
            .into_iter()
            .map(|candidate| self.score_with_minimum_score(viewer, candidate, minimum_score))
            .collect()
    }

    /// True iff `viewer` has an ingested, non-empty follow set.
    ///
    /// Used to detect the WoT cold-start case: a viewer with no follows
    /// (never ingested a kind:3, or ingested an empty one) scores every
    /// candidate at 0, which sits at or below any sane minimum floor and
    /// hides everything. [`score_rooted`](Self::score_rooted) and
    /// [`score_rooted_with_minimum_score`](Self::score_rooted_with_minimum_score)
    /// use this to decide whether to reroute scoring through a
    /// `fallback_root` instead.
    #[must_use]
    pub fn has_follows(&self, viewer: &str) -> bool {
        self.follows_by_author
            .get(viewer)
            .is_some_and(|follows| !follows.is_empty())
    }

    /// Resolve the pubkey to actually score from: `viewer` when it has its
    /// own follow graph, otherwise `fallback_root` (falling back to `viewer`
    /// itself when no fallback root is given, which reproduces today's
    /// behavior exactly). Returns `(effective_root, rooted_at_fallback)`.
    fn effective_root<'a>(
        &self,
        viewer: &'a str,
        fallback_root: Option<&'a str>,
    ) -> (&'a str, bool) {
        if self.has_follows(viewer) {
            (viewer, false)
        } else {
            match fallback_root {
                Some(root) => (root, true),
                None => (viewer, false),
            }
        }
    }

    /// Score `candidate` from `viewer`'s perspective, falling back to scoring
    /// from `fallback_root` when `viewer`'s own follow graph is cold.
    ///
    /// This is a GENERAL WoT primitive: it exists in `nmp-wot` (not in any one
    /// consumer) because the cold-start problem — a brand-new or
    /// zero-follows viewer scoring every candidate at 0 and having everything
    /// hidden — hits every WoT consumer identically (mint discovery, feeds,
    /// spam filtering, nutzap-sender trust). `fallback_root` is a caller-owned
    /// bootstrap trust seed (e.g. an app's curated pubkey, or a well-known
    /// community root); this crate does not choose one.
    ///
    /// This is distinct from this crate's own "bootstrap" runtime
    /// ([`crate::runtime::WotBootstrapRuntime`]), which means fetching the
    /// *active account's own* follow/mute lists from relays. `fallback_root`
    /// scoring instead substitutes a *different* pubkey's already-ingested
    /// graph as the root when the viewer has none of their own yet.
    ///
    /// Reuses [`score`](Self::score) unchanged for the actual scoring math —
    /// only the pubkey used as the root of the graph walk changes. Passing
    /// `fallback_root: None` reproduces today's [`score`](Self::score)
    /// behavior byte-for-byte, including for a cold viewer.
    #[must_use]
    pub fn score_rooted(
        &self,
        viewer: &str,
        fallback_root: Option<&str>,
        candidate: &str,
    ) -> TrustDecision {
        let (root, rooted_at_fallback) = self.effective_root(viewer, fallback_root);
        let mut decision = self.score(root, candidate);
        decision.rooted_at_fallback = rooted_at_fallback;
        decision
    }

    /// [`score_rooted`](Self::score_rooted) with a caller-supplied minimum-score
    /// floor, mirroring [`score_with_minimum_score`](Self::score_with_minimum_score).
    #[must_use]
    pub fn score_rooted_with_minimum_score(
        &self,
        viewer: &str,
        fallback_root: Option<&str>,
        candidate: &str,
        minimum_score: i32,
    ) -> TrustDecision {
        let (root, rooted_at_fallback) = self.effective_root(viewer, fallback_root);
        let mut decision = self.score_with_minimum_score(root, candidate, minimum_score);
        decision.rooted_at_fallback = rooted_at_fallback;
        decision
    }

    /// Batch variant of [`score_rooted`](Self::score_rooted).
    #[must_use]
    pub fn batch_score_rooted<'a, I>(
        &self,
        viewer: &str,
        fallback_root: Option<&str>,
        candidates: I,
    ) -> Vec<TrustDecision>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let (root, rooted_at_fallback) = self.effective_root(viewer, fallback_root);
        self.batch_score(root, candidates)
            .into_iter()
            .map(|mut decision| {
                decision.rooted_at_fallback = rooted_at_fallback;
                decision
            })
            .collect()
    }

    /// Batch variant of [`score_rooted_with_minimum_score`](Self::score_rooted_with_minimum_score).
    #[must_use]
    pub fn batch_score_rooted_with_minimum_score<'a, I>(
        &self,
        viewer: &str,
        fallback_root: Option<&str>,
        candidates: I,
        minimum_score: i32,
    ) -> Vec<TrustDecision>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let (root, rooted_at_fallback) = self.effective_root(viewer, fallback_root);
        self.batch_score_with_minimum_score(root, candidates, minimum_score)
            .into_iter()
            .map(|mut decision| {
                decision.rooted_at_fallback = rooted_at_fallback;
                decision
            })
            .collect()
    }

    /// Pubkeys followed by `viewer` who also follow `candidate`.
    ///
    /// Returned in deterministic pubkey order so callers can safely snapshot,
    /// diff, and test the result without another sorting pass.
    #[must_use]
    pub fn mutual_follows(&self, viewer: &str, candidate: &str) -> Vec<String> {
        let Some(follows) = self.follows_by_author.get(viewer) else {
            return Vec::new();
        };
        follows
            .iter()
            .filter(|followed| {
                self.follows_by_author
                    .get(*followed)
                    .is_some_and(|their_follows| their_follows.contains(candidate))
            })
            .cloned()
            .collect()
    }

    /// True when `viewer` directly follows `candidate`.
    #[must_use]
    pub fn directly_follows(&self, viewer: &str, candidate: &str) -> bool {
        self.follows_by_author
            .get(viewer)
            .is_some_and(|follows| follows.contains(candidate))
    }

    fn score_parts(&self, viewer: &str, candidate: &str) -> ScoredParts {
        if viewer == candidate {
            return ScoredParts {
                score: SELF_SCORE,
                reason: "self",
                self_muted: false,
            };
        }

        let viewer_follows = self.follows_by_author.get(viewer);
        let viewer_mutes = self.mutes_by_author.get(viewer);
        if viewer_mutes.is_some_and(|mutes| mutes.contains(candidate)) {
            return ScoredParts {
                score: SELF_MUTE_SCORE,
                reason: "muted-by-self",
                self_muted: true,
            };
        }

        let direct = viewer_follows.is_some_and(|follows| follows.contains(candidate));
        if direct {
            return ScoredParts {
                score: DIRECT_FOLLOW_SCORE,
                reason: "direct-follow",
                self_muted: false,
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

        let default_hide = score <= DEFAULT_AUTO_HIDE_SCORE;
        let reason = if default_hide {
            "muted-by-followed"
        } else if second_degree > 0 {
            "second-degree"
        } else if followed_mutes > 0 {
            "weak-negative"
        } else {
            "unknown"
        };

        ScoredParts {
            score,
            reason,
            self_muted: false,
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

    /// Current graph size counters.
    #[must_use]
    pub fn stats(&self) -> WotGraphStats {
        WotGraphStats {
            follow_authors: self.follow_author_count(),
            mute_authors: self.mute_author_count(),
        }
    }

    /// Return second-degree candidates (not yet followed by `viewer`) ranked by
    /// how many of `viewer`'s direct follows also follow each candidate.
    ///
    /// Results are sorted descending by mutual-follow count, then ascending by
    /// pubkey for deterministic ordering at equal counts. Up to `limit` entries
    /// are returned (0 = unlimited).
    ///
    /// Each entry is `(candidate_pubkey, mutual_follow_count)`.
    #[must_use]
    pub fn ranked_second_degree_candidates(
        &self,
        viewer: &str,
        limit: usize,
    ) -> Vec<(String, usize)> {
        let Some(follows) = self.follows_by_author.get(viewer) else {
            return Vec::new();
        };
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for followed in follows {
            if let Some(their_follows) = self.follows_by_author.get(followed) {
                for candidate in their_follows {
                    // Skip accounts the viewer already follows and the viewer themselves.
                    if !follows.contains(candidate) && candidate != viewer {
                        *counts.entry(candidate.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
        let mut ranked: Vec<(String, usize)> = counts.into_iter().collect();
        // Descending by count, then ascending by pubkey for stability.
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        if limit > 0 {
            ranked.truncate(limit);
        }
        ranked
    }
}

struct ScoredParts {
    score: i32,
    reason: &'static str,
    self_muted: bool,
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
#[path = "score_tests.rs"]
mod tests;
