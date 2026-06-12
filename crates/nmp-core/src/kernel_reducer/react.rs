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
    /// Returns `None` when `target_event_id` is not a valid 64-char lowercase
    /// hex event id (fail-closed; callers use `react_target_invalid_reason:`).
    ///
    /// Takes `&self` — the borrow drops before any async boundary (wasm
    /// `RefCell` borrow discipline, same contract as `build_reply_tags`).
    ///
    /// Byte-identical tag construction to native `react()` in
    /// `actor/commands/publish.rs` lines 565-568: `e` tag always, `p` tag
    /// only when author is in read-cache. NIP-25 makes `k` optional and
    /// native omits it; this builder matches native exactly (no drift).
    #[must_use]
    pub fn build_reaction_draft(
        &self,
        target_event_id: &str,
        reaction: &str,
    ) -> Option<(Vec<Vec<String>>, String)> {
        if !crate::kernel::is_hex_id(target_event_id) {
            return None;
        }
        let content = if reaction.trim().is_empty() {
            "+".to_string()
        } else {
            reaction.to_string()
        };
        // NIP-25 §1: an `e` tag (the reacted-to event) is always included.
        // The `p` tag (the event's author) is added when the author is in the
        // kernel's read-cache so the reaction routes to their notification
        // inbox. If not cached, the reaction is still valid NIP-25 with only
        // the `e` tag (D6 — degrade, never panic).
        let mut tags = vec![vec!["e".to_string(), target_event_id.to_string()]];
        if let Some(author) = self.kernel.event_author(target_event_id) {
            tags.push(vec!["p".to_string(), author]);
        }
        Some((tags, content))
    }
}
