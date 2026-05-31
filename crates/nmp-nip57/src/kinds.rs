//! NIP-57 zap kinds — re-exported from the workspace registry (`nmp-kinds`).
//!
//! The canonical definitions live in `nmp_kinds`; this module re-exports them
//! so all existing `crate::kinds::KIND_ZAP_*` call sites compile unchanged
//! (V-57 dedup).

pub use nmp_kinds::KIND_ZAP_RECEIPT;
pub use nmp_kinds::KIND_ZAP_REQUEST;
