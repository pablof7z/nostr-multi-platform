//! Reaction write-path surface for [`super::KernelReducer`].
//!
//! Split from `kernel_reducer.rs` to keep that file under the 500-LOC hard
//! ceiling (AGENTS.md). `build_reaction_draft` resolves the target author from
//! the kernel read-cache before the async sign boundary, then delegates wire
//! draft construction to the registered protocol builder.

impl super::KernelReducer {
    /// Build a reaction draft for `target_event_id` through the registered
    /// protocol builder.
    ///
    /// Takes `&self` — the borrow drops before any async boundary (wasm
    /// `RefCell` borrow discipline, same contract as `build_reply_tags`).
    #[must_use]
    pub fn build_reaction_draft(
        &self,
        target_event_id: &str,
        reaction: &str,
    ) -> Option<(Vec<Vec<String>>, String)> {
        let author = self.kernel.event_author(target_event_id);
        self.reaction_draft_builder
            .build_reaction_draft(target_event_id, author.as_deref(), reaction)
            .map(|draft| (draft.tags, draft.content))
    }
}
