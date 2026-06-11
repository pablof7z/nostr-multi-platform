//! ADR-0044 proof — the typed Tier-3 `SnapshotFrame` envelope fields are
//! populated by `make_update` and agree, field-for-field, with both the
//! generic JSON `payload` tree and the source `KernelSnapshot` state.
//!
//! These tests decode a *real* frame produced by `make_update` (via the
//! `make_update_frame_and_json_for_test` seam): they read the struct-serialized
//! JSON view AND the typed `SnapshotFrame` accessors off the same tick, then
//! assert the two encodings agree. They cover both the present and
//! the absent case for the optional diagnostic fields (`store_open_failure`,
//! `no_configured_relays`) — the pair most likely to expose an encode bug.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::transport::wire as fb;

/// Decode the typed `SnapshotFrame` from raw `UpdateFrameBytes`, returning a
/// closure-friendly borrow into the frame's typed accessors.
fn with_snapshot_frame<R>(bytes: &[u8], f: impl FnOnce(fb::SnapshotFrame<'_>) -> R) -> R {
    assert!(
        fb::update_frame_buffer_has_identifier(bytes),
        "frame must carry the NMPU identifier"
    );
    let frame = fb::root_as_update_frame(bytes).expect("decode update frame");
    assert_eq!(frame.kind(), fb::FrameKind::Snapshot, "expected a snapshot frame");
    let snapshot = frame.snapshot().expect("snapshot frame present");
    f(snapshot)
}

/// On a healthy kernel, the typed Tier-3 scalars/nested fields agree with the
/// JSON obtained by serialising the `KernelSnapshot` struct directly (PR-B:
/// `payload:Value` is no longer emitted on the wire). The optional diagnostic
/// fields are absent in BOTH representations (absent = healthy).
#[test]
fn adr0044_typed_envelope_agrees_with_json_on_healthy_kernel() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (bytes, json) = kernel.make_update_frame_and_json_for_test(true);

    with_snapshot_frame(&bytes, |frame| {
        // Scalars: typed == JSON.
        assert_eq!(frame.rev(), json["rev"].as_u64().expect("rev in json"));
        assert_eq!(
            u64::from(frame.kernel_schema_version()),
            json["schema_version"].as_u64().expect("schema_version in json"),
            "kernel_schema_version mirrors the KernelSnapshot schema_version (KERNEL_SCHEMA_VERSION)"
        );
        assert_eq!(
            frame.last_tick_ms(),
            json["last_tick_ms"].as_u64().expect("last_tick_ms in json")
        );
        assert_eq!(
            frame.update_kind(),
            json["update_kind"].as_str(),
            "update_kind must round-trip as the same ViewBatch string"
        );
        assert_eq!(frame.running(), json["running"].as_bool().expect("running in json"));

        // Nested Metrics: assert EVERY field agrees with the JSON map. This is
        // the bulk of the encode surface — a transposed field (e.g. the
        // timeline_opened/timeline_first_item timestamp cluster) or a botched
        // u128->u64 cast (make_update_us / serialize_us / max_event_to_emit_ms)
        // would diverge here.
        let metrics = frame.metrics().expect("typed metrics present");
        assert_metrics_agree(&metrics, &json["metrics"]);

        // The singular `relay_status` aggregate is ALWAYS emitted (even on a
        // fresh kernel), so it exercises the full 17-field RelayStatus encoder
        // field-for-field without any state setup.
        let relay_status = frame.relay_status().expect("typed relay_status present");
        assert_relay_status_agrees(&relay_status, &json["relay_status"]);

        // `relay_statuses` is the per-role vector — non-empty even on a fresh
        // kernel (one entry per RelayRole). Assert the vector length AND every
        // element field-for-field through the same RelayStatus encoder.
        let json_relay_statuses = json["relay_statuses"].as_array().expect("relay_statuses array");
        let typed_relay_statuses = frame.relay_statuses().expect("typed relay_statuses present");
        assert_eq!(
            typed_relay_statuses.len(),
            json_relay_statuses.len(),
            "typed relay_statuses vector length agrees with JSON"
        );
        assert!(
            !json_relay_statuses.is_empty(),
            "a fresh kernel emits one relay_status per RelayRole — vector must be non-empty \
             so the element-level assertion below is not vacuous"
        );
        for (index, json_status) in json_relay_statuses.iter().enumerate() {
            assert_relay_status_agrees(&typed_relay_statuses.get(index), json_status);
        }
        assert_eq!(
            frame.logs().map_or(0, |v| v.len()),
            json["logs"].as_array().expect("logs array").len()
        );
        // `logical_interests` is ALWAYS non-empty (Profile + Timeline pushed
        // unconditionally), so assert every element field-for-field — this is
        // the only coverage of the LogicalInterestStatus encoder, including its
        // nested `relay_urls:[string]` vector.
        let json_interests =
            json["logical_interests"].as_array().expect("logical_interests array");
        let typed_interests = frame.logical_interests().expect("typed logical_interests present");
        assert_eq!(typed_interests.len(), json_interests.len(), "logical_interests vector length");
        assert!(
            !json_interests.is_empty(),
            "Profile + Timeline interests are always present — vector must be non-empty"
        );
        for (index, json_interest) in json_interests.iter().enumerate() {
            assert_logical_interest_agrees(&typed_interests.get(index), json_interest);
        }
        // wire_subscriptions is empty on a bare kernel; the populated case is
        // covered field-for-field by the dedicated test below.
        assert_eq!(
            frame.wire_subscriptions().map_or(0, |v| v.len()),
            json["wire_subscriptions"].as_array().expect("wire_subscriptions array").len()
        );

        // Optional diagnostics: absent in JSON (skip_serializing_if) ⇒ typed None.
        assert!(
            !json.as_object().map(|o| o.contains_key("store_open_failure")).unwrap_or(false),
            "healthy kernel must omit store_open_failure from JSON"
        );
        assert_eq!(
            frame.store_open_failure(),
            None,
            "absent store_open_failure must read back as typed None"
        );
        assert!(
            !json.as_object().map(|o| o.contains_key("no_configured_relays")).unwrap_or(false),
            "healthy kernel must omit no_configured_relays from JSON"
        );
        assert_eq!(
            frame.no_configured_relays(),
            None,
            "absent no_configured_relays must read back as typed None"
        );
        // The error trio serializes to JSON null (no skip_serializing_if) and
        // must read back as typed None in steady state.
        assert_eq!(frame.last_error_toast(), None);
        assert_eq!(frame.last_planner_error(), None);
    });
}

/// When `store_open_failure` is set, the typed field carries the exact same
/// string the struct-serialized JSON does — the present case for an `Option<String>`.
#[test]
fn adr0044_typed_store_open_failure_agrees_with_json_when_present() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let reason = "LMDB open failed: No such file or directory (os error 2)";
    kernel.set_store_open_failure_for_test(reason);

    let (bytes, json) = kernel.make_update_frame_and_json_for_test(true);

    assert_eq!(
        json["store_open_failure"].as_str(),
        Some(reason),
        "JSON payload must carry the failure string"
    );
    with_snapshot_frame(&bytes, |frame| {
        assert_eq!(
            frame.store_open_failure(),
            Some(reason),
            "typed store_open_failure must agree with the JSON payload string"
        );
    });
}

/// Drive a wire-subscription into the kernel via the production registration
/// path (`register_wire_frames_for_test` mirrors the actor wiring), then assert
/// the typed `wire_subscriptions` vector element agrees field-for-field with the
/// JSON — exercising the `WireSubscriptionStatus` encoder, which is empty (and
/// therefore untested) on a bare kernel. Covers `Option<String> close_reason` in
/// the `None` state and the `u128 -> u64` `opened_at_ms` cast.
#[test]
fn adr0044_typed_wire_subscriptions_agree_with_json_when_populated() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let frames = vec![crate::subs::WireFrame::Req {
        relay_url: "wss://relay.example".to_string(),
        sub_id: "sub-test-tier3".to_string(),
        filter_json: r#"{"kinds":[1],"limit":10}"#.to_string(),
        interest_id: crate::planner::InterestId(0),
        lifecycle: crate::planner::InterestLifecycle::OneShot,
    }];
    kernel.register_wire_frames_for_test(&frames);

    let (bytes, json) = kernel.make_update_frame_and_json_for_test(true);
    let json_subs = json["wire_subscriptions"].as_array().expect("wire_subscriptions array");
    assert!(
        !json_subs.is_empty(),
        "registering a wire frame must populate wire_subscriptions so this assertion is not vacuous"
    );

    with_snapshot_frame(&bytes, |frame| {
        let typed_subs = frame.wire_subscriptions().expect("typed wire_subscriptions present");
        assert_eq!(typed_subs.len(), json_subs.len(), "wire_subscriptions vector length");
        for (index, json_sub) in json_subs.iter().enumerate() {
            let sub = typed_subs.get(index);
            assert_eq!(sub.wire_id(), json_opt_str(json_sub, "wire_id"), "wire_id");
            assert_eq!(sub.relay_url(), json_opt_str(json_sub, "relay_url"), "relay_url");
            assert_eq!(
                sub.filter_summary(),
                json_opt_str(json_sub, "filter_summary"),
                "filter_summary"
            );
            assert_eq!(sub.state(), json_opt_str(json_sub, "state"), "state");
            assert_eq!(
                u64::from(sub.logical_consumer_count()),
                json_u64(json_sub, "logical_consumer_count"),
                "logical_consumer_count"
            );
            assert_eq!(sub.events_rx(), json_u64(json_sub, "events_rx"), "events_rx");
            assert_eq!(sub.opened_at_ms(), json_u64(json_sub, "opened_at_ms"), "opened_at_ms");
            assert_eq!(
                sub.last_event_at_ms(),
                json_opt_u64(json_sub, "last_event_at_ms"),
                "last_event_at_ms"
            );
            assert_eq!(sub.eose_at_ms(), json_opt_u64(json_sub, "eose_at_ms"), "eose_at_ms");
            assert_eq!(
                sub.close_reason(),
                json_opt_str(json_sub, "close_reason"),
                "close_reason (Option<String>, None on a freshly opened sub)"
            );
        }
    });
}

/// Measure the wire frame size produced by an empty kernel (no payload:Value emitted).
/// Prints `PRB_FRAME_SIZE total_frame=NNN json_payload_absent=true` for CI logs.
/// This is the final zeroing proof: frame should be much smaller than the old 14 504 B
/// (which included a 4 457 B JSON blob = ~31% overhead).
#[test]
fn prb_frame_size_no_payload() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (bytes, json) = kernel.make_update_frame_and_json_for_test(true);
    let json_str = serde_json::to_string(&json).expect("json serializable");
    let json_snapshot_bytes = json_str.len();
    println!(
        "PRB_FRAME_SIZE total_frame={} json_snapshot_bytes_not_on_wire={}",
        bytes.len(),
        json_snapshot_bytes,
    );
    // Payload absence is now a COMPILE-TIME guarantee: the regenerated
    // transport bindings (`payload:Value (deprecated)`) expose no `payload()`
    // / `add_payload` accessor, so no Rust code can write or read the slot.
    // The runtime assertion left here is the size envelope: the old
    // empty-kernel frame (payload emitted) was 14 504 B, of which the JSON
    // blob alone was 4 457 B. With the blob gone the frame must stay well
    // under the old floor — hard failure if a generic blob ever sneaks back.
    assert!(
        bytes.len() < 10_000,
        "empty-kernel frame ballooned to {} bytes — did the generic payload return?",
        bytes.len()
    );
    // And the frame still decodes through both typed paths.
    let envelope = crate::update_envelope::decode_snapshot_envelope(&bytes).expect("envelope");
    assert!(envelope.running);
    crate::update_envelope::decode_snapshot_typed_projections(&bytes).expect("typed sidecar");
}

/// JSON `Option<u128>` field: a number when `Some`, absent/null when `None`.
/// Must equal the typed native-optional `Option<u64>` accessor.
fn json_opt_u64(json: &serde_json::Value, key: &str) -> Option<u64> {
    json.get(key).and_then(serde_json::Value::as_u64)
}

fn json_u64(json: &serde_json::Value, key: &str) -> u64 {
    json[key].as_u64().unwrap_or_else(|| panic!("metric {key} must be a u64: {:?}", json.get(key)))
}

/// Assert EVERY `Metrics` field agrees between the typed table and the JSON map.
fn assert_metrics_agree(metrics: &fb::Metrics<'_>, json: &serde_json::Value) {
    macro_rules! u64_field {
        ($name:ident) => {
            assert_eq!(
                metrics.$name(),
                json_u64(json, stringify!($name)),
                concat!("Metrics::", stringify!($name), " typed vs JSON")
            );
        };
    }
    macro_rules! opt_field {
        ($name:ident) => {
            assert_eq!(
                metrics.$name(),
                json_opt_u64(json, stringify!($name)),
                concat!("Metrics::", stringify!($name), " (optional) typed vs JSON")
            );
        };
    }
    u64_field!(generated_events);
    u64_field!(note_events);
    u64_field!(profile_events);
    u64_field!(duplicate_events);
    u64_field!(delete_events);
    u64_field!(stored_events);
    u64_field!(tombstones);
    u64_field!(visible_items);
    u64_field!(visible_profiled_items);
    u64_field!(visible_placeholder_avatar_items);
    assert_eq!(u64::from(metrics.open_views()), json_u64(json, "open_views"));
    u64_field!(events_since_last_update);
    u64_field!(diagnostic_firehose_events);
    u64_field!(inserted_count);
    u64_field!(updated_count);
    u64_field!(removed_count);
    assert_eq!(
        u64::from(metrics.events_per_second_configured()),
        json_u64(json, "events_per_second_configured")
    );
    assert_eq!(u64::from(metrics.emit_hz_configured()), json_u64(json, "emit_hz_configured"));
    u64_field!(update_sequence);
    u64_field!(estimated_store_bytes);
    u64_field!(payload_bytes);
    assert_eq!(
        metrics.store_to_payload_ratio(),
        json["store_to_payload_ratio"].as_f64().expect("store_to_payload_ratio"),
        "Metrics::store_to_payload_ratio typed vs JSON"
    );
    assert_eq!(u64::from(metrics.actor_queue_depth()), json_u64(json, "actor_queue_depth"));
    u64_field!(frames_rx);
    u64_field!(events_rx);
    u64_field!(eose_rx);
    u64_field!(notices_rx);
    u64_field!(closed_rx);
    u64_field!(bytes_rx);
    u64_field!(bytes_tx);
    u64_field!(contacts_authors);
    u64_field!(timeline_authors);
    // The Option<u128> timestamp cluster — the most likely transposition site.
    opt_field!(first_event_ms);
    opt_field!(target_profile_loaded_ms);
    opt_field!(timeline_opened_ms);
    opt_field!(timeline_first_item_ms);
    opt_field!(update_emitted_ms);
    opt_field!(last_event_to_emit_ms);
    // The u128 -> u64 trio (non-optional).
    u64_field!(max_event_to_emit_ms);
    u64_field!(max_events_per_update);
    u64_field!(dispatch_drops_total);
    u64_field!(claim_drops_total);
    u64_field!(make_update_us);
    u64_field!(serialize_us);
    u64_field!(update_frame_degradations_total);
}

/// JSON `Option<String>` relay-status field: a string when `Some`, null when
/// `None`. Must equal the typed `Option<&str>` accessor.
fn json_opt_str<'a>(json: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    json.get(key).and_then(serde_json::Value::as_str)
}

/// Assert EVERY `RelayStatus` field agrees between the typed table and the JSON
/// object — covers the 17-field shared encoder (used by both the singular
/// aggregate and each `relay_statuses` element).
fn assert_relay_status_agrees(status: &fb::RelayStatus<'_>, json: &serde_json::Value) {
    assert_eq!(status.role(), json_opt_str(json, "role"), "RelayStatus::role");
    assert_eq!(status.relay_url(), json_opt_str(json, "relay_url"), "RelayStatus::relay_url");
    assert_eq!(status.connection(), json_opt_str(json, "connection"), "RelayStatus::connection");
    assert_eq!(status.auth(), json_opt_str(json, "auth"), "RelayStatus::auth");
    assert_eq!(
        status.negentropy_probe(),
        json_opt_str(json, "negentropy_probe"),
        "RelayStatus::negentropy_probe"
    );
    assert_eq!(
        status.active_wire_subscriptions(),
        json_u64(json, "active_wire_subscriptions"),
        "RelayStatus::active_wire_subscriptions"
    );
    assert_eq!(
        u64::from(status.reconnect_count()),
        json_u64(json, "reconnect_count"),
        "RelayStatus::reconnect_count"
    );
    assert_eq!(
        status.last_connected_at_ms(),
        json_opt_u64(json, "last_connected_at_ms"),
        "RelayStatus::last_connected_at_ms"
    );
    assert_eq!(
        status.last_event_at_ms(),
        json_opt_u64(json, "last_event_at_ms"),
        "RelayStatus::last_event_at_ms"
    );
    assert_eq!(status.last_notice(), json_opt_str(json, "last_notice"), "RelayStatus::last_notice");
    assert_eq!(status.last_error(), json_opt_str(json, "last_error"), "RelayStatus::last_error");
    assert_eq!(
        status.error_category(),
        json_opt_str(json, "error_category"),
        "RelayStatus::error_category"
    );
    assert_eq!(status.events_rx(), json_u64(json, "events_rx"), "RelayStatus::events_rx");
    assert_eq!(status.bytes_rx(), json_u64(json, "bytes_rx"), "RelayStatus::bytes_rx");
    assert_eq!(status.bytes_tx(), json_u64(json, "bytes_tx"), "RelayStatus::bytes_tx");
    assert_eq!(
        status.denied(),
        json["denied"].as_bool().expect("denied"),
        "RelayStatus::denied"
    );
    assert_eq!(
        status.last_close_reason(),
        json_opt_str(json, "last_close_reason"),
        "RelayStatus::last_close_reason"
    );
}

/// Assert every `LogicalInterestStatus` field agrees, including the nested
/// `relay_urls:[string]` vector.
fn assert_logical_interest_agrees(interest: &fb::LogicalInterestStatus<'_>, json: &serde_json::Value) {
    assert_eq!(interest.key(), json_opt_str(json, "key"), "LogicalInterestStatus::key");
    assert_eq!(interest.state(), json_opt_str(json, "state"), "LogicalInterestStatus::state");
    assert_eq!(
        u64::from(interest.refcount()),
        json_u64(json, "refcount"),
        "LogicalInterestStatus::refcount"
    );
    assert_eq!(
        interest.cache_coverage(),
        json_opt_str(json, "cache_coverage"),
        "LogicalInterestStatus::cache_coverage"
    );
    assert_eq!(
        interest.warming_until_ms(),
        json_opt_u64(json, "warming_until_ms"),
        "LogicalInterestStatus::warming_until_ms"
    );
    let json_urls = json["relay_urls"].as_array().expect("relay_urls array");
    let typed_urls = interest.relay_urls().expect("typed relay_urls present");
    assert_eq!(typed_urls.len(), json_urls.len(), "LogicalInterestStatus::relay_urls length");
    for (index, json_url) in json_urls.iter().enumerate() {
        assert_eq!(
            typed_urls.get(index),
            json_url.as_str().expect("relay_url string"),
            "LogicalInterestStatus::relay_urls[{index}]"
        );
    }
}
