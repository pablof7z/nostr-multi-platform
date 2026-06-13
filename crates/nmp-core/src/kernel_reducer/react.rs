//! React (NIP-25 kind:7) write-path surface for [`super::KernelReducer`].
//!
//! Split from `kernel_reducer.rs` to keep that file under the 500-LOC hard
//! ceiling (AGENTS.md). `build_reaction_draft` is the PR-6a wasm write-path
//! seam: it resolves NIP-25 kind:7 tags from the kernel read-cache before
//! the async sign boundary so no `RefCell` borrow lives across an await
//! point — identical borrow discipline to `build_reply_tags` in `reply.rs`.

impl super::KernelReducer {
    /// Build a NIP-25 kind:7 reaction draft for `target_event_id` (hex).
    ///
    /// Returns `Some((tags, content))` where:
    /// - `tags` is `[["e", target_event_id], ["p", author]?]` — the `p` tag
    ///   is included only when `target_event_id`'s author is in the kernel's
    ///   read-cache; absent author degrades to `e`-only (valid NIP-25, D6).
    /// - `content` is the reaction string, normalised to `"+"` when blank.
    ///
    /// Returns `None` when `target_event_id` is not a valid 64-char hex event
    /// id (fail-closed; callers use `react_target_invalid_reason:`).
    ///
    /// Takes `&self` — the borrow drops before any async boundary (wasm
    /// `RefCell` borrow discipline, same contract as `build_reply_tags`).
    ///
    /// Delegates tag construction to [`crate::tags::reaction_tags`] — the
    /// shared canonical implementation also used by native `react()`.
    #[must_use]
    pub fn build_reaction_draft(
        &self,
        target_event_id: &str,
        reaction: &str,
    ) -> Option<(Vec<Vec<String>>, String)> {
        let author = self.kernel.event_author(target_event_id);
        crate::tags::reaction_tags(target_event_id, author.as_deref(), reaction)
    }
}
