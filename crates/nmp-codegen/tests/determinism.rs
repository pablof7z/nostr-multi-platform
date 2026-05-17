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
