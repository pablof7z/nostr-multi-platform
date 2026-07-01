//! Tests for snapshot JSON shape.

use super::*;

#[test]
fn snapshot_json_carries_new_projections() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(),
        kind: 1,
        tags: Vec::new(),
        content: "json shape check".to_string(),
        created_at: 0,
    };
    publish_unsigned_event(
        &id,
        &mut kernel,
        unsigned,
        None,
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    add_relay(&mut kernel, "wss://relay.damus.io", "both");
    let json = kernel.make_update_json_for_test(true);
    assert!(json.contains("\"accounts\""));
    assert!(json.contains("\"active_account\""));
    assert!(json.contains("\"last_error_toast\""));
    // D0: the publish cluster (`publish_queue`, `publish_outbox`,
    // `configured_relays`) is no longer a set of typed `KernelSnapshot` fields —
    // all three are kernel-owned built-in entries in the host-extensible
    // `projections` map. They are always present (kernel-owned data, no host
    // registration step), unlike the host-registered `"bunker_handshake"`
    // projection. Decode the map and assert the keys nest under it.
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("snapshot must be valid JSON");
    let projections = parsed
        .get("projections")
        .expect("snapshot must carry the projections map once the publish cluster is populated");
    assert!(projections.get("publish_queue").is_some());
    assert!(projections.get("publish_outbox").is_some());
    assert!(projections.get("outbox_summary").is_some());
    assert!(projections.get("configured_relays").is_some());
    let role_options = projections["relay_role_options"]
        .as_array()
        .expect("relay_role_options must be a projection array");
    assert_eq!(role_options[0]["value"].as_str(), Some("both,indexer"));
    // `label` and `tint` removed from the wire (#1678/#2314, D7) — shells map
    // value to presentation locally.
    assert!(
        role_options[0].get("label").is_none(),
        "label must not appear on the wire"
    );
    assert!(
        role_options[0].get("tint").is_none(),
        "tint must not appear on the wire"
    );
    assert_eq!(role_options[1]["value"].as_str(), Some("both"));
    assert_eq!(role_options[1]["is_default"].as_bool(), Some(true));
    let relay_rows = projections["configured_relays"]
        .as_array()
        .expect("configured_relays must be a projection array");
    assert!(
        !relay_rows.is_empty(),
        "configured_relays projection must have entries"
    );
    // D0: the views cluster (`profile`) is a kernel-owned built-in entry in
    // the `projections` map. `profile` is always present.
    // V-112 (ADR-0042): `author_view` / `thread_view` deleted from projections.
    // #1610: `timeline`, `inserted`, `updated`, `removed` removed from the
    // codegen registry (JSON-era vestigials; typed feeds ship through app-owned session keys).
    // These asserts confirm the kernel never emits those legacy keys.
    assert!(projections.get("profile").is_some());
    // Kernel never emits the JSON-era timeline/delta keys (#1610).
    assert!(
        projections.get("timeline").is_none(),
        "#1610: timeline must never appear in projections (removed JSON-era key)"
    );
    assert!(
        projections.get("inserted").is_none(),
        "#1610: inserted must never appear in projections (removed JSON-era key)"
    );
    assert!(
        projections.get("updated").is_none(),
        "#1610: updated must never appear in projections (removed JSON-era key)"
    );
    assert!(
        projections.get("removed").is_none(),
        "#1610: removed must never appear in projections (removed JSON-era key)"
    );
    // V-112 (ADR-0042): `author_view` / `thread_view` deleted from snapshot.
    // `retarget_timeline` no longer calls `kernel.open_author()`.
    assert!(
        projections.get("author_view").is_none(),
        "V-112: author_view must be absent — deleted in ADR-0042 M2 migration"
    );
    assert!(
        projections.get("thread_view").is_none(),
        "V-112: thread_view must be absent — deleted in ADR-0042 M2 migration"
    );
    // The typed `KernelSnapshot` fields must be gone — a shell that still
    // reads them would silently get `null`.
    assert!(parsed.get("profile").is_none());
    assert!(parsed.get("items").is_none());
    assert!(parsed.get("author_view").is_none());
    assert!(parsed.get("thread_view").is_none());
    // D0: NIP-46 bunker handshake is no longer a typed `KernelSnapshot` field
    // — it is surfaced through the built-in `"bunker_handshake"` snapshot
    // projection registered in `nmp_app_new`. A bare `make_update` (no
    // projection registered) therefore does NOT carry the key; the projection
    // path is covered by `snapshot_carries_bunker_handshake_value` in
    // `remote_signer_tests.rs`.
}
