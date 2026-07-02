//! `nmp-reposts` — the app-facing repost-count read owner for NMP.
//!
//! Apps provide a plain kind:1 note event id. This crate compiles the
//! matching NIP-18 read plan (kind:6 reposts, plus `k`-tag-discriminated
//! kind:16 generic reposts) and drives it through the ONE read-lifecycle
//! engine (`nmp-read-session`) via [`open_reposts`] / [`close_reposts`].
//! Protocol-specific wire shape stays in `nmp-nip18` / `nmp-nip09`; apps do
//! not construct `e`/`k` tags or NIP-09 deletion grammar.

pub mod ownership;
mod read;
mod summary;
mod target;

pub use read::RepostReadPlan;
pub use summary::{
    close_reposts, encode_repost_summary_snapshot, open_reposts, RepostSummaryProjection,
    RepostSummarySnapshot, RepostsReadHandle, REPOST_SUMMARY_FILE_IDENTIFIER,
    REPOST_SUMMARY_SCHEMA_ID, REPOST_SUMMARY_SCHEMA_VERSION,
};
pub use target::{RepostTarget, RepostTargetError};
