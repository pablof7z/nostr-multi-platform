//! Contact-list read accessors used by the publish / follow-edit path.
//!
//! Split from `publish_cmd.rs` to keep each file under the 500-LOC hard cap
//! (AGENTS.md §file-size). These methods belong to [`Kernel`] and are declared
//! in a separate `impl` block; callers are unaffected (no visibility change).

use super::{is_hex_pubkey, Kernel};

impl Kernel {
    /// Hex pubkey of the author of `event_id_hex`, or `None` if that event is
    /// not in the kernel's read-cache.
    ///
    /// Reads `self.events` — the lightweight read-cache — rather than the
    /// store directly. Production ingest (`ingest/timeline.rs`) populates both
    /// in lockstep, so the read-cache is a faithful view; the choice avoids a
    /// store round-trip on the publish hot path. `None` is a normal result
    /// (the event simply hasn't been ingested);
    /// the caller degrades gracefully (D6 — emit the reaction with only the `e`
    /// tag, never panic).
    #[must_use]
    pub(crate) fn event_author(&self, event_id_hex: &str) -> Option<String> {
        self.events.get(event_id_hex).map(|e| e.author.clone())
    }

    /// Latest kind:3 follow set for the active account, distinguishing
    /// "not loaded" from "loaded but empty".
    ///
    /// Returns `Some(pubkeys)` when the active account's kind:3 contact list
    /// IS present in the store — even when no valid `p` tags survive the
    /// hex-validation filter (legitimately empty follow list → `Some(vec![])`).
    ///
    /// Returns `None` when:
    /// - No active account is set, **or**
    /// - The active account's kind:3 has not been ingested yet.
    ///
    /// This is the safety gate for wasm Follow / Unfollow: callers MUST
    /// receive `Some` before editing the follow set. Publishing an edit when
    /// `None` is returned would risk silently wiping an unloaded contact list.
    ///
    /// Note: the list is uncapped — and the follow set is now uncapped
    /// everywhere (#1497 amendment 6 collapsed the follow-feed to one
    /// multi-author interest with no per-author limit).
    #[must_use]
    pub(crate) fn try_current_follows(&self) -> Option<Vec<String>> {
        let (tags, _content, _created_at) = self.try_current_kind3_event()?;
        let follows = tags
            .iter()
            .filter(|t: &&Vec<String>| t.first().map(String::as_str) == Some("p"))
            .filter_map(|t| t.get(1).cloned())
            .filter(|pk| is_hex_pubkey(pk))
            .collect();
        Some(follows)
    }

    /// Return the active account's FULL existing kind:3 raw event — every tag
    /// verbatim (`Vec<Vec<String>>`, including relay-hint and petname columns
    /// on `p` tags and every non-`p` tag), the original `content` string, and
    /// the baseline `created_at` — so a follow-list edit can splice ONLY the
    /// `p` section and stamp a strict replacement without discarding the rest of
    /// the user's contact list (issue #1246).
    ///
    /// Fails closed: returns `None` when no active account is set OR the active
    /// account's kind:3 has not been ingested yet — the SAME safety gate as
    /// [`Self::try_current_follows`]. Callers MUST receive `Some` before
    /// editing; publishing an edit built from `None` would silently wipe an
    /// unloaded contact list. The tag set is uncapped (a cap is a subscription
    /// concern, not a contact-list-editing one — capping here would silently
    /// drop follows ≥501 on every edit).
    #[must_use]
    pub(crate) fn try_current_kind3_event(&self) -> Option<(Vec<Vec<String>>, String, u64)> {
        let author_hex = self.active_account_pubkey()?;
        let author = crate::kernel::hex_to_pubkey_bytes(author_hex)?;
        let Ok(mut iter) = self.store.scan_by_author_kind(&author, &[3], None, None, 1) else {
            return None;
        };
        let Some(Ok(stored)) = iter.next() else {
            // kind:3 not yet ingested — None, not empty.
            return None;
        };
        Some((
            stored.raw.tags.clone(),
            stored.raw.content.clone(),
            stored.raw.created_at,
        ))
    }

    /// Resolve the active account's CURRENT kind:3 baseline for a follow-set
    /// edit (the actor `follow` / `follow_many` write path), in priority order:
    ///
    /// 1. The FULL raw kind:3 event from the store — every tag + content
    ///    verbatim ([`Self::try_current_kind3_event`]). This preserves relay
    ///    hints, petnames, non-`p` tags, and content on re-publish (issue
    ///    #1246a). It is the synced / locally-published path.
    /// 2. Otherwise `None` — an account whose kind:3 has NOT synced yet
    ///    (cache `None`). Editing here would silently clobber an unsynced remote
    ///    contact list, so callers MUST fail closed (issue #1246b).
    ///
    /// This is the gate that distinguishes "no list exists (a brand-new local
    /// account, safe to publish its first kind:3)" from "a list exists remotely
    /// but is not loaded (must fail closed)". The store-only
    /// [`Self::try_current_kind3_event`] remains the wasm reducer seam's gate and
    /// keeps its strict not-loaded → `None` contract unchanged.
    #[must_use]
    pub(crate) fn try_current_kind3_event_for_edit(
        &self,
    ) -> Option<(Vec<Vec<String>>, String, u64)> {
        self.try_current_kind3_event()
    }
}
