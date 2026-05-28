//! Public typed accessor over the active account's `timeline_authors` set.
//!
//! `timeline_authors` is the kernel's projection of the active account's
//! follow-derived feed authorship set — single-sourced from the M2 planner
//! (`ingest/contacts.rs::sync_follow_feed_interests`). It is the read-side
//! "which authors does the active account's home stream cover" set.
//!
//! This accessor exposes that set publicly as a sorted `Vec<String>` of raw
//! hex pubkeys. It is the substrate-generic read seam later rungs of the
//! OP-centric feed (V-59) consume to seed the `FollowSetLookup` capability —
//! the kernel emits raw pubkeys only; no display formatting, no protocol noun.
//!
//! Lives as a sibling of `kernel/mod.rs` so the new `impl Kernel` method does
//! not grow the already-large `mod.rs` / `types.rs` (D-V12). The
//! `#[cfg(test)]` `timeline_authors_for_test` accessor in `test_support.rs`
//! is the borrowed test-only twin and is intentionally retained.
//!
//! Doctrine:
//! - **D0** — substrate-generic. `timeline_authors` is a generic
//!   application-read projection; the name carries no NIP-02 / "follow" noun.
//! - **D8** — O(n) clone bounded by the follow-set size; called on demand by
//!   a consumer, never on the per-event hot path.

use super::Kernel;

impl Kernel {
    /// The active account's timeline-author set as a sorted `Vec` of raw hex
    /// pubkeys.
    ///
    /// Returns the authors whose kind:1 / kind:6 events the active account's
    /// home stream covers (the projection `sync_follow_feed_interests`
    /// single-sources from the contact-list ingest). The set is returned
    /// sorted (the backing store is a `BTreeSet`, so iteration order is
    /// already ascending) and as raw pubkeys — display composition is a
    /// higher-layer concern (2026-05-25 display-separation doctrine).
    #[must_use]
    pub fn active_timeline_authors(&self) -> Vec<String> {
        self.timeline_authors.iter().cloned().collect()
    }
}
