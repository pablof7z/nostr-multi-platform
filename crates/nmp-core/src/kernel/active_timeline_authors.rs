//! Public typed accessor over the active account's `timeline_authors` set.
//!
//! `timeline_authors` is the kernel's timeline-projection author gate. Dynamic
//! feed author-set expansion is owned by feed-source compilation above
//! `nmp-core`; this accessor only exposes the current projection gate.
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
//!   application-read projection; the name carries no NIP-02 noun.
//! - **D8** — O(n) clone bounded by the follow-set size; called on demand by
//!   a consumer, never on the per-event hot path.

use super::Kernel;

impl Kernel {
    /// The active account's timeline-author set as a sorted `Vec` of raw hex
    /// pubkeys.
    ///
    /// Returns the authors currently admitted by the timeline projection gate.
    /// The set is returned sorted (the backing store is a `BTreeSet`, so
    /// iteration order is already ascending) and as raw pubkeys — display
    /// composition is a higher-layer concern.
    #[must_use]
    pub fn active_timeline_authors(&self) -> Vec<String> {
        self.timeline_authors.iter().cloned().collect()
    }
}
