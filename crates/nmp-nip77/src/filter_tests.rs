use nmp_coverage_gate::ResultSurface;
use serde_json::Value;

use crate::{EligibleFilter, FilterEligibilityError};

fn hex(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

#[test]
fn accepts_empty_authors_empty_kinds_and_tags() {
    let filter =
        EligibleFilter::parse(&serde_json::json!({"#e":[hex(1)],"kinds":[]}).to_string()).unwrap();
    assert!(filter.authors.is_empty());
    assert!(filter.kinds.is_empty());
    assert_eq!(filter.tags["e"], vec![hex(1)]);
    assert_eq!(filter.result_surface(), ResultSurface::Unbounded);
}

#[test]
fn ids_bound_result_surface() {
    let filter =
        EligibleFilter::parse(&serde_json::json!({"ids":[hex(1), hex(2), hex(3)]}).to_string())
            .unwrap();
    assert_eq!(filter.result_surface(), ResultSurface::KnownMax(3));
}

#[test]
fn exact_replaceable_and_addressable_keys_are_small() {
    let replaceable =
        EligibleFilter::parse(&serde_json::json!({"authors":[hex(1)],"kinds":[0,3]}).to_string())
            .unwrap();
    assert_eq!(replaceable.result_surface(), ResultSurface::KnownMax(2));

    let addressable = EligibleFilter::parse(
        &serde_json::json!({"authors":[hex(1)],"kinds":[30023],"#d":["hello"]}).to_string(),
    )
    .unwrap();
    assert_eq!(addressable.result_surface(), ResultSurface::KnownMax(1));
}

#[test]
fn addressable_without_d_is_unbounded() {
    let filter =
        EligibleFilter::parse(&serde_json::json!({"authors":[hex(1)],"kinds":[30023]}).to_string())
            .unwrap();
    assert_eq!(filter.result_surface(), ResultSurface::Unbounded);
}

#[test]
fn accepts_limit_and_can_build_live_only_filter() {
    let filter = EligibleFilter::parse(
        &serde_json::json!({
            "authors": [hex(1)],
            "kinds": [1],
            "limit": 200,
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(filter.limit, Some(200));

    let live: Value = serde_json::from_str(&filter.live_only_filter_json()).unwrap();
    assert_eq!(live["limit"], Value::from(0));
    assert_eq!(live["kinds"], serde_json::json!([1]));
}

#[test]
fn unfloored_drops_since_keeps_until_limit_and_tags() {
    let filter = EligibleFilter::parse(
        &serde_json::json!({
            "authors": [hex(1)],
            "kinds": [1],
            "#e": [hex(2)],
            "since": 5_000,
            "until": 9_000,
            "limit": 200,
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(filter.since, Some(5_000));

    let unfloored = filter.unfloored();
    assert_eq!(unfloored.since, None);
    assert_eq!(unfloored.until, Some(9_000));
    assert_eq!(unfloored.limit, Some(200));
    assert_eq!(unfloored.tags["e"], vec![hex(2)]);
    let obj = unfloored.value.as_object().unwrap();
    assert!(!obj.contains_key("since"));
    assert_eq!(obj["#e"], serde_json::json!([hex(2)]));
}

#[test]
fn rejects_search_because_local_set_is_not_structural() {
    assert_eq!(
        EligibleFilter::parse(r#"{"search":"nostr"}"#).unwrap_err(),
        FilterEligibilityError::SearchUnsupported
    );
}
