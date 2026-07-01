//! Wave A proof (codec layer): the `nmp.wot.bootstrap` typed projection builds
//! a typed-sidecar entry (`TypedProjectionData`) whose `payload` decodes back
//! to the same `WotBootstrapSnapshot` via the generated `NWBS` bindings.
//!
//! `typed_projection` returns exactly the `TypedProjectionData` the kernel's
//! `SnapshotRegistry::run_typed` collects into a frame's `typed_projections`
//! sidecar (the trait→sidecar surface itself is proven generically by
//! `nmp-ffi::snapshot::typed_projection_registered_through_trait_surfaces_in_sidecar`;
//! `nmp-wot` must not depend on the C-ABI crate, so this in-crate test proves
//! the wot-specific schema identity + encode/decode round-trip, matching the
//! wallet template `crates/nmp-nip47/src/register_tests.rs`).

use super::{decode_wot_bootstrap, typed_projection, FILE_IDENTIFIER, SCHEMA_ID, SCHEMA_VERSION};
use crate::runtime::WotBootstrapSnapshot;

fn sample(active: Option<&str>) -> WotBootstrapSnapshot {
    WotBootstrapSnapshot {
        active_pubkey: active.map(str::to_string),
        active_follow_count: 1_052,
        bootstrap_requested: true,
        graph_follow_authors: 1_052,
        graph_mute_authors: 7,
    }
}

#[test]
fn typed_projection_carries_the_schema_identity_and_round_trips() {
    let snapshot = sample(Some(&"a".repeat(64)));
    let entry = typed_projection(&snapshot);

    // Schema identity the host's NWBS decoder keys off.
    assert_eq!(entry.key, "nmp.wot.bootstrap");
    assert_eq!(entry.schema_id, SCHEMA_ID);
    assert_eq!(entry.schema_id, "nmp.wot.bootstrap");
    assert_eq!(entry.schema_version, SCHEMA_VERSION);
    assert_eq!(entry.file_identifier, "NWBS");
    assert_eq!(
        String::from_utf8_lossy(FILE_IDENTIFIER).into_owned(),
        "NWBS"
    );
    assert!(
        !entry.payload.is_empty(),
        "the typed sidecar payload must carry the encoded snapshot bytes"
    );

    // The bytes in the sidecar decode back to the original struct via the
    // generated NWBS bindings — not only the generic `Value` tree.
    let decoded =
        decode_wot_bootstrap(&entry.payload).expect("sidecar payload must decode as NWBS");
    assert_eq!(decoded, snapshot);
}

#[test]
fn absent_active_pubkey_round_trips_as_none_not_empty_string() {
    let snapshot = WotBootstrapSnapshot {
        active_pubkey: None,
        active_follow_count: 0,
        bootstrap_requested: false,
        graph_follow_authors: 0,
        graph_mute_authors: 0,
    };
    let entry = typed_projection(&snapshot);
    let decoded = decode_wot_bootstrap(&entry.payload).expect("empty snapshot must decode");
    assert_eq!(decoded.active_pubkey, None);
    assert_eq!(decoded, snapshot);
}

#[test]
fn present_empty_active_pubkey_round_trips_distinct_from_absent() {
    let snapshot = sample(Some(""));
    let decoded =
        decode_wot_bootstrap(&typed_projection(&snapshot).payload).expect("decode present-empty");
    assert_eq!(
        decoded.active_pubkey,
        Some(String::new()),
        "present-empty pubkey must NOT collapse to None"
    );
}

#[test]
fn decode_rejects_bytes_without_the_nwbs_identifier() {
    assert!(decode_wot_bootstrap(b"not a flatbuffer").is_err());
    assert!(decode_wot_bootstrap(&[]).is_err());
}
