//! `nmp-highlighter-core` — Highlighter's app-specific extension crate.
//!
//! Per the M11.5 design (`docs/plan/m11.5-highlighter.md` Step 3): app-specific
//! projections that don't generalize live in the app's own extension crate,
//! not in a protocol crate. Cross-protocol composition (`HydratedGroupChat`,
//! `DiscussionsWithReplyCounts`, `GroupArtifactLanes`,
//! `PublishHighlightAndShareToGroup`) lives here, NOT inside any `nmp-nip*`.
//!
//! ## Step 0 scope
//!
//! For M11.5 Step 0 (T42), this crate ships as a **scaffold** — module
//! placeholders that the iOS app's Generated layer (`Core/Generated/`) can
//! depend on through codegen even though the full ReadsFeed / SearchIndex /
//! CaptureFlow / Feedback / WebMetadata / IsbnLookup / BookRegistry impls
//! land in Steps 3 + 5.
//!
//! The deliverable here proves the crate boundary holds: this crate may
//! import `nmp-nip29`, but `nmp-nip29` does not import this crate (and never
//! will — protocol crates are leaves in the dependency graph).

pub mod placeholders;

/// Re-export so consumers can confirm at compile time that the dependency on
/// `nmp-nip29` is correctly wired without needing to import it themselves.
pub use nmp_nip29::GroupId;
