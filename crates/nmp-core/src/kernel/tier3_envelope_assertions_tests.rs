use crate::transport::wire as fb;

/// JSON `Option<u128>` field: a number when `Some`, absent/null when `None`.
/// Must equal the typed native-optional `Option<u64>` accessor.
pub(super) fn json_opt_u64(json: &serde_json::Value, key: &str) -> Option<u64> {
    json.get(key).and_then(serde_json::Value::as_u64)
}

pub(super) fn json_u64(json: &serde_json::Value, key: &str) -> u64 {
    json[key]
        .as_u64()
        .unwrap_or_else(|| panic!("metric {key} must be a u64: {:?}", json.get(key)))
}

/// Assert EVERY `Metrics` field agrees between the typed table and the JSON map.
pub(super) fn assert_metrics_agree(metrics: &fb::Metrics<'_>, json: &serde_json::Value) {
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
    assert_eq!(
        u64::from(metrics.open_views()),
        json_u64(json, "open_views")
    );
    u64_field!(events_since_last_update);
    u64_field!(diagnostic_firehose_events);
    u64_field!(inserted_count);
    u64_field!(updated_count);
    u64_field!(removed_count);
    assert_eq!(
        u64::from(metrics.emit_hz_configured()),
        json_u64(json, "emit_hz_configured")
    );
    u64_field!(update_sequence);
    u64_field!(estimated_store_bytes);
    u64_field!(payload_bytes);
    assert_eq!(
        metrics.store_to_payload_ratio(),
        json["store_to_payload_ratio"]
            .as_f64()
            .expect("store_to_payload_ratio"),
        "Metrics::store_to_payload_ratio typed vs JSON"
    );
    assert_eq!(
        u64::from(metrics.actor_queue_depth()),
        json_u64(json, "actor_queue_depth")
    );
    u64_field!(frames_rx);
    u64_field!(events_rx);
    u64_field!(eose_rx);
    u64_field!(notices_rx);
    u64_field!(closed_rx);
    u64_field!(bytes_rx);
    u64_field!(bytes_tx);
    u64_field!(contacts_authors);
    u64_field!(timeline_authors);
    opt_field!(first_event_ms);
    opt_field!(target_profile_loaded_ms);
    opt_field!(timeline_opened_ms);
    opt_field!(timeline_first_item_ms);
    opt_field!(update_emitted_ms);
    opt_field!(last_event_to_emit_ms);
    u64_field!(max_event_to_emit_ms);
    u64_field!(max_events_per_update);
    u64_field!(claim_drops_total);
    u64_field!(make_update_us);
    u64_field!(serialize_us);
    u64_field!(update_frame_degradations_total);
    u64_field!(command_drops);
    u64_field!(relay_backlog_drops);
    u64_field!(external_event_sink_channel_overflow_drops);
}

/// JSON `Option<String>` field: a string when `Some`, null when `None`.
/// Must equal the typed `Option<&str>` accessor.
pub(super) fn json_opt_str<'a>(json: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    json.get(key).and_then(serde_json::Value::as_str)
}

/// Assert EVERY `RelayStatus` field agrees between the typed table and the JSON
/// object, covering the shared encoder used by the aggregate and vector rows.
pub(super) fn assert_relay_status_agrees(status: &fb::RelayStatus<'_>, json: &serde_json::Value) {
    assert_eq!(
        status.role(),
        json_opt_str(json, "role"),
        "RelayStatus::role"
    );
    assert_eq!(
        status.relay_url(),
        json_opt_str(json, "relay_url"),
        "RelayStatus::relay_url"
    );
    assert_eq!(
        status.connection(),
        json_opt_str(json, "connection"),
        "RelayStatus::connection"
    );
    assert_eq!(
        status.auth(),
        json_opt_str(json, "auth"),
        "RelayStatus::auth"
    );
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
    assert_eq!(
        status.last_notice(),
        json_opt_str(json, "last_notice"),
        "RelayStatus::last_notice"
    );
    assert_eq!(
        status.last_error(),
        json_opt_str(json, "last_error"),
        "RelayStatus::last_error"
    );
    assert_eq!(
        status.error_category(),
        json_opt_str(json, "error_category"),
        "RelayStatus::error_category"
    );
    assert_eq!(
        status.events_rx(),
        json_u64(json, "events_rx"),
        "RelayStatus::events_rx"
    );
    assert_eq!(
        status.bytes_rx(),
        json_u64(json, "bytes_rx"),
        "RelayStatus::bytes_rx"
    );
    assert_eq!(
        status.bytes_tx(),
        json_u64(json, "bytes_tx"),
        "RelayStatus::bytes_tx"
    );
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

/// Assert every `LogicalInterestStatus` field agrees, including its
/// `relay_urls:[string]` vector.
pub(super) fn assert_logical_interest_agrees(
    interest: &fb::LogicalInterestStatus<'_>,
    json: &serde_json::Value,
) {
    assert_eq!(
        interest.key(),
        json_opt_str(json, "key"),
        "LogicalInterestStatus::key"
    );
    assert_eq!(
        interest.state(),
        json_opt_str(json, "state"),
        "LogicalInterestStatus::state"
    );
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
    assert_eq!(
        typed_urls.len(),
        json_urls.len(),
        "LogicalInterestStatus::relay_urls length"
    );
    for (index, json_url) in json_urls.iter().enumerate() {
        assert_eq!(
            typed_urls.get(index),
            json_url.as_str().expect("relay_url string"),
            "LogicalInterestStatus::relay_urls[{index}]"
        );
    }
}
