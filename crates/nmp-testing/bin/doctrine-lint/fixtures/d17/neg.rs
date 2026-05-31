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

#[cfg(test)]
mod tests {
    // Inside cfg(test) block: even "kinds":[1,6] is exempt here.
    #[test]
    fn test_kinds_filter() {
        let s = r#"{"kinds":[1,6]}"#;
        assert!(s.contains("kinds"));
    }
}
