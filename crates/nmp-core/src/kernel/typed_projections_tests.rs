//! End-to-end proof for the Tier-2 (kernel-owned built-in) typed-projection
//! sidecars — the Wave C pattern (ADR-0037).
//!
//! The bar (mirroring the Tier-1 proof
//! `crates/nmp-defaults/tests/typed_dm_runtime_sidecar.rs`): a built-in
//! typed projection must appear in the `typed_projections` sidecar of the frame
//! `make_update` actually emits — decoded back to its typed struct — IN ADDITION
//! to its existing generic `Value` entry under the SAME key. This drives the
//! real frame path, NOT `run_typed_projections()` (which sees only the host
//! registry, never the kernel-owned built-ins).

use super::typed_projections::{
    decode_configured_relays, decode_outbox_summary, decode_publish_outbox, decode_publish_queue,
    decode_relay_role_options, decode_settings_hub, CONFIGURED_RELAYS_FILE_IDENTIFIER,
    CONFIGURED_RELAYS_SCHEMA_ID, CONFIGURED_RELAYS_SCHEMA_VERSION, OUTBOX_SUMMARY_FILE_IDENTIFIER,
    OUTBOX_SUMMARY_SCHEMA_ID, PUBLISH_OUTBOX_FILE_IDENTIFIER, PUBLISH_OUTBOX_SCHEMA_ID,
    PUBLISH_QUEUE_FILE_IDENTIFIER, PUBLISH_QUEUE_SCHEMA_ID, RELAY_ROLE_OPTIONS_FILE_IDENTIFIER,
    RELAY_ROLE_OPTIONS_SCHEMA_ID, SETTINGS_HUB_FILE_IDENTIFIER, SETTINGS_HUB_SCHEMA_ID,
};
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::update_envelope::TypedProjectionData;

fn typed_entry<'a>(typed: &'a [TypedProjectionData], key: &str) -> &'a TypedProjectionData {
    typed
        .iter()
        .find(|t| t.key == key)
        .unwrap_or_else(|| panic!("typed sidecar must carry a `{key}` entry; got {typed:?}"))
}

/// All three relay/settings built-ins land in the `typed_projections` sidecar of
/// the emitted frame, decode back to their typed structs, AND keep their generic
/// `Value` entries (additivity).
#[test]
fn relay_settings_builtins_emit_typed_sidecars_alongside_json() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Seed a configured relay so the proof exercises non-empty nested data
    // end-to-end through the real frame (not the trivial 0 == 0 empty case) —
    // the template ~20 built-ins inherit must demonstrate populated rows.
    kernel.set_configured_relays(vec![crate::kernel::AppRelay::new(
        "wss://seed.example/".to_string(),
        "both".to_string(),
    )]);
    let (value, typed) = kernel.make_update_typed_for_test(true);

    let projections = value
        .get("projections")
        .and_then(serde_json::Value::as_object)
        .expect("snapshot must carry a projections object");

    // --- configured_relays --------------------------------------------------
    assert!(
        projections.contains_key("configured_relays"),
        "the generic JSON `configured_relays` entry must remain (additive)"
    );
    let cr = typed_entry(&typed, "configured_relays");
    assert_eq!(cr.schema_id, CONFIGURED_RELAYS_SCHEMA_ID);
    assert_eq!(cr.schema_version, CONFIGURED_RELAYS_SCHEMA_VERSION);
    assert_eq!(
        cr.file_identifier.as_bytes(),
        CONFIGURED_RELAYS_FILE_IDENTIFIER
    );
    let decoded_relays =
        decode_configured_relays(&cr.payload).expect("configured_relays sidecar must decode");
    let json_relays = projections
        .get("configured_relays")
        .and_then(serde_json::Value::as_array)
        .expect("configured_relays JSON must be an array");
    assert_eq!(
        decoded_relays.relays.len(),
        json_relays.len(),
        "typed and JSON configured_relays must carry the same row count"
    );
    // Non-empty nested data survives the round-trip through the real frame, and
    // the typed row agrees field-for-field with the JSON row.
    assert_eq!(
        decoded_relays.relays.len(),
        1,
        "the seeded relay must appear"
    );
    assert_eq!(decoded_relays.relays[0].url, "wss://seed.example/");
    assert_eq!(decoded_relays.relays[0].role, "both");
    assert_eq!(
        json_relays[0]
            .get("url")
            .and_then(serde_json::Value::as_str),
        Some(decoded_relays.relays[0].url.as_str()),
        "typed and JSON configured_relays[0].url must agree"
    );
    assert_eq!(
        json_relays[0]
            .get("role")
            .and_then(serde_json::Value::as_str),
        Some(decoded_relays.relays[0].role.as_str()),
        "typed and JSON configured_relays[0].role must agree"
    );

    // --- relay_role_options -------------------------------------------------
    assert!(
        projections.contains_key("relay_role_options"),
        "the generic JSON `relay_role_options` entry must remain (additive)"
    );
    let rro = typed_entry(&typed, "relay_role_options");
    assert_eq!(rro.schema_id, RELAY_ROLE_OPTIONS_SCHEMA_ID);
    assert_eq!(
        rro.file_identifier.as_bytes(),
        RELAY_ROLE_OPTIONS_FILE_IDENTIFIER
    );
    let decoded_options =
        decode_relay_role_options(&rro.payload).expect("relay_role_options sidecar must decode");
    let json_options = projections
        .get("relay_role_options")
        .and_then(serde_json::Value::as_array)
        .expect("relay_role_options JSON must be an array");
    assert_eq!(
        decoded_options.options.len(),
        json_options.len(),
        "typed and JSON relay_role_options must carry the same option count"
    );
    assert!(
        !decoded_options.options.is_empty(),
        "relay_role_options is a static non-empty option set"
    );
    // Field-for-field agreement on the first option (value/label/tint/is_default).
    let first_typed = &decoded_options.options[0];
    let first_json = &json_options[0];
    assert_eq!(
        first_json.get("value").and_then(serde_json::Value::as_str),
        Some(first_typed.value.as_str()),
        "typed and JSON relay_role_options[0].value must agree"
    );
    assert_eq!(
        first_json
            .get("is_default")
            .and_then(serde_json::Value::as_bool),
        Some(first_typed.is_default),
        "typed and JSON relay_role_options[0].is_default must agree"
    );

    // --- settings_hub -------------------------------------------------------
    let sh_json = projections
        .get("settings_hub")
        .and_then(serde_json::Value::as_object)
        .expect("the generic JSON `settings_hub` entry must remain (additive)");
    let sh = typed_entry(&typed, "settings_hub");
    assert_eq!(sh.schema_id, SETTINGS_HUB_SCHEMA_ID);
    assert_eq!(sh.file_identifier.as_bytes(), SETTINGS_HUB_FILE_IDENTIFIER);
    let decoded_hub = decode_settings_hub(&sh.payload).expect("settings_hub sidecar must decode");
    let json_count = sh_json
        .get("relay_count")
        .and_then(serde_json::Value::as_u64)
        .expect("settings_hub JSON must carry relay_count");
    assert_eq!(
        u64::from(decoded_hub.relay_count),
        json_count,
        "typed and JSON settings_hub.relay_count must agree"
    );
}

/// Wave C: all three publish/outbox built-ins land in the `typed_projections`
/// sidecar of the emitted frame, decode back to their typed structs, AND keep
/// their generic `Value` entries (additivity).
///
/// `publish_queue` / `publish_outbox` are empty in a fresh kernel (no publish
/// in flight) — the populated-rows round-trip is carried by the per-codec
/// `*_fb_tests.rs` unit tests (driving the publish engine here would add no
/// coverage the codecs don't already have). This test pins the end-to-end frame
/// contract: typed sidecar present, decodes, count-agrees with JSON, and the
/// JSON entry survives. `outbox_summary` is non-empty even at `total = 0`, so it
/// also proves field-for-field string agreement through the real frame.
#[test]
fn publish_cluster_builtins_emit_typed_sidecars_alongside_json() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (value, typed) = kernel.make_update_typed_for_test(true);

    let projections = value
        .get("projections")
        .and_then(serde_json::Value::as_object)
        .expect("snapshot must carry a projections object");

    // --- publish_queue ------------------------------------------------------
    let json_queue = projections
        .get("publish_queue")
        .and_then(serde_json::Value::as_array)
        .expect("the generic JSON `publish_queue` entry must remain (additive)");
    let pq = typed_entry(&typed, "publish_queue");
    assert_eq!(pq.schema_id, PUBLISH_QUEUE_SCHEMA_ID);
    assert_eq!(pq.file_identifier.as_bytes(), PUBLISH_QUEUE_FILE_IDENTIFIER);
    let decoded_queue =
        decode_publish_queue(&pq.payload).expect("publish_queue sidecar must decode");
    assert_eq!(
        decoded_queue.entries.len(),
        json_queue.len(),
        "typed and JSON publish_queue must carry the same entry count"
    );

    // --- publish_outbox -----------------------------------------------------
    let json_outbox = projections
        .get("publish_outbox")
        .and_then(serde_json::Value::as_array)
        .expect("the generic JSON `publish_outbox` entry must remain (additive)");
    let po = typed_entry(&typed, "publish_outbox");
    assert_eq!(po.schema_id, PUBLISH_OUTBOX_SCHEMA_ID);
    assert_eq!(
        po.file_identifier.as_bytes(),
        PUBLISH_OUTBOX_FILE_IDENTIFIER
    );
    let decoded_outbox =
        decode_publish_outbox(&po.payload).expect("publish_outbox sidecar must decode");
    assert_eq!(
        decoded_outbox.items.len(),
        json_outbox.len(),
        "typed and JSON publish_outbox must carry the same item count"
    );

    // --- outbox_summary (non-empty even at total = 0) -----------------------
    let os_json = projections
        .get("outbox_summary")
        .and_then(serde_json::Value::as_object)
        .expect("the generic JSON `outbox_summary` entry must remain (additive)");
    let os = typed_entry(&typed, "outbox_summary");
    assert_eq!(os.schema_id, OUTBOX_SUMMARY_SCHEMA_ID);
    assert_eq!(
        os.file_identifier.as_bytes(),
        OUTBOX_SUMMARY_FILE_IDENTIFIER
    );
    let decoded_summary =
        decode_outbox_summary(&os.payload).expect("outbox_summary sidecar must decode");
    // The kernel owns the English strings even with an empty outbox.
    assert!(
        !decoded_summary.title.is_empty(),
        "outbox_summary.title is always non-empty (D1)"
    );
    assert_eq!(
        os_json.get("title").and_then(serde_json::Value::as_str),
        Some(decoded_summary.title.as_str()),
        "typed and JSON outbox_summary.title must agree"
    );
    assert_eq!(
        os_json.get("subtitle").and_then(serde_json::Value::as_str),
        Some(decoded_summary.subtitle.as_str()),
        "typed and JSON outbox_summary.subtitle must agree"
    );
    assert_eq!(
        os_json.get("total").and_then(serde_json::Value::as_u64),
        Some(u64::from(decoded_summary.total)),
        "typed and JSON outbox_summary.total must agree"
    );
}

/// The Tier-2 built-in sidecars are emitted even when NO host typed projection
/// is registered — they do not depend on the registry path. (A kernel built
/// outside the actor has no projection slot bound at all.)
#[test]
fn builtins_emit_without_any_host_typed_registration() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (_value, typed) = kernel.make_update_typed_for_test(true);
    let keys: std::collections::BTreeSet<&str> = typed.iter().map(|t| t.key.as_str()).collect();
    assert!(keys.contains("configured_relays"));
    assert!(keys.contains("relay_role_options"));
    assert!(keys.contains("settings_hub"));
    // Wave C publish/outbox cluster.
    assert!(keys.contains("publish_queue"));
    assert!(keys.contains("publish_outbox"));
    assert!(keys.contains("outbox_summary"));
    // Wave C identity cluster: accounts / active_account / profile are
    // unconditional.
    // V-112 (ADR-0042): author_view / thread_view deleted from typed sidecars.
    assert!(keys.contains("accounts"));
    assert!(keys.contains("active_account"));
    assert!(keys.contains("profile"));
    // Wave C profile/event cluster: all four are unconditional (`{}` when empty),
    // so they appear even on a fresh kernel.
    assert!(keys.contains("mention_profiles"));
    assert!(keys.contains("claimed_profiles"));
    assert!(keys.contains("claimed_events"));
    assert!(keys.contains("resolved_profiles"));
    // Wave C action-lifecycle + diagnostics cluster: `relay_diagnostics` is
    // unconditional (captured every emit), so it appears on a fresh kernel; the
    // four drain-on-emit built-ins are absent in steady state (nothing settled /
    // tracked → nothing captured → no key).
    assert!(keys.contains("relay_diagnostics"));
    assert!(
        !keys.contains("action_results"),
        "action_results is absent in steady state (nothing settled this tick)"
    );
    assert!(
        !keys.contains("signed_events"),
        "signed_events is absent in steady state (nothing settled this tick)"
    );
    assert!(
        !keys.contains("action_stages"),
        "action_stages is absent in steady state (no correlation_id tracked)"
    );
    assert!(
        !keys.contains("action_lifecycle"),
        "action_lifecycle is absent in steady state (nothing tracked)"
    );
    assert_eq!(
        typed.len(),
        14,
        "the six relay/settings/publish built-ins + the three unconditional \
         identity/views built-ins (accounts / active_account / profile) + the four \
         unconditional profile/event built-ins (mention_profiles / claimed_profiles \
         / claimed_events / resolved_profiles) + `relay_diagnostics` (unconditional); \
         the two view built-ins AND the four drain-on-emit built-ins (action_results \
         / signed_events / action_stages / action_lifecycle) are absent on a fresh \
         kernel: {typed:?}"
    );
}

/// `builtin_typed_projections` is a pure read of `&self` — calling it twice on
/// an unchanged kernel yields byte-identical payloads (deterministic; no hidden
/// per-tick state).
#[test]
fn builtin_typed_projections_are_deterministic() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let first = kernel.builtin_typed_projections();
    let second = kernel.builtin_typed_projections();
    assert_eq!(first, second);
}

/// A built-in key wins on collision: a host that registers a typed projection
/// under one of the reserved built-in keys is dropped, so the kernel-owned value
/// stays authoritative AND remains the ONLY entry for that key (the host-side
/// consumer matches by first key — a surviving host entry would shadow the
/// built-in). Mirrors the documented generic-JSON contract.
#[test]
fn builtin_key_wins_over_colliding_host_typed_projection() {
    use crate::kernel::snapshot_registry::new_snapshot_projection_slot;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let slot = new_snapshot_projection_slot();
    // A host (mis)registers a typed projection under the reserved built-in key,
    // plus a non-colliding one that must pass through untouched.
    {
        let mut registry = slot.lock().unwrap();
        registry.register_typed("settings_hub", || {
            Some(TypedProjectionData {
                key: "settings_hub".to_string(),
                schema_id: "host.imposter".to_string(),
                schema_version: 99,
                file_identifier: "HOST".to_string(),
                payload: vec![0xFF, 0xFF, 0xFF, 0xFF],
                ..Default::default()
            })
        });
        registry.register_typed("host.feed", || {
            Some(TypedProjectionData {
                key: "host.feed".to_string(),
                schema_id: "host.feed".to_string(),
                schema_version: 1,
                file_identifier: "HFED".to_string(),
                payload: vec![0x01],
                ..Default::default()
            })
        });
    }
    kernel.set_snapshot_projection_handle(slot);

    let (_value, typed) = kernel.make_update_typed_for_test(true);

    // Exactly one `settings_hub` entry, and it is the kernel-owned built-in
    // (decodes as KSHB — the imposter's 0xFFFF... bytes would not).
    let settings: Vec<&TypedProjectionData> =
        typed.iter().filter(|t| t.key == "settings_hub").collect();
    assert_eq!(
        settings.len(),
        1,
        "the colliding host `settings_hub` entry must be dropped, not appended: {typed:?}"
    );
    assert_eq!(settings[0].schema_id, SETTINGS_HUB_SCHEMA_ID);
    assert!(
        decode_settings_hub(&settings[0].payload).is_ok(),
        "the surviving `settings_hub` must be the kernel-owned KSHB buffer"
    );

    // The non-colliding host projection passes through untouched.
    assert!(
        typed.iter().any(|t| t.key == "host.feed"),
        "a non-colliding host typed projection must still be emitted"
    );
}
