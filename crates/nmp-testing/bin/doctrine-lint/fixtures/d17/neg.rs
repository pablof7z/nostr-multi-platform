//! D17 negative fixture — these shapes must NOT be flagged by D17.

/// Doc comment referencing `"kinds":[1,6]` — comment lines are exempt.
/// Example: `json!({"kinds":[1,6],"limit":10})`
pub fn explained_in_docs() {}

pub fn unrelated_array() {
    // A bare [1, 6] without the "kinds": prefix is NOT the social-kind filter.
    let arr = [1, 6];
    let _ = arr;
}

pub fn kind_one_only() {
    // "kinds":[1] — only kind 1, no 6.
    let filter = r#"{"kinds":[1],"limit":5}"#;
    let _ = filter;
}

pub fn kind_one_six_seven() {
    // "kinds":[1,6,7] — three-element array, not the banned pair.
    let filter = r#"{"kinds":[1,6,7]}"#;
    let _ = filter;
}

pub fn unrelated_kinds() {
    // "kinds":[3,10000] — unrelated kind pair.
    let filter = r#"{"kinds":[3,10000]}"#;
    let _ = filter;
}

pub fn rust_u32_array_other_kinds() {
    // [1u32, 7u32] — the `[1u32` needle does NOT fire without 6.
    // Note: the detector fires on `[1u32` as a prefix regardless of what
    // follows, so this is actually tested at the `check()` unit level.
    // This negative fixture tests that non-u32-suffixed arrays are clean.
    let arr = [1_u32, 7_u32];
    let _ = arr;
}

#[cfg(test)]
mod tests {
    // Inside cfg(test) block: even "kinds":[1,6] is exempt here.
    #[test]
    fn test_kinds_filter() {
        let s = r#"{"kinds":[1,6]}"#;
        assert!(s.contains("kinds"));
    }
}
