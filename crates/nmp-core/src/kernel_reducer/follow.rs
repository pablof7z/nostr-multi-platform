//! Follow / Unfollow write-path surface for [`super::KernelReducer`].
//!
//! Split from `kernel_reducer.rs` to keep that file under the 500-LOC hard
//! ceiling (AGENTS.md). `try_current_follows` is the PR-6b wasm write-path
//! seam: it looks up the active account's kind:3 contact list from the kernel
//! store before the async sign boundary so no `RefCell` borrow lives across an
//! await point — identical borrow discipline to `build_reply_tags` in
//! `reply.rs` and `build_reaction_draft` in `react.rs`.

impl super::KernelReducer {
    /// Read the active account's kind:3 follow set from the store, cleanly
    /// distinguishing "kind:3 not yet loaded" from "loaded but empty".
    ///
    /// Returns `Some(pubkeys)` when the active account IS set AND their kind:3
    /// contact list IS present in the store — even when no valid `p` tags
    /// survive the hex-validation filter (legitimately empty list →
    /// `Some(vec![])`).
    ///
    /// Returns `None` when:
    /// - No active account is set, **or**
    /// - The active account's kind:3 has not been ingested yet.
    ///
    /// The wasm Follow / Unfollow path MUST check for `Some` before editing:
    /// publishing a kind:3 built from `None` → `[]` would silently wipe the
    /// user's contact list. The `None` path returns an honest
    /// `CapabilityFailure(follow_list_not_loaded)` to the host instead.
    ///
    /// Takes `&self` — the borrow drops before any async boundary (wasm
    /// `RefCell` borrow discipline, same contract as `build_reply_tags`).
    #[must_use]
    pub fn try_current_follows(&self) -> Option<Vec<String>> {
        self.kernel.try_current_follows()
    }

    /// Read the active account's FULL existing kind:3 raw event — every tag
    /// verbatim (relay-hint + petname columns on `p` tags, every non-`p` tag)
    /// plus the original `content` string — so the wasm Follow / Unfollow
    /// write-path can splice ONLY the `p` section and preserve the rest of the
    /// user's contact list on re-publish (issue #1246).
    ///
    /// Same fail-closed gate as [`Self::try_current_follows`]: `None` when no
    /// active account is set OR the kind:3 has not been ingested yet. The wasm
    /// path MUST check for `Some` before editing — building a kind:3 from
    /// `None` would silently wipe the user's contacts. Takes `&self`; the
    /// borrow drops before any async boundary (wasm `RefCell` discipline).
    #[must_use]
    pub fn try_current_kind3_event(&self) -> Option<(Vec<Vec<String>>, String)> {
        self.kernel.try_current_kind3_event()
    }
}
