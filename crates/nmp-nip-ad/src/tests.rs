//! Pure unit tests for NIP-AD `.well-known` parsing (no network).

use crate::parse::{is_valid_domain, parse_ad_wellknown};

/// A response with multiple path entries: selection must pick the requested key
/// (the NIP's own trellis example), not the first entry.
#[test]
fn selects_matching_path_entry_not_first() {
    let doc = serde_json::json!({
        "/other": {
            "filter": { "kinds": [1] },
            "relays": ["wss://other.example"]
        },
        "/legible": {
            "filter": {
                "kinds": [30023],
                "authors": ["3f68dede81549cc0844fafe528f1574b51e095e7491f468bd9689f87779bb81d"],
                "#d": ["the-machine-that-could-tell-you-why"]
            },
            "relays": ["wss://relay.primal.net", "wss://relay.damus.io"]
        }
    });
    let res = parse_ad_wellknown(&doc, "/legible").expect("entry selects");
    assert_eq!(
        res.relays,
        vec![
            "wss://relay.primal.net".to_string(),
            "wss://relay.damus.io".to_string()
        ]
    );
    // The full filter is kept intact (kinds + authors + #d, no `limit`).
    let filter_json = serde_json::to_value(&res.filter).unwrap();
    assert_eq!(filter_json["kinds"], serde_json::json!([30023]));
    assert_eq!(
        filter_json["authors"],
        serde_json::json!(["3f68dede81549cc0844fafe528f1574b51e095e7491f468bd9689f87779bb81d"])
    );
    assert_eq!(
        filter_json["#d"],
        serde_json::json!(["the-machine-that-could-tell-you-why"])
    );
    // No `limit` was present and none was invented.
    assert!(filter_json.get("limit").is_none());
}

/// A multi-result filter (no `authors`/`#d`, no `limit`) parses fine and is
/// kept whole — NOT reduced to a single pointer.
#[test]
fn multi_result_filter_is_kept_whole() {
    let doc = serde_json::json!({
        "/highlights": {
            "filter": { "kinds": [20], "authors": ["a".repeat(64)] },
            "relays": ["wss://relay.example"]
        }
    });
    let res = parse_ad_wellknown(&doc, "/highlights").expect("parses");
    let filter_json = serde_json::to_value(&res.filter).unwrap();
    assert_eq!(filter_json["kinds"], serde_json::json!([20]));
    assert!(filter_json.get("limit").is_none());
}

#[test]
fn missing_path_is_error() {
    let doc = serde_json::json!({
        "/present": { "filter": { "kinds": [1] }, "relays": ["wss://r.example"] }
    });
    assert!(parse_ad_wellknown(&doc, "/absent").is_err());
}

#[test]
fn missing_filter_is_error() {
    let doc = serde_json::json!({ "/x": { "relays": ["wss://r.example"] } });
    assert!(parse_ad_wellknown(&doc, "/x").is_err());
}

#[test]
fn missing_relays_is_error() {
    let doc = serde_json::json!({ "/x": { "filter": { "kinds": [1] } } });
    assert!(parse_ad_wellknown(&doc, "/x").is_err());
}

#[test]
fn empty_relays_is_error() {
    let doc = serde_json::json!({ "/x": { "filter": { "kinds": [1] }, "relays": [] } });
    assert!(parse_ad_wellknown(&doc, "/x").is_err());
}

#[test]
fn non_object_document_is_error() {
    let doc = serde_json::json!(["not", "an", "object"]);
    assert!(parse_ad_wellknown(&doc, "/x").is_err());
}

#[test]
fn filter_not_object_is_error() {
    let doc = serde_json::json!({ "/x": { "filter": "nope", "relays": ["wss://r.example"] } });
    assert!(parse_ad_wellknown(&doc, "/x").is_err());
}

#[test]
fn relays_non_string_element_is_error() {
    let doc = serde_json::json!({ "/x": { "filter": { "kinds": [1] }, "relays": [42] } });
    assert!(parse_ad_wellknown(&doc, "/x").is_err());
}

#[test]
fn domain_shape_guard() {
    assert!(is_valid_domain("trellis.rs"));
    assert!(is_valid_domain("sub.example.com"));
    assert!(!is_valid_domain("localhost"));
    assert!(!is_valid_domain("example..com"));
    assert!(!is_valid_domain("-example.com"));
}
