//! D17 positive fixture — contains a `"kinds":[1,6]` social-kind filter in
//! non-comment, non-test production code. The lint must flag this.

pub fn build_timeline_filter() -> String {
    // This is the shape that V-68 removed and D17 guards against.
    let filter = r#"{"kinds":[1,6],"limit":100}"#;
    filter.to_string()
}
