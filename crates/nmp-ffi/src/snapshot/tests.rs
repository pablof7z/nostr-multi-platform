use super::super::{test_app_free, test_app_new};
use super::*;
use nmp_core::TypedProjectionData;
use nmp_core::substrate::SnapshotProjectionRegistrar;
use std::ffi::CString;

/// ADR-0053 — the C-ABI declaration seam unions keys into the registry's
/// declared set (read back through the shared `NmpApp` registry clone).
#[test]
fn declare_consumed_projections_unions_keys_into_registry() {
    let app = test_app_new();
    let k1 = CString::new("profile").unwrap();
    let k2 = CString::new("accounts").unwrap();
    let arr: [*const c_char; 2] = [k1.as_ptr(), k2.as_ptr()];
    nmp_app_declare_consumed_projections(app, arr.as_ptr(), arr.len());

    // SAFETY: `test_app_new` never returns null.
    let app_ref = unsafe { &*app };
    let registry = app_ref.snapshot_projections.lock().expect("registry lock");
    let declared = registry.declared_projections();
    assert!(declared.is_narrowing(), "a non-empty declaration narrows");
    assert!(declared.permits("profile"));
    assert!(declared.permits("accounts"));
    assert!(
        !declared.permits("relay_diagnostics"),
        "undeclared key is gated out once a non-empty set is declared"
    );
    drop(registry);
    test_app_free(app);
}

/// A null `app` / null `keys` / zero `len` declaration is a silent no-op (D6).
#[test]
fn declare_consumed_projections_bad_args_are_noops() {
    // Null app — must not crash.
    let k = CString::new("profile").unwrap();
    let arr: [*const c_char; 1] = [k.as_ptr()];
    nmp_app_declare_consumed_projections(std::ptr::null_mut(), arr.as_ptr(), arr.len());

    let app = test_app_new();
    // Null keys pointer — no-op, set stays empty (no narrowing).
    nmp_app_declare_consumed_projections(app, std::ptr::null(), 3);
    // Zero len — no-op.
    nmp_app_declare_consumed_projections(app, arr.as_ptr(), 0);
    // SAFETY: `test_app_new` never returns null.
    let app_ref = unsafe { &*app };
    let registry = app_ref.snapshot_projections.lock().expect("registry lock");
    assert!(
        !registry.declared_projections().is_narrowing(),
        "bad-arg declarations leave the set empty (no narrowing)"
    );
    drop(registry);
    test_app_free(app);
}

/// ADR-0037 — the typed-projection registration seam is reachable through
/// the narrow `SnapshotProjectionRegistrar` **trait** (was concrete-only on
/// `NmpApp`), so a reusable protocol/feed crate that wires through
/// `register_runtime(app: &mut impl SnapshotProjectionRegistrar)` can
/// register a typed FlatBuffers projection. This mirrors
/// `registered_typed_projection_surfaces_through_run_typed`
/// (`nmp-core/src/kernel/snapshot_registry_tests.rs`) but drives the
/// registration through `&impl AppHost` — the exact path protocol crates
/// use — and asserts the typed projection surfaces in the typed sidecar.
#[test]
fn typed_projection_registered_through_trait_surfaces_in_sidecar() {
    // Register through `&impl SnapshotProjectionRegistrar`, NOT the inherent
    // `NmpApp` method — this is the seam protocol crates reach via
    // `register_runtime`.
    fn register_via_trait(host: &impl SnapshotProjectionRegistrar) {
        host.register_typed_snapshot_projection("nmp.feed.home", || {
            Some(TypedProjectionData {
                key: "nmp.feed.home".to_string(),
                schema_id: "nmp.nip01.timeline".to_string(),
                schema_version: 1,
                file_identifier: "NFTS".to_string(),
                payload: vec![0xde, 0xad, 0xbe, 0xef],
                ..Default::default()
            })
        });
    }

    let app = test_app_new();
    // SAFETY: `test_app_new` never returns null.
    let app_ref = unsafe { &*app };
    register_via_trait(app_ref);

    let typed = app_ref.run_typed_snapshot_projections_for_test();
    let entry = typed.iter().find(|d| d.key == "nmp.feed.home").expect(
        "a typed projection registered through the SnapshotProjectionRegistrar trait must \
         surface in run_typed",
    );
    assert_eq!(entry.schema_id, "nmp.nip01.timeline");
    assert_eq!(entry.schema_version, 1);
    assert_eq!(entry.file_identifier, "NFTS");
    assert_eq!(entry.payload, vec![0xde, 0xad, 0xbe, 0xef]);
    test_app_free(app);
}

/// ADR-0049 / Blocker C — `register_typed_snapshot_projection` records a
/// truthful composition-ledger disposition:
/// - First registration for a key → `Installed`.
/// - Second registration for the same key → `ReplacedPrevious`.
/// - Over-cap new key → NO ledger entry (silent drop, not a false `Installed`).
/// - `DroppedLateWiring` is never recorded (the typed registry is live at all
///   times — there is no "post-start drop" for it).
#[test]
fn typed_projection_records_composition_ledger_disposition() {
    let app = test_app_new();
    // SAFETY: `test_app_new` never returns null.
    let app_ref = unsafe { &*app };

    // First registration: Installed.
    app_ref.register_typed_snapshot_projection("nmp.feed.home", || None);
    let ledger_json = app_ref.composition_ledger().to_json();
    let records = ledger_json["records"]
        .as_array()
        .expect("composition ledger must have a records array");
    let first = records
        .iter()
        .find(|r| r["key"] == "nmp.feed.home")
        .expect("ledger must contain an entry for nmp.feed.home after first registration");
    assert_eq!(
        first["seam"], "typed_snapshot_projection",
        "seam must be typed_snapshot_projection"
    );
    // serde derives serialize `Disposition::Installed` as `"Installed"`.
    assert_eq!(
        first["disposition"], "Installed",
        "first registration must record Installed"
    );
    assert!(
        first.get("replaced").is_none() || first["replaced"].is_null(),
        "Installed disposition must not carry a replaced field"
    );

    // Second registration for the same key: ReplacedPrevious.
    app_ref.register_typed_snapshot_projection("nmp.feed.home", || None);
    let ledger_json2 = app_ref.composition_ledger().to_json();
    let records2 = ledger_json2["records"]
        .as_array()
        .expect("records array present");
    let home_records: Vec<_> = records2
        .iter()
        .filter(|r| r["key"] == "nmp.feed.home")
        .collect();
    assert_eq!(
        home_records.len(),
        2,
        "two registrations must produce two ledger entries"
    );
    let second = &home_records[1];
    assert_eq!(
        second["disposition"], "ReplacedPrevious",
        "second registration for the same key must record ReplacedPrevious"
    );

    // Distinct key: separate Installed entry.
    app_ref.register_typed_snapshot_projection("nmp.nip17.dm_inbox", || None);
    let ledger_json3 = app_ref.composition_ledger().to_json();
    let records3 = ledger_json3["records"]
        .as_array()
        .expect("records array present");
    let dm = records3
        .iter()
        .find(|r| r["key"] == "nmp.nip17.dm_inbox")
        .expect("dm_inbox entry must be in ledger");
    assert_eq!(
        dm["disposition"], "Installed",
        "distinct key must record Installed, not ReplacedPrevious"
    );

    test_app_free(app);
}

/// Blocker C — over-cap new key must NOT produce a false `Installed` ledger
/// entry. The D5 ceiling is 64; when the 65th distinct key is registered
/// the registry drops it silently (`TypedAdmission::DroppedFull`) and the
/// caller must NOT record `Installed` for the ghost key.
#[test]
fn over_cap_typed_projection_does_not_record_installed() {
    use nmp_core::__ffi_internal::MAX_SNAPSHOT_PROJECTIONS;

    let app = test_app_new();
    // SAFETY: `test_app_new` never returns null.
    let app_ref = unsafe { &*app };

    // Fill the registry to the exact cap with distinct keys.
    for i in 0..MAX_SNAPSHOT_PROJECTIONS {
        app_ref.register_typed_snapshot_projection(format!("nmp.test.key{i}"), || None);
    }

    // Verify the cap keys are in the ledger.
    let ledger_before = app_ref.composition_ledger().to_json();
    let records_before = ledger_before["records"].as_array().expect("records array");
    let count_before = records_before.len();
    assert_eq!(
        count_before, MAX_SNAPSHOT_PROJECTIONS,
        "ledger must have exactly {MAX_SNAPSHOT_PROJECTIONS} entries after filling to cap"
    );

    // Now attempt to register one MORE distinct key — should be dropped.
    let over_cap_key = "nmp.test.over_cap_key";
    app_ref.register_typed_snapshot_projection(over_cap_key, || None);

    // The over-cap key must NOT appear in the ledger.
    let ledger_after = app_ref.composition_ledger().to_json();
    let records_after = ledger_after["records"].as_array().expect("records array");
    let has_over_cap_entry = records_after.iter().any(|r| r["key"] == over_cap_key);
    assert!(
        !has_over_cap_entry,
        "an over-cap registration must NOT produce a ledger entry (no false Installed)"
    );
    assert_eq!(
        records_after.len(),
        MAX_SNAPSHOT_PROJECTIONS,
        "ledger size must remain {MAX_SNAPSHOT_PROJECTIONS} after over-cap attempt"
    );

    // Replacing an existing key at cap IS still allowed and MUST record ReplacedPrevious.
    let first_key = "nmp.test.key0";
    app_ref.register_typed_snapshot_projection(first_key, || None);
    let ledger_after_replace = app_ref.composition_ledger().to_json();
    let records_after_replace = ledger_after_replace["records"]
        .as_array()
        .expect("records array");
    let replaced_entries: Vec<_> = records_after_replace
        .iter()
        .filter(|r| r["key"] == first_key)
        .collect();
    assert_eq!(
        replaced_entries.len(),
        2,
        "replacing an existing key at cap must produce two ledger entries"
    );
    assert_eq!(
        replaced_entries[1]["disposition"], "ReplacedPrevious",
        "second registration for an existing key at cap must record ReplacedPrevious"
    );

    test_app_free(app);
}
