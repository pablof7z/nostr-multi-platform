//! Unit tests for the [`CompositionLedger`] (ADR-0049 Part 2).

use super::*;

#[test]
fn empty_ledger_reports_zero_records() {
    let ledger = CompositionLedger::new();
    let v = ledger.to_json();
    assert_eq!(v["schema_version"], COMPOSITION_REPORT_SCHEMA_VERSION);
    assert_eq!(v["count"], 0);
    assert_eq!(v["records"].as_array().unwrap().len(), 0);
}

#[test]
fn record_appends_in_order() {
    let ledger = CompositionLedger::new();
    ledger.record(
        "action_registry",
        "nmp.publish",
        "nmp_core::publish::PublishModule",
        Disposition::Installed,
        None,
    );
    ledger.record(
        "action_registry",
        "nmp.nip02.follow",
        "nmp_nip02::FollowModule",
        Disposition::YieldedToExisting,
        Some("app::MyFollow".to_string()),
    );
    assert_eq!(ledger.len(), 2);

    let records = ledger.records();
    assert_eq!(records[0].key, "nmp.publish");
    assert_eq!(records[0].disposition, Disposition::Installed);
    assert!(records[0].replaced.is_none());
    assert_eq!(records[1].key, "nmp.nip02.follow");
    assert_eq!(records[1].disposition, Disposition::YieldedToExisting);
    assert_eq!(records[1].replaced.as_deref(), Some("app::MyFollow"));
}

#[test]
fn to_json_serializes_disposition_and_replaced() {
    let ledger = CompositionLedger::new();
    ledger.record(
        "action_registry",
        "nmp.publish",
        "app::MyPublish",
        Disposition::ReplacedPrevious,
        Some("nmp_core::publish::PublishModule".to_string()),
    );
    let v = ledger.to_json();
    assert_eq!(v["count"], 1);
    let rec = &v["records"][0];
    assert_eq!(rec["seam"], "action_registry");
    assert_eq!(rec["key"], "nmp.publish");
    assert_eq!(rec["provider"], "app::MyPublish");
    // serde serializes the unit enum variant by its Rust name.
    assert_eq!(rec["disposition"], "ReplacedPrevious");
    assert_eq!(rec["replaced"], "nmp_core::publish::PublishModule");
}

#[test]
fn replaced_is_omitted_when_none() {
    let ledger = CompositionLedger::new();
    ledger.record(
        "snapshot_projection",
        "todo.items",
        "app::TodoProjection",
        Disposition::Installed,
        None,
    );
    let v = ledger.to_json();
    let rec = &v["records"][0];
    assert!(
        rec.get("replaced").is_none(),
        "replaced must be omitted (skip_serializing_if) when None"
    );
}

#[test]
fn disposition_as_str_tokens_are_stable() {
    assert_eq!(Disposition::Installed.as_str(), "installed");
    assert_eq!(Disposition::ReplacedPrevious.as_str(), "replaced_previous");
    assert_eq!(
        Disposition::YieldedToExisting.as_str(),
        "yielded_to_existing"
    );
    assert_eq!(
        Disposition::DroppedLateWiring.as_str(),
        "dropped_late_wiring"
    );
}

#[test]
fn report_is_round_trippable_through_serde() {
    let ledger = CompositionLedger::new();
    ledger.record(
        "routing_substrate",
        "routing_substrate",
        "nmp_router::GenericOutboxRouter",
        Disposition::Installed,
        None,
    );
    let v = ledger.to_json();
    let s = serde_json::to_string(&v).expect("ledger JSON serializes");
    let back: serde_json::Value = serde_json::from_str(&s).expect("ledger JSON round-trips");
    assert_eq!(back["count"], 1);
    assert_eq!(back["records"][0]["seam"], "routing_substrate");
}
