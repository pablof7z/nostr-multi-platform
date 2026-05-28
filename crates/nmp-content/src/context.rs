//! Recursion guard for embed rendering — `RenderContext { depth, visited }`.
//!
//! This recursion guard is often absent in other Nostr content renderers. See
//! `content-rendering.md` §5 (`RenderContext`) and PD-015 (default `max_depth
//! = 4`, configurable per app; beyond `max_depth` the embed card collapses
//! to a "see full thread" link rather than mounting another renderer).

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use nmp_core::substrate::EventId;

/// Per-render-pass state threaded through embedded event cards.
///
/// Constructed at the top-level entry to a render (one per `tokenize` call
/// site that mounts a renderer). Passed by reference into per-segment
/// rendering; the embed card's child renderer receives a `descend()`d copy.
///
/// `visited` is a small-vec because realistic chains are 1–4 deep; we avoid
/// a heap alloc for the common case. Beyond 8 the small-vec spills to heap.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RenderContext {
    /// Current recursion depth (0 = top-level event being rendered).
    pub depth: u8,
    /// Maximum allowed depth (default 4). Beyond this `should_collapse`
    /// returns `true` and the renderer SHOULD show a "see full thread"
    /// link instead of mounting another embed.
    pub max_depth: u8,
    /// `EventIds` already visited on this render path. Prevents an event
    /// that quotes (transitively) itself from infinite-recursing.
    pub visited: SmallVec<[EventId; 8]>,
}

impl Default for RenderContext {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderContext {
    /// Construct a top-level context with the default max depth (4).
    #[must_use]
    pub fn new() -> Self {
        Self {
            depth: 0,
            max_depth: 4,
            visited: SmallVec::new(),
        }
    }

    /// Construct a top-level context with an explicit `max_depth`. Apps
    /// configure once at app startup; per-render overrides should be rare.
    #[must_use]
    pub fn with_max_depth(max_depth: u8) -> Self {
        Self {
            depth: 0,
            max_depth,
            visited: SmallVec::new(),
        }
    }

    /// True if the renderer SHOULD collapse this embed to a placeholder
    /// link rather than descend. Per PD-015 the conditions are:
    ///   - `depth >= max_depth` (budget exhausted), OR
    ///   - `visited.contains(into)` (cycle detected on this path).
    #[must_use]
    pub fn should_collapse(&self, into: &EventId) -> bool {
        self.depth >= self.max_depth || self.visited.iter().any(|id| id == into)
    }

    /// Produce a child context for rendering an embed of `into`. Increments
    /// `depth` and pushes `into` onto `visited`. Returns the child;
    /// caller passes it to the child renderer.
    ///
    /// Callers MUST check [`should_collapse`] before calling this — calling
    /// `descend` when the budget is exhausted silently increments past
    /// `max_depth` and downstream `should_collapse` will keep returning
    /// `true`, but better-behaved callers gate on it.
    #[must_use]
    pub fn descend(&self, into: EventId) -> Self {
        let mut visited = self.visited.clone();
        visited.push(into);
        Self {
            depth: self.depth.saturating_add(1),
            max_depth: self.max_depth,
            visited,
        }
    }
}

/// Free-function form for FFI surfaces that prefer pure functions over
/// methods. Returns `true` when the renderer may descend into `into`.
///
/// This is the inverse of [`RenderContext::should_collapse`].
#[must_use]
pub fn render_context_can_descend(ctx: &RenderContext, into: &EventId) -> bool {
    !ctx.should_collapse(into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_max_depth_is_four() {
        let ctx = RenderContext::new();
        assert_eq!(ctx.max_depth, 4);
        assert_eq!(ctx.depth, 0);
        assert!(ctx.visited.is_empty());
    }

    #[test]
    fn can_descend_when_under_budget_and_unvisited() {
        let ctx = RenderContext::new();
        assert!(render_context_can_descend(&ctx, &"id".to_string()));
        assert!(!ctx.should_collapse(&"id".to_string()));
    }

    #[test]
    fn cannot_descend_when_cycle_detected() {
        let mut ctx = RenderContext::new();
        ctx.visited.push("self".to_string());
        assert!(!render_context_can_descend(&ctx, &"self".to_string()));
        assert!(ctx.should_collapse(&"self".to_string()));
    }

    #[test]
    fn cannot_descend_past_max_depth() {
        let mut ctx = RenderContext::with_max_depth(2);
        ctx.depth = 2;
        assert!(!render_context_can_descend(&ctx, &"any".to_string()));
        assert!(ctx.should_collapse(&"any".to_string()));
    }

    #[test]
    fn descend_increments_depth_and_pushes_visited() {
        let ctx = RenderContext::new();
        let child = ctx.descend("a".to_string());
        assert_eq!(child.depth, 1);
        assert_eq!(child.visited.as_slice(), &["a".to_string()]);
        let grand = child.descend("b".to_string());
        assert_eq!(grand.depth, 2);
        assert_eq!(
            grand.visited.as_slice(),
            &["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn depth_saturates_does_not_overflow() {
        let mut ctx = RenderContext::with_max_depth(255);
        ctx.depth = u8::MAX;
        let child = ctx.descend("x".to_string());
        assert_eq!(child.depth, u8::MAX);
    }

    #[test]
    fn max_depth_four_collapses_at_fourth_level() {
        // depth 0 (top) -> 1 -> 2 -> 3 -> 4: at depth 4, should_collapse.
        let mut ctx = RenderContext::new();
        for i in 0..4 {
            assert!(!ctx.should_collapse(&"x".to_string()), "leak at depth {i}");
            ctx = ctx.descend(format!("e{i}"));
        }
        assert_eq!(ctx.depth, 4);
        assert!(ctx.should_collapse(&"x".to_string()));
    }
}
