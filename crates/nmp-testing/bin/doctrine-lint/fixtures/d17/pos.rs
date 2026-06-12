//! D17 positive fixture — contains social-kind policy literals in non-comment,
//! non-test production code. The lint must flag each occurrence.

use std::collections::BTreeSet;

/// JSON filter shape — the original V-68 regression target.
pub fn build_timeline_filter() -> String {
    // This is the shape that V-68 removed and D17 guards against.
    let filter = r#"{"kinds":[1,6],"limit":100}"#;
    filter.to_string()
}

/// Rust array literal shape — `[1u32, 6u32]` — the form that the deleted
/// `nmp_app_open_timeline` used and that a future regression could reintroduce
/// in nmp-ffi.
pub fn rust_array_literal() -> BTreeSet<u32> {
    BTreeSet::from([1u32, 6u32])
}
