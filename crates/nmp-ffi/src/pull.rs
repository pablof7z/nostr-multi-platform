//! ADR-0058 §3 mirror pull-page encoding helpers.
//!
//! The public C doorway for mirror pulls has been deleted. The shared pull
//! algorithm and wire encoding live in [`nmp_native_runtime::app_mirror`], and
//! callers use the UniFFI `NmpApp::mirror_pull_page` / `MirrorPullResult`
//! surface instead.

pub use nmp_native_runtime::app_mirror::{
    encode_gap, encode_page, error, variant, MAX_PULL_PAGE_ENTRIES, MAX_PULL_PAGE_RAW_BYTES,
};
