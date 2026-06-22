//! ADR-0064 §3 (#1783) — action-builder emitter tests.
//!
//! These cover the EMITTER (determinism, registry coverage, structural shape).
//! The authoritative WIRE round-trip — that a builder-shaped `DispatchEnvelope`
//! decodes via S2 (`decode_dispatch_envelope`) and routes via S3 (per-crate
//! `ActionPayload::decode`) to the right namespace/payload — lives in
//! `crates/nmp-codegen/tests/action_builder_wire_roundtrip.rs`, which CAN depend
//! on `nmp-core` + the nip crates (this library crate deliberately does not).

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
fn schema_version_constant_is_mirrored_in_both() {
    let v = registry::DISPATCH_ENVELOPE_SCHEMA_VERSION;
    assert!(render(Platform::Swift)
        .contains(&format!("dispatchEnvelopeSchemaVersion: UInt32 = {v}")));
    assert!(render(Platform::Kotlin)
        .contains(&format!("DISPATCH_ENVELOPE_SCHEMA_VERSION: Int = {v}")));
}

#[test]
fn platform_parse_roundtrips() {
    assert_eq!(Platform::parse("swift").unwrap(), Platform::Swift);
    assert_eq!(Platform::parse("kotlin").unwrap(), Platform::Kotlin);
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
    for platform in [Platform::Swift, Platform::Kotlin] {
        let tmp = std::env::temp_dir().join(format!(
            "nmp-action-builders-roundtrip-{}-{:?}.gen",
            std::process::id(),
            platform
        ));
        generate_action_builders(platform, &tmp).unwrap();
        let outcome = check_action_builders(platform, &tmp).unwrap();
        assert!(outcome.up_to_date, "fresh-generated file should be up to date");
        let _ = std::fs::remove_file(&tmp);
    }
}
