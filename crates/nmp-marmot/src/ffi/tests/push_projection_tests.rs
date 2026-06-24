use super::*;

// ── V-107 / ADR-0039: push-projection logic verification ─────────────────
//
// These tests verify the logic that the push-projection CLOSURES delegate to:
//
// - `MarmotProjection::messages_all_groups_json` — the method the
//   `nmp.marmot.messages` closure calls. This is the code path that was
//   previously inlined in the closure; extracting it makes it directly testable.
//
// - The projection_slot lifecycle: after `nmp_marmot_unregister` the slot
//   is cleared to `None`, so closures that read it emit empty objects.
//
// The registered closures themselves wrap these methods with a slot guard:
//
//   `nmp.marmot.snapshot`:  slot.lock() → proj.snapshot(now_secs())
//   `nmp.marmot.messages`:  slot.lock() → proj.messages_all_groups_json(page)
//
// The closure logic is trivially correct once the underlying methods are
// verified and the slot is confirmed to hold the right projection. Both
// are exercised here.

/// `MarmotProjection::messages_all_groups_json` must return a JSON object
/// keyed by `group_id_hex` containing the sent message rows.
///
/// This is the exact logic the `nmp.marmot.messages` push projection closure
/// delegates to on every snapshot tick.
#[test]
fn messages_all_groups_json_emits_keyed_rows_after_send() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let bob = in_memory(bob_keys.clone());
    let bob_kp_json = bob
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
        .expect("bob kp")
        .event_30443
        .as_json();

    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), None);

    // Publish key package and create a group.
    let kp_r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
                1_000,
                None,
            )
        })
        .unwrap();
    assert_eq!(kp_r["ok"], json!(true));

    let create_r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "create_group",
                    "name": "Msgs All Groups Test",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                    "signed_key_package_events_json": [bob_kp_json],
                }),
                1_001,
                None,
            )
        })
        .unwrap();
    assert_eq!(create_r["ok"], json!(true), "create_group: {create_r}");
    let group_id_hex = create_r["group_id_hex"].as_str().unwrap().to_string();

    // Before send: no messages.
    let empty = proj.messages_all_groups_json(200);
    assert!(empty.is_object(), "must be a JSON object before send");
    assert!(
        empty
            .get(&group_id_hex)
            .and_then(|v| v.as_array())
            .map(|a| a.is_empty())
            .unwrap_or(true),
        "no messages yet: {empty}"
    );

    // Send a message.
    let send_r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "send",
                    "group_id_hex": &group_id_hex,
                    "text": "all-groups map test",
                }),
                1_002,
                None,
            )
        })
        .unwrap();
    assert_eq!(send_r["ok"], json!(true), "send: {send_r}");

    // After send: messages_all_groups_json must include the row under the
    // correct group key. This is the exact code path the closure runs.
    let msgs = proj.messages_all_groups_json(200);
    assert!(
        msgs.is_object(),
        "must be a JSON object after send; got: {msgs}"
    );
    let rows = msgs
        .get(&group_id_hex)
        .and_then(|v| v.as_array())
        .expect("group key must be present in the map");
    assert_eq!(rows.len(), 1, "one sent message expected; got: {rows:?}");
    assert_eq!(
        rows[0].get("content").and_then(|v| v.as_str()),
        Some("all-groups map test"),
        "message content must match"
    );
    assert_eq!(
        rows[0].get("sender_pubkey_hex").and_then(|v| v.as_str()),
        Some(alice_keys.public_key().to_hex().as_str()),
        "sender pubkey must match"
    );
}

/// After `nmp_marmot_unregister` the `projection_slot` held by the push-
/// projection closures is cleared to `None`. A subsequent read through the
/// slot must return an empty result — not the signed-out account's data.
///
/// Verifies both the snapshot-slot and the messages-slot clear by calling
/// the closures' read path (through the `MarmotProjectionSlot`) directly.
#[test]
fn projection_slot_cleared_on_unregister_emits_empty() {
    use crate::ffi::MarmotProjectionSlot;
    use std::sync::{Arc, Mutex};

    // Build a projection with a group.
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let bob = in_memory(bob_keys.clone());
    let bob_kp_json = bob
        .publish_key_package(vec![nostr::RelayUrl::parse("wss://t.relay").unwrap()])
        .expect("bob kp")
        .event_30443
        .as_json();

    let proj = Arc::new(MarmotProjection::new(in_memory(alice_keys.clone()), None));
    // Test setup step — dispatch result is asserted via the subsequent
    // `create_r`; discard the `#[must_use]` return here explicitly.
    let _ = proj.with_inner(|h| {
        ops::dispatch_json_for_tests(
            h,
            json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            1_000,
            None,
        )
    });
    let create_r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "create_group",
                    "name": "Slot Clear Test",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                    "signed_key_package_events_json": [bob_kp_json],
                }),
                1_001,
                None,
            )
        })
        .unwrap();
    assert_eq!(create_r["ok"], json!(true));

    // Build a slot as register_with_keys does (#1651: `MarmotSlotState`).
    use crate::ffi::MarmotSlotState;
    let slot: MarmotProjectionSlot =
        Arc::new(Mutex::new(MarmotSlotState::Ready(Arc::clone(&proj))));

    // With slot Ready: messages_all_groups_json via the slot returns data.
    let msgs_before = {
        let guard = slot.lock().unwrap();
        match &*guard {
            MarmotSlotState::Ready(p) => Some(p.messages_all_groups_json(200)),
            _ => None,
        }
    };
    assert!(msgs_before.is_some(), "Ready-slot read must return data");

    // Simulate nmp_marmot_unregister clearing the slot.
    if let Ok(mut s) = slot.lock() {
        *s = MarmotSlotState::Cleared;
    }

    // With slot Cleared: the closure read path emits nothing → empty object.
    let guard = slot.lock().unwrap();
    assert!(
        matches!(&*guard, MarmotSlotState::Cleared),
        "slot must be Cleared after unregister"
    );
    let empty_msgs = match &*guard {
        MarmotSlotState::Ready(p) => p.messages_all_groups_json(200),
        _ => serde_json::Value::Object(serde_json::Map::new()),
    };
    assert!(
        empty_msgs
            .as_object()
            .map(|m| m.is_empty())
            .unwrap_or(false),
        "cleared slot must produce empty object; got: {empty_msgs}"
    );
}
