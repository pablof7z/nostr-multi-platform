//! Wave A proof (codec layer): the `nmp.marmot.snapshot` typed projection
//! builds a typed-sidecar entry (`TypedProjectionData`) whose `payload` decodes
//! back to the same `MarmotSnapshot` via the generated `NMMS` bindings.
//!
//! `typed_projection` returns exactly the `TypedProjectionData` the kernel's
//! `SnapshotRegistry::run_typed` collects into a frame's `typed_projections`
//! sidecar. The trait→sidecar surface itself is proven generically by
//! `nmp-ffi::snapshot::typed_projection_registered_through_trait_surfaces_in_sidecar`
//! (anything registered via `register_typed` surfaces in `run_typed`); the
//! inherent `NmpApp::register_typed_snapshot_projection` marmot uses delegates
//! to the SAME `snapshot_projections.register_typed` registry (snapshot.rs:53),
//! so that generic test covers the marmot registration path too. `nmp-marmot`'s
//! `run_typed` helper is `pub(crate)` to `nmp-ffi`, so this in-crate test proves
//! the marmot-specific schema identity + encode/decode round-trip on populated,
//! nested, Option-bearing data (the wallet/wot template pattern).

use super::{
    decode_marmot_snapshot, typed_projection, FILE_IDENTIFIER, SCHEMA_ID, SCHEMA_VERSION,
};
use crate::projection::payload::{
    KeyPackageStatus, LastOpError, MarmotGroupRow, MarmotSnapshot, PendingOpRow, PendingWelcomeRow,
};

/// A fully-populated snapshot exercising every nested vector and `Option`
/// branch: two groups (one with both counts present, one with both absent),
/// a pending welcome, a non-default key package with all options present.
/// Presentation fields (`display_name`, `initials`, `invites_chip_label`,
/// `display_label`) are absent — shells compute those from raw data.
fn populated() -> MarmotSnapshot {
    MarmotSnapshot {
        groups: vec![
            MarmotGroupRow {
                id_hex: "a".repeat(64),
                name: "Marmot Test".to_string(),
                members: vec!["b".repeat(64), "c".repeat(64)],
                member_count: 2,
                unread_count: Some(7),
                last_msg_at: Some(1_700_000_123),
            },
            MarmotGroupRow {
                id_hex: "d".repeat(64),
                name: String::new(),
                members: vec!["e".repeat(64)],
                member_count: 1,
                unread_count: None,
                last_msg_at: None,
            },
        ],
        pending_welcomes: vec![PendingWelcomeRow {
            id_hex: "f".repeat(64),
            group_name: "Invite Group".to_string(),
            inviter_npub: "1".repeat(64),
        }],
        key_package: KeyPackageStatus {
            published: true,
            d_tag: Some("kp-d-tag".to_string()),
            age_secs: Some(42),
            stale: false,
            is_registered: true,
        },
        cached_kp_pubkeys: vec!["2".repeat(64), "3".repeat(64)],
        is_registered: true,
        orphaned_commit_count: 3,
        keyring_unavailable: false,
        pending_ops: vec![PendingOpRow {
            correlation_id: "corr-test-1".to_string(),
            op_tag: "create_group".to_string(),
            missing_count: 2,
            age_secs: 12,
        }],
        last_op_error: Some(LastOpError {
            op: "invite".to_string(),
            reason: "key_package_unavailable".to_string(),
            at_secs: 1_700_000_500,
            correlation_id: "corr-failed-1".to_string(),
        }),
    }
}

#[test]
fn typed_projection_carries_schema_identity_and_round_trips_populated() {
    let snapshot = populated();
    let entry = typed_projection(&snapshot);

    // Schema identity the host's NMMS decoder keys off.
    assert_eq!(entry.key, "nmp.marmot.snapshot");
    assert_eq!(entry.schema_id, SCHEMA_ID);
    assert_eq!(entry.schema_id, "nmp.marmot.snapshot");
    assert_eq!(entry.schema_version, SCHEMA_VERSION);
    assert_eq!(entry.file_identifier, "NMMS");
    assert_eq!(String::from_utf8_lossy(FILE_IDENTIFIER).into_owned(), "NMMS");
    assert!(
        !entry.payload.is_empty(),
        "the typed sidecar payload must carry the encoded snapshot bytes"
    );

    // The bytes in the sidecar decode back to the original struct via the
    // generated NMMS bindings — nested groups, members, welcome, key package,
    // and every Option field round-trip exactly.
    let decoded =
        decode_marmot_snapshot(&entry.payload).expect("sidecar payload must decode as NMMS");
    assert_eq!(decoded, snapshot);
}

#[test]
fn empty_snapshot_round_trips() {
    let snapshot = MarmotSnapshot::empty();
    let decoded = decode_marmot_snapshot(&typed_projection(&snapshot).payload)
        .expect("empty snapshot must decode");
    assert_eq!(decoded, snapshot);
    assert!(decoded.groups.is_empty());
    assert!(decoded.pending_welcomes.is_empty());
    assert!(!decoded.is_registered);
}

#[test]
fn absent_options_round_trip_as_none_not_zero_or_empty() {
    let mut snapshot = populated();
    // Force every Option to its absent state.
    snapshot.groups[0].unread_count = None;
    snapshot.groups[0].last_msg_at = None;
    snapshot.key_package.d_tag = None;
    snapshot.key_package.age_secs = None;

    let decoded = decode_marmot_snapshot(&typed_projection(&snapshot).payload)
        .expect("absent-option snapshot must decode");
    assert_eq!(decoded.groups[0].unread_count, None);
    assert_eq!(decoded.groups[0].last_msg_at, None);
    assert_eq!(decoded.key_package.d_tag, None);
    assert_eq!(decoded.key_package.age_secs, None);
    assert_eq!(decoded, snapshot);
}

#[test]
fn present_empty_string_options_round_trip_distinct_from_absent() {
    let mut snapshot = populated();
    // `Some("")` must NOT collapse to `None` on the wire.
    snapshot.key_package.d_tag = Some(String::new());

    let decoded = decode_marmot_snapshot(&typed_projection(&snapshot).payload)
        .expect("present-empty options must decode");
    assert_eq!(decoded.key_package.d_tag, Some(String::new()));
}

#[test]
fn present_zero_count_round_trips_distinct_from_absent() {
    let mut snapshot = populated();
    // `Some(0)` must NOT collapse to `None`.
    snapshot.groups[0].unread_count = Some(0);
    snapshot.groups[0].last_msg_at = Some(0);
    snapshot.key_package.age_secs = Some(0);

    let decoded = decode_marmot_snapshot(&typed_projection(&snapshot).payload)
        .expect("present-zero counts must decode");
    assert_eq!(decoded.groups[0].unread_count, Some(0));
    assert_eq!(decoded.groups[0].last_msg_at, Some(0));
    assert_eq!(decoded.key_package.age_secs, Some(0));
}

#[test]
fn decode_rejects_bytes_without_the_nmms_identifier() {
    assert!(decode_marmot_snapshot(b"not a flatbuffer").is_err());
    assert!(decode_marmot_snapshot(&[]).is_err());
    // A valid NMMG (messages) buffer must be rejected by the NMMS decoder.
    let nmmg = super::super::messages_fb::encode_marmot_messages(&[]);
    assert!(decode_marmot_snapshot(&nmmg).is_err());
}

#[test]
fn pending_ops_round_trip_and_absent_last_op_error_is_none() {
    // Two pending ops, no last_op_error.
    let mut snapshot = populated();
    snapshot.pending_ops = vec![
        PendingOpRow {
            correlation_id: "cid-a".to_string(),
            op_tag: "create_group".to_string(),
            missing_count: 1,
            age_secs: 5,
        },
        PendingOpRow {
            correlation_id: "cid-b".to_string(),
            op_tag: "invite".to_string(),
            missing_count: 3,
            age_secs: 45,
        },
    ];
    snapshot.last_op_error = None;

    let decoded = decode_marmot_snapshot(&typed_projection(&snapshot).payload)
        .expect("pending_ops snapshot must decode");
    assert_eq!(decoded.pending_ops.len(), 2, "both pending ops must survive round-trip");
    assert_eq!(decoded.pending_ops[0].correlation_id, "cid-a");
    assert_eq!(decoded.pending_ops[0].op_tag, "create_group");
    assert_eq!(decoded.pending_ops[0].missing_count, 1);
    assert_eq!(decoded.pending_ops[0].age_secs, 5, "age_secs must round-trip");
    assert_eq!(decoded.pending_ops[1].correlation_id, "cid-b");
    assert_eq!(decoded.pending_ops[1].missing_count, 3);
    assert_eq!(decoded.pending_ops[1].age_secs, 45);
    assert_eq!(decoded.last_op_error, None, "None last_op_error must remain None");
    assert_eq!(decoded, snapshot);
}

#[test]
fn empty_pending_ops_and_present_last_op_error_round_trip() {
    let mut snapshot = populated();
    snapshot.pending_ops = Vec::new();
    snapshot.last_op_error = Some(LastOpError {
        op: "create_group".to_string(),
        reason: "key_package_unavailable".to_string(),
        at_secs: 1_700_000_999,
        correlation_id: "cid-failed".to_string(),
    });

    let decoded = decode_marmot_snapshot(&typed_projection(&snapshot).payload)
        .expect("snapshot with last_op_error must decode");
    assert!(decoded.pending_ops.is_empty(), "empty pending_ops must round-trip as empty");
    let err = decoded
        .last_op_error
        .clone()
        .expect("last_op_error must be present");
    assert_eq!(err.op, "create_group");
    assert_eq!(err.reason, "key_package_unavailable");
    assert_eq!(err.at_secs, 1_700_000_999);
    assert_eq!(err.correlation_id, "cid-failed");
    assert_eq!(decoded, snapshot);
}

#[test]
fn absent_last_op_error_round_trips_as_none() {
    let mut snapshot = populated();
    snapshot.last_op_error = None;

    let decoded = decode_marmot_snapshot(&typed_projection(&snapshot).payload)
        .expect("must decode");
    assert_eq!(
        decoded.last_op_error, None,
        "an absent LastOpError table must decode to None"
    );
}
