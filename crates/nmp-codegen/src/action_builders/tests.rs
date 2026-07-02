//! ADR-0071 §3 (#1783) — action-builder emitter tests.
//!
//! These cover the EMITTER (determinism, registry coverage, structural shape).
//! The authoritative WIRE round-trip — that builder-shaped `DispatchEnvelope`
//! bytes decode via S2 (`decode_dispatch_envelope`) and route via
//! `ActionRegistry::start_bytes` to the right namespace/payload (react + the
//! `nmp.publish` union) — lives in
//! `crates/nmp-nip25/tests/dispatch_integration.rs`, which CAN depend on
//! `nmp-core` + the nip crates (this library crate deliberately does not).

use super::*;

#[test]
fn render_is_deterministic_swift() {
    assert_eq!(render(Platform::Swift), render(Platform::Swift));
}

#[test]
fn render_is_deterministic_kotlin() {
    assert_eq!(render(Platform::Kotlin), render(Platform::Kotlin));
}

#[test]
fn render_is_deterministic_ts() {
    assert_eq!(render(Platform::Ts), render(Platform::Ts));
}

#[test]
fn ts_emits_every_namespace_and_method() {
    let s = render(Platform::Ts);
    assert!(s.contains("export const GeneratedActionBuilders"));
    // The web reuses the hand-written envelope wrapper rather than re-emitting
    // one — assert the import is present (single web source of truth).
    assert!(s.contains("import { encodeDispatchEnvelope } from \"./dispatchEnvelope\";"));
    for b in ACTION_BUILDERS {
        assert!(
            s.contains(&format!("  {}(", b.method)),
            "missing ts builder method `{}`",
            b.method
        );
        assert!(
            s.contains(&format!(
                "encodeDispatchEnvelope(correlationId, {:?}",
                b.namespace
            )),
            "missing ts namespace stamp `{}`",
            b.namespace
        );
    }
}

#[test]
fn ts_emits_the_publish_union_builders() {
    let s = render(Platform::Ts);
    for builder in registry::PUBLISH_BUILDERS {
        assert!(
            s.contains(&format!("  {}(", builder.method)),
            "missing ts publish builder method `{}`",
            builder.method
        );
    }
    assert!(s.contains("encodeDispatchEnvelope(correlationId, \"nmp.publish\""));
    assert!(s.contains("fbb.finish(payloadRoot, \"NPUB\")"));
}

#[test]
fn swift_emits_every_namespace_and_method() {
    let s = render(Platform::Swift);
    assert!(s.contains("public enum GeneratedActionBuilders"));
    for b in ACTION_BUILDERS {
        assert!(
            s.contains(&format!("public static func {}(", b.method)),
            "missing swift builder method `{}`",
            b.method
        );
        assert!(
            s.contains(&format!("actionNamespace: {:?}", b.namespace)),
            "missing swift namespace stamp `{}`",
            b.namespace
        );
    }
    // The shared envelope wrapper + the NMPD file identifier are present once.
    assert!(s.contains("private static func encodeDispatchEnvelope"));
    assert!(s.contains("fileId: \"NMPD\""));
}

#[test]
fn kotlin_emits_every_namespace_and_method() {
    let s = render(Platform::Kotlin);
    assert!(s.contains("object GeneratedActionBuilders"));
    for b in ACTION_BUILDERS {
        assert!(
            s.contains(&format!("fun {}(", b.method)),
            "missing kotlin builder method `{}`",
            b.method
        );
        assert!(
            s.contains(&format!("actionNamespace = {:?}", b.namespace)),
            "missing kotlin namespace stamp `{}`",
            b.namespace
        );
    }
    assert!(s.contains("private fun encodeDispatchEnvelope"));
    assert!(s.contains("fbb.finish(root, \"NMPD\")"));
}

#[test]
fn both_platforms_emit_the_publish_union_builders() {
    // `nmp.publish` is a UNION body, emitted by the dedicated `*_publish`
    // emitters (not the flat-table `ACTION_BUILDERS` loop). Assert the two
    // typed-field variants land in both platforms, each stamping the publish
    // namespace + the `NPUB` payload identifier.
    for (s, ns_pat, id_pat) in [
        (
            render(Platform::Swift),
            "actionNamespace: \"nmp.publish\"",
            "fileId: \"NPUB\"",
        ),
        (
            render(Platform::Kotlin),
            "actionNamespace = \"nmp.publish\"",
            "fbb.finish(payloadRoot, \"NPUB\")",
        ),
    ] {
        for builder in registry::PUBLISH_BUILDERS {
            assert!(
                s.contains(&format!("{}(", builder.method)),
                "missing publish builder method `{}`",
                builder.method
            );
        }
        assert!(s.contains(ns_pat), "missing publish namespace stamp");
        assert!(s.contains(id_pat), "missing NPUB payload identifier");
    }
}

#[test]
fn schema_version_constant_is_mirrored_in_both() {
    let v = registry::DISPATCH_ENVELOPE_SCHEMA_VERSION;
    assert!(
        render(Platform::Swift).contains(&format!("dispatchEnvelopeSchemaVersion: UInt32 = {v}"))
    );
    assert!(
        render(Platform::Kotlin).contains(&format!("DISPATCH_ENVELOPE_SCHEMA_VERSION: Int = {v}"))
    );
}

#[test]
fn platform_parse_roundtrips() {
    assert_eq!(Platform::parse("swift").unwrap(), Platform::Swift);
    assert_eq!(Platform::parse("kotlin").unwrap(), Platform::Kotlin);
    assert_eq!(Platform::parse("ts").unwrap(), Platform::Ts);
    assert!(Platform::parse("rust").is_err());
}

#[test]
fn optional_field_is_guarded_in_both() {
    // `nmp.nip25.react` has an optional `targetAuthorPubkey`; the emitters must
    // guard it (skip the field when absent) so the Rust decoder reads `None`.
    let swift = render(Platform::Swift);
    assert!(swift.contains("targetAuthorPubkey: String?"));
    assert!(swift.contains("if targetAuthorPubkeyOffset.o != 0"));
    let kotlin = render(Platform::Kotlin);
    assert!(kotlin.contains("targetAuthorPubkey: String?"));
    assert!(kotlin.contains("if (targetAuthorPubkeyOffset != 0)"));
    let ts = render(Platform::Ts);
    assert!(ts.contains("targetAuthorPubkey: string | null"));
    assert!(ts.contains("if (targetAuthorPubkeyOffset !== 0)"));
}

#[test]
fn optional_sbyte_fields_are_guarded_in_default_builders() {
    // `editGroupMetadata` exposes optional tri-state byte enums. Swift/Kotlin
    // must unwrap before calling FlatBuffers, and TS must keep the nullable API.
    let swift = render(Platform::Swift);
    assert!(swift.contains("visibility: Int8?"));
    assert!(swift.contains(
        "if let visibilityVal = visibility { fbb.add(element: visibilityVal, def: Int8(0), at: 14) } // slot 5: visibility"
    ));

    let kotlin = render(Platform::Kotlin);
    assert!(kotlin.contains("visibility: Byte?"));
    assert!(kotlin
        .contains("if (visibility != null) fbb.addByte(5, visibility, 0) // slot 5: visibility"));

    let ts = render(Platform::Ts);
    assert!(ts.contains("visibility: number | null"));
    assert!(ts.contains(
        "if (visibility !== null) fbb.addFieldInt8(5, visibility, 0); // slot 5: visibility"
    ));
}

#[test]
fn optional_scalar_fields_are_guarded_for_app_local_registries() {
    const FIELDS: &[PayloadField] = &[
        PayloadField {
            name: "count",
            kind: FieldKind::Uint,
            optional: true,
        },
        PayloadField {
            name: "flag",
            kind: FieldKind::Ubyte,
            optional: true,
        },
        PayloadField {
            name: "mode",
            kind: FieldKind::Sbyte,
            optional: true,
        },
    ];
    const BUILDERS: &[ActionBuilder] = &[ActionBuilder {
        namespace: "app.test.optional_scalars",
        method: "optionalScalars",
        fields: FIELDS,
        doc: "Exercise optional scalar emission.",
    }];
    const CONTRACTS: &[AppActionBuilderWireContract] = &[AppActionBuilderWireContract {
        namespace: "app.test.optional_scalars",
        contract: ActionBuilderWireContract {
            schema_version: 7,
            file_identifier: "TST1",
        },
    }];
    let registry = ActionBuilderRegistry::app_local(BUILDERS, CONTRACTS);

    let swift = render_from_registry(Platform::Swift, &registry);
    assert!(swift.contains("count: UInt32?"));
    assert!(swift.contains(
        "if let countVal = count { fbb.add(element: UInt32(countVal), def: UInt32(0), at: 6) } // slot 1: count"
    ));
    assert!(swift.contains(
        "if let flagVal = flag { fbb.add(element: flagVal, def: UInt8(0), at: 8) } // slot 2: flag"
    ));
    assert!(swift.contains(
        "if let modeVal = mode { fbb.add(element: modeVal, def: Int8(0), at: 10) } // slot 3: mode"
    ));

    let kotlin = render_from_registry(Platform::Kotlin, &registry);
    assert!(kotlin.contains("count: Int?"));
    assert!(kotlin.contains("flag: Byte?"));
    assert!(kotlin.contains("mode: Byte?"));
    assert!(kotlin.contains("if (count != null) fbb.addInt(1, count, 0) // slot 1: count"));
    assert!(kotlin.contains("if (flag != null) fbb.addByte(2, flag, 0) // slot 2: flag"));
    assert!(kotlin.contains("if (mode != null) fbb.addByte(3, mode, 0) // slot 3: mode"));

    let ts = render_from_registry(Platform::Ts, &registry);
    assert!(ts.contains("count: number | null"));
    assert!(ts.contains("flag: number | null"));
    assert!(ts.contains("mode: number | null"));
    assert!(ts.contains("if (count !== null) fbb.addFieldInt32(1, count, 0); // slot 1: count"));
    assert!(ts.contains("if (flag !== null) fbb.addFieldInt8(2, flag, 0); // slot 2: flag"));
    assert!(ts.contains("if (mode !== null) fbb.addFieldInt8(3, mode, 0); // slot 3: mode"));
}

#[test]
fn check_reports_missing_file_as_stale() {
    let tmp = std::env::temp_dir().join(format!(
        "nmp-action-builders-missing-{}.swift",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&tmp);
    let outcome = check_action_builders(Platform::Swift, &tmp).unwrap();
    assert!(!outcome.up_to_date);
    assert_eq!(outcome.first_diff_line, None);
}

#[test]
fn generate_then_check_is_up_to_date() {
    for platform in [Platform::Swift, Platform::Kotlin, Platform::Ts] {
        let tmp = std::env::temp_dir().join(format!(
            "nmp-action-builders-roundtrip-{}-{:?}.gen",
            std::process::id(),
            platform
        ));
        generate_action_builders(platform, &tmp).unwrap();
        let outcome = check_action_builders(platform, &tmp).unwrap();
        assert!(
            outcome.up_to_date,
            "fresh-generated file should be up to date"
        );
        let _ = std::fs::remove_file(&tmp);
    }
}
