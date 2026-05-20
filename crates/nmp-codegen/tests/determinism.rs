use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn generation_is_byte_deterministic() {
    let root = test_root("nmp-codegen-determinism");
    let manifest = root.join("nmp.toml");
    let one = root.join("one");
    let two = root.join("two");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        &manifest,
        r#"
        [app]
        name = "fixture"
        display_name = "Fixture"

        [modules]
        kernel = "nmp-core"
        protocol = []
        app = ["fixture-todo-core"]
        "#,
    )
    .unwrap();

    let report_one = nmp_codegen::generate_modules(&manifest, &one).unwrap();
    let report_two = nmp_codegen::generate_modules(&manifest, &two).unwrap();

    assert_eq!(report_one.files, report_two.files);
    for relative in report_one.files {
        assert_eq!(read(&one, &relative), read(&two, &relative), "{relative:?}");
    }
    fs::remove_dir_all(root).unwrap();
}

/// Pins the generated update-channel envelope wire contract from the codegen
/// side: the host `UpdateEnvelope` MUST be the tagged union T103 specifies
/// (`t`/`v`, snake_case, Update + Snapshot + Panic arms). A refactor of
/// `envelope_rs` that drifts from `nmp_core::UpdateEnvelope` would silently
/// break every host.
#[test]
fn generated_envelope_models_the_tagged_union() {
    let root = test_root("nmp-codegen-envelope");
    let manifest = root.join("nmp.toml");
    let out = root.join("out");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        &manifest,
        r#"
        [app]
        name = "fixture"
        display_name = "Fixture"

        [modules]
        kernel = "nmp-core"
        protocol = []
        app = ["fixture-todo-core"]
        "#,
    )
    .unwrap();

    nmp_codegen::generate_modules(&manifest, &out).unwrap();
    let envelope = fs::read_to_string(out.join("src/envelope.rs")).unwrap();

    assert!(
        envelope.contains(r#"#[serde(tag = "t", content = "v", rename_all = "snake_case")]"#),
        "generated envelope must use the canonical t/v snake_case tagging:\n{envelope}"
    );
    assert!(
        envelope.contains("Update(nmp_core::KernelUpdate)"),
        "generated envelope must carry the discrete kernel update:\n{envelope}"
    );
    assert!(
        envelope.contains("FullState(serde_json::Value)"),
        "generated envelope must carry the opaque full-state payload:\n{envelope}"
    );
    assert!(
        envelope.contains("ViewBatch(nmp_core::ViewBatchFrame)"),
        "generated envelope must carry the reserved view batch shape:\n{envelope}"
    );
    assert!(
        envelope.contains("SideEffect(nmp_core::SideEffectFrame)"),
        "generated envelope must carry the reserved side-effect shape:\n{envelope}"
    );
    assert!(
        envelope.contains("Panic(nmp_core::PanicFrame)"),
        "generated envelope must carry the D7 actor-death panic frame:\n{envelope}"
    );

    let lib = fs::read_to_string(out.join("src/lib.rs")).unwrap();
    assert!(
        lib.contains("pub mod envelope;") && lib.contains("pub use envelope::UpdateEnvelope;"),
        "envelope module must be wired into the generated crate:\n{lib}"
    );

    fs::remove_dir_all(root).unwrap();
}

fn read(root: &Path, relative: &Path) -> Vec<u8> {
    fs::read(root.join(relative)).unwrap()
}

fn test_root(name: &str) -> PathBuf {
    let mut root = std::env::temp_dir();
    root.push(format!("{name}-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).unwrap();
    }
    root
}
