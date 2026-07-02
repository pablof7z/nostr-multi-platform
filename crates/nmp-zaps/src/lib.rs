//! `nmp-zaps` — the app-facing zap-summary read owner for NMP.
//!
//! Apps provide a plain kind:1 note event id. This crate compiles the
//! NIP-57 kind:9735 `#e` read plan over `nmp-nip57`'s validated receipt
//! decoder and drives it through the shared read-lifecycle engine
//! (`nmp-read-session`, #2777) via [`open_zaps`] / [`close_zaps`]. Protocol
//! semantics — receipt decode, bolt11 amount parsing, embedded
//! zap-request/amount-consistency checks, and provider-mismatch rejection —
//! stay in `nmp-nip57`; apps and this crate never re-parse a tag or a bolt11
//! invoice.
//!
//! `ZapSummarySnapshot` carries raw per-sender aggregation only
//! (`zappers: pubkey -> msats/count`); it has no viewer-relative field. A
//! shell that wants "did the active user zap this" already has its own
//! active-account pubkey and membership-checks it against `zappers` —
//! `open_zaps` takes no viewer parameter and this crate has no viewer-
//! identity dependency (mirrors `nmp-reposts`' `reposter_pubkeys`).

pub mod ownership;
mod read;
mod summary;
mod target;

pub use read::ZapReadPlan;
pub use summary::{
    close_zaps, encode_zap_summary_snapshot, open_zaps, ZapSummaryProjection, ZapSummarySnapshot,
    ZapperTotal, ZapsReadHandle, ZAP_SUMMARY_FILE_IDENTIFIER, ZAP_SUMMARY_SCHEMA_ID,
    ZAP_SUMMARY_SCHEMA_VERSION,
};
pub use target::{ZapTarget, ZapTargetError};
