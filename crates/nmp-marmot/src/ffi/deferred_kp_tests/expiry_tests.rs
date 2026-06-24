use super::*;

/// A parked op whose KP never arrives expires on an explicit actor expiry edge.
#[test]
fn pending_op_expires_on_internal_edge_without_any_further_ingest() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();

    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), None);
    proj.with_inner(|h| {
        ops::dispatch_json_for_tests(
            h,
            json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            1_000,
            None,
        )
    })
    .unwrap();

    let r = proj
        .with_inner(|h| {
            ops::dispatch_json_for_tests(
                h,
                json!({
                    "op": "create_group",
                    "name": "Snapshot Expiry",
                    "relays": ["wss://t.relay"],
                    "invitee_npubs": [bob_keys.public_key().to_hex()],
                }),
                1_001,
                Some("corr-snap-expiry"),
            )
        })
        .unwrap();
    assert_eq!(r["pending"], json!(true), "must park: {r}");

    let _ = proj.snapshot(1_002);
    let summaries = proj.with_inner(|h| h.pending_op_summaries()).unwrap();
    assert_eq!(
        summaries.len(),
        1,
        "op must still be pending before deadline: {summaries:?}"
    );

    let expired_now = 1_001 + PENDING_OP_EXPIRY_SECS + 1;
    let snap_before = proj.snapshot(expired_now);
    let summaries = proj.with_inner(|h| h.pending_op_summaries()).unwrap();
    assert_eq!(
        summaries.len(),
        1,
        "snapshot must not expire pending ops: {summaries:?}"
    );
    assert_eq!(
        snap_before.pending_ops.len(),
        1,
        "snapshot still reports the parked op: {snap_before:?}"
    );

    proj.with_inner(|h| h.evict_expired_pending(expired_now))
        .unwrap();
    let snap = proj.snapshot(expired_now);
    let summaries = proj.with_inner(|h| h.pending_op_summaries()).unwrap();
    assert!(
        summaries.is_empty(),
        "op must expire on the explicit expiry edge: {summaries:?}"
    );
    assert!(snap.groups.is_empty(), "no group must be created: {snap:?}");

    let cmds = proj.with_inner(|h| h.drain_captured_commands()).unwrap();
    assert_eq!(cmds.len(), 1, "exactly one terminal command: {cmds:?}");
    assert_eq!(cmds[0].0, "failure", "verdict must be failure: {cmds:?}");
    assert_eq!(
        cmds[0].1, "corr-snap-expiry",
        "under the original correlation_id: {cmds:?}"
    );
}

/// Assert exactly one terminal command per correlation id across retry,
/// expiry, and late-KP-after-expiry outcomes.
#[test]
fn exactly_one_terminal_command_per_correlation_id() {
    let alice_keys = Keys::generate();
    let bob_keys = Keys::generate();
    let carol_keys = Keys::generate();

    let proj = MarmotProjection::new(in_memory(alice_keys.clone()), None);
    proj.with_inner(|h| {
        ops::dispatch_json_for_tests(
            h,
            json!({ "op": "publish_key_package", "relays": ["wss://t.relay"] }),
            1_000,
            None,
        )
    })
    .unwrap();

    proj.with_inner(|h| {
        ops::dispatch_json_for_tests(
            h,
            json!({
                "op": "create_group",
                "name": "Retry Success",
                "relays": ["wss://t.relay"],
                "invitee_npubs": [bob_keys.public_key().to_hex()],
            }),
            1_001,
            Some("corr-success"),
        )
    })
    .unwrap();

    proj.with_inner(|h| {
        ops::dispatch_json_for_tests(
            h,
            json!({
                "op": "create_group",
                "name": "Will Expire",
                "relays": ["wss://t.relay"],
                "invitee_npubs": [carol_keys.public_key().to_hex()],
            }),
            1_001,
            Some("corr-expire"),
        )
    })
    .unwrap();

    let bob_kp = make_kp_event(&bob_keys);
    proj.with_inner(|h| ingest_signed_event_core(h, &bob_kp, 1_002))
        .unwrap()
        .unwrap();

    let expired_now = 1_001 + PENDING_OP_EXPIRY_SECS + 1;
    proj.with_inner(|h| h.evict_expired_pending(expired_now))
        .unwrap();

    let carol_kp = make_kp_event(&carol_keys);
    proj.with_inner(|h| ingest_signed_event_core(h, &carol_kp, expired_now + 1))
        .unwrap()
        .unwrap();

    let cmds = proj.with_inner(|h| h.drain_captured_commands()).unwrap();
    assert_eq!(
        cmds.len(),
        2,
        "exactly two terminal commands total (one per op): {cmds:?}"
    );
    let success_count = cmds
        .iter()
        .filter(|(v, c)| *v == "success" && c == "corr-success")
        .count();
    let failure_count = cmds
        .iter()
        .filter(|(v, c)| *v == "failure" && c == "corr-expire")
        .count();
    assert_eq!(
        success_count, 1,
        "exactly one success for corr-success: {cmds:?}"
    );
    assert_eq!(
        failure_count, 1,
        "exactly one failure for corr-expire: {cmds:?}"
    );
    let mut ids: Vec<&String> = cmds.iter().map(|(_, c)| c).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(
        ids.len(),
        2,
        "no correlation_id may receive two terminals: {cmds:?}"
    );
}
