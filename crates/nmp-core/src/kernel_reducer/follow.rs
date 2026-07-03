//! Follow / Unfollow write-path surface for [`super::KernelReducer`].
//!
//! Split from `kernel_reducer.rs` to keep that file under the 500-LOC hard
//! ceiling (AGENTS.md). `try_current_follows` is the PR-6b wasm write-path
//! seam: it looks up the active account's contact list through the registered
//! protocol reader before the async sign boundary so no `RefCell` borrow lives
//! across an await point — identical borrow discipline to `build_reply_tags` in
//! `reply.rs` and `build_reaction_draft` in `react.rs`.

impl super::KernelReducer {
    /// Read the active account's follow set, cleanly distinguishing "not
    /// loaded" from "loaded but empty".
    ///
    /// Returns `Some(pubkeys)` when the active account IS set AND their
    /// contact list is loaded — including a legitimately empty list
    /// (`Some(vec![])`).
    ///
    /// Returns `None` when:
    /// - No active account is set, **or**
    /// - The active account's contact list has not been ingested yet.
    ///
    /// The wasm Follow / Unfollow path MUST check for `Some` before editing:
    /// publishing a contact-list edit from `None` → `[]` would silently wipe the
    /// user's contact list. The `None` path returns an honest
    /// `CapabilityFailure(follow_list_not_loaded)` to the host instead.
    ///
    /// Takes `&self` — the borrow drops before any async boundary (wasm
    /// `RefCell` borrow discipline, same contract as `build_reply_tags`).
    #[must_use]
    pub fn try_current_follows(&self) -> Option<Vec<String>> {
        self.kernel.try_current_follows()
    }

    /// Read the active account's FULL existing contact-list raw event — every tag
    /// verbatim (relay-hint + petname columns on `p` tags, every non-`p` tag)
    /// plus the original `content` string — so the wasm Follow / Unfollow
    /// write-path can splice ONLY the `p` section and preserve the rest of the
    /// user's contact list on re-publish (issue #1246).
    ///
    /// Same fail-closed gate as [`Self::try_current_follows`]: `None` when no
    /// active account is set OR the contact list has not been ingested yet. The wasm
    /// path MUST check for `Some` before editing — building a contact-list event from
    /// `None` would silently wipe the user's contacts. Takes `&self`; the
    /// borrow drops before any async boundary (wasm `RefCell` discipline).
    #[must_use]
    pub fn try_current_contact_list_event(&self) -> Option<(Vec<Vec<String>>, String)> {
        self.kernel
            .try_current_contact_list_event()
            .map(|event| (event.tags, event.content))
    }
}
