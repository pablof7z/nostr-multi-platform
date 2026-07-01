//! Gallery composition source ratchets.
//!
//! `nmp-app-gallery` is a production app composition root, so it must show the
//! named ADR-0069 substrate/protocol installers directly instead of reintroducing
//! the hidden defaults-era preset.

use std::path::{Path, PathBuf};

use super::workspace_root;

const BANNED_DEFAULTS_TOKENS: &[&str] = &[
    "register_defaults_with_handles",
    "register_defaults_with",
    "register_defaults",
];

#[test]
fn gallery_app_crate_does_not_use_hidden_defaults_preset() {
    let root = workspace_root();
    let gallery_crate = root.join("apps/nmp-gallery/crates/nmp-app-gallery");
    let mut files = Vec::new();
    collect_rs_files(&gallery_crate.join("src"), &mut files)
        .expect("gallery Rust sources must be walkable");
    files.push(gallery_crate.join("Cargo.toml"));
    files.sort();

    let mut violations = Vec::new();
    for path in files {
        scan_file_for_hidden_defaults(&root, &path, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "nmp-app-gallery must compose explicit named installers and must not \
         present or call hidden defaults preset entry points:\n{}",
        violations.join("\n")
    );
}

#[test]
fn gallery_composition_root_claims_exactly_once() {
    let root = workspace_root();
    let lib = root.join("apps/nmp-gallery/crates/nmp-app-gallery/src/lib.rs");
    let body = std::fs::read_to_string(&lib)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", lib.display()));

    for token in [
        "GALLERY_COMPOSITION_ROOT",
        "claim_composition_root",
        "install_gallery_composition(app)",
    ] {
        assert!(
            body.contains(token),
            "Gallery composition root must keep the one-shot runtime claim and \
             explicit owner installer body; missing `{token}`"
        );
    }
}

fn scan_file_for_hidden_defaults(root: &PathBuf, path: &PathBuf, violations: &mut Vec<String>) {
    let body = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    for (idx, line) in body.lines().enumerate() {
        for token in BANNED_DEFAULTS_TOKENS {
            if line.contains(token) {
                violations.push(format!(
                    "{}:{} banned `{token}`",
                    path.strip_prefix(root).unwrap_or(path).display(),
                    idx + 1
                ));
            }
        }
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_rs_files(&path, out)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}
