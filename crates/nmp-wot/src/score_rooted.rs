//! Fallback-root ("bootstrap trust seed") scoring for the WoT cold-start.
//!
//! A viewer with no ingested follows (empty/absent kind:3) scores every
//! candidate at 0, which sits at or below any sane minimum floor, so
//! everything gets filtered out. This cold-start hits every WoT consumer
//! (mint discovery, feeds, spam filtering, nutzap-sender trust) identically,
//! so the fix lives here in the scoring primitive as a GENERAL, opt-in
//! `fallback_root` seed rather than being special-cased per consumer.
//!
//! This module hangs the `*_rooted*` family off [`WotGraph`] so `score.rs`
//! stays under the file-size ceiling; the base scoring math stays in
//! `score.rs`. Split by cohesive sub-behavior, not by TEA role (AGENTS.md).

use super::{TrustDecision, WotGraph, SELF_MUTE_SCORE};

impl WotGraph {
    /// True iff `viewer` has an ingested, non-empty follow set.
    ///
    /// Used to detect the WoT cold-start case: a viewer with no follows
    /// (never ingested a kind:3, or ingested an empty one) scores every
    /// candidate at 0, which sits at or below any sane minimum floor and
    /// hides everything. [`score_rooted`](Self::score_rooted) and
    /// [`score_rooted_with_minimum_score`](Self::score_rooted_with_minimum_score)
    /// use this to decide whether to reroute scoring through a
    /// `fallback_root` instead.
    ///
    /// "Cold" is defined purely by the *follow* set: a viewer's own mute list
    /// is not part of coldness and is never delegated to a fallback root — a
    /// cold viewer's self-mutes are still honored when scoring reroutes (see
    /// [`score_rooted`](Self::score_rooted)).
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
    ///
    /// `rooted_at_fallback` is `true` only when the effective root is actually
    /// *different* from `viewer`, so a `Some(viewer)` no-op substitution is
    /// reported as `false` — the viewer's own graph was used after all.
    fn effective_root<'a>(
        &self,
        viewer: &'a str,
        fallback_root: Option<&'a str>,
    ) -> (&'a str, bool) {
        let root = if self.has_follows(viewer) {
            viewer
        } else {
            fallback_root.unwrap_or(viewer)
        };
        (root, root != viewer)
    }

    /// True when `viewer`'s own ingested mute list mutes `candidate`.
    ///
    /// This consults *only* the real viewer's mutes, never a fallback root's,
    /// so it can preserve the viewer's self-mute across a reroute (see
    /// [`rooted_self_mute_guard`](Self::rooted_self_mute_guard)).
    fn viewer_self_muted(&self, viewer: &str, candidate: &str) -> bool {
        self.mutes_by_author
            .get(viewer)
            .is_some_and(|mutes| mutes.contains(candidate))
    }

    /// When scoring is about to be rerouted to a fallback root
    /// (`rooted_at_fallback == true`), a candidate the *real viewer* has
    /// explicitly muted must still hide, regardless of what the fallback
    /// root's graph thinks. Returns the viewer's own self-mute decision in
    /// that case, or `None` when no reroute-time mute override applies and the
    /// caller should proceed with the positive graph walk from the fallback
    /// root.
    ///
    /// The returned decision reports `rooted_at_fallback: false` because it is
    /// the viewer's own data, not a fabricated fallback-root opinion.
    fn rooted_self_mute_guard(
        &self,
        viewer: &str,
        candidate: &str,
        rooted_at_fallback: bool,
    ) -> Option<TrustDecision> {
        if rooted_at_fallback && self.viewer_self_muted(viewer, candidate) {
            Some(TrustDecision {
                score: SELF_MUTE_SCORE,
                hide: true,
                reason: "muted-by-self",
                rooted_at_fallback: false,
            })
        } else {
            None
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
    /// Reuses [`score`](Self::score) unchanged for the *positive* scoring math —
    /// only the pubkey used as the root of the graph walk changes. Passing
    /// `fallback_root: None` reproduces today's [`score`](Self::score)
    /// behavior byte-for-byte, including for a cold viewer.
    ///
    /// The real viewer's own self-mute is preserved across the reroute: a
    /// candidate the viewer explicitly muted still hides even when the
    /// fallback root follows (and would otherwise "trust") them. Only the
    /// positive graph walk delegates to the fallback root; the negative
    /// "muted-by-self" veto always uses the viewer's own mute list.
    #[must_use]
    pub fn score_rooted(
        &self,
        viewer: &str,
        fallback_root: Option<&str>,
        candidate: &str,
    ) -> TrustDecision {
        let (root, rooted_at_fallback) = self.effective_root(viewer, fallback_root);
        if let Some(muted) = self.rooted_self_mute_guard(viewer, candidate, rooted_at_fallback) {
            return muted;
        }
        let mut decision = self.score(root, candidate);
        decision.rooted_at_fallback = rooted_at_fallback;
        decision
    }

    /// [`score_rooted`](Self::score_rooted) with a caller-supplied minimum-score
    /// floor, mirroring [`score_with_minimum_score`](Self::score_with_minimum_score).
    /// Preserves the viewer's self-mute across the reroute in the same way.
    #[must_use]
    pub fn score_rooted_with_minimum_score(
        &self,
        viewer: &str,
        fallback_root: Option<&str>,
        candidate: &str,
        minimum_score: i32,
    ) -> TrustDecision {
        let (root, rooted_at_fallback) = self.effective_root(viewer, fallback_root);
        if let Some(muted) = self.rooted_self_mute_guard(viewer, candidate, rooted_at_fallback) {
            return muted;
        }
        let mut decision = self.score_with_minimum_score(root, candidate, minimum_score);
        decision.rooted_at_fallback = rooted_at_fallback;
        decision
    }

    /// Batch variant of [`score_rooted`](Self::score_rooted).
    ///
    /// Delegates per-candidate so the viewer's self-mute veto is honored for
    /// each candidate individually.
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
        candidates
            .into_iter()
            .map(|candidate| self.score_rooted(viewer, fallback_root, candidate))
            .collect()
    }

    /// Batch variant of [`score_rooted_with_minimum_score`](Self::score_rooted_with_minimum_score).
    ///
    /// Delegates per-candidate so the viewer's self-mute veto is honored for
    /// each candidate individually.
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
        candidates
            .into_iter()
            .map(|candidate| {
                self.score_rooted_with_minimum_score(
                    viewer,
                    fallback_root,
                    candidate,
                    minimum_score,
                )
            })
            .collect()
    }
}
