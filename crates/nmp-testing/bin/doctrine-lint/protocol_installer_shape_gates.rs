//! Protocol installer shape ratchet (#2724).
//!
//! Reusable protocol crates expose one public composition entry point:
//! `Config`, `Handles`, and `register(app, config) -> Result<Handles, _>`.
//! Split public helpers such as `register_actions` / `register_runtime` make
//! half-installed protocols possible, so this gate keeps them out of the public
//! crate surface.

use std::path::{Path, PathBuf};

use super::workspace_root;

const CANONICAL_INSTALLER_CRATES: &[&str] = &[
    "nmp-blossom",
    "nmp-nip02",
    "nmp-nip09",
    "nmp-nip11",
    "nmp-nip17",
    "nmp-nip18",
    "nmp-nip22",
    "nmp-nip23",
    "nmp-nip25",
    "nmp-nip29",
    "nmp-nip47",
    "nmp-nip50",
    "nmp-nip51",
    "nmp-nip57",
    "nmp-nip84",
    "nmp-replies",
    "nmp-wot",
];

#[test]
fn protocol_crates_expose_canonical_register_config_handles() {
    let root = workspace_root();
    let mut violations = Vec::new();

    for krate in CANONICAL_INSTALLER_CRATES {
        let src_root = root.join("crates").join(krate).join("src");
        let lib = read(&src_root.join("lib.rs"));
        let all_sources = read_all_rs(&src_root);

        let exposes_register = lib.contains("pub fn register(")
            || lib.contains("pub use installer::{register")
            || lib.contains("pub use register::{register");
        if !exposes_register {
            violations.push(format!("{krate}: missing public canonical `register`"));
        }
        if !all_sources.contains("pub struct Config") {
            violations.push(format!("{krate}: missing public `Config`"));
        }
        if !all_sources.contains("pub struct Handles") {
            violations.push(format!("{krate}: missing public `Handles`"));
        }
    }

    assert!(
        violations.is_empty(),
        "protocol crates must expose Config, Handles, and register:\n{}",
        violations.join("\n")
    );
}

#[test]
fn protocol_crates_do_not_expose_split_public_installers() {
    let root = workspace_root();
    let mut violations = Vec::new();

    for krate in CANONICAL_INSTALLER_CRATES {
        let src_root = root.join("crates").join(krate).join("src");
        for file in collect_rs_files(&src_root) {
            let body = read(&file);
            let rel = file.strip_prefix(&root).unwrap_or(&file).display();
            if body.contains("ProtocolDescriptor") || body.contains("ProtocolInstaller") {
                violations.push(format!("{rel}: descriptor-style protocol installer"));
            }
            for (idx, line) in body.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("pub(crate)") {
                    continue;
                }
                if trimmed.starts_with("pub fn register_")
                    && !trimmed.starts_with("pub fn register_content_embed_projection_adapter")
                {
                    violations.push(format!(
                        "{rel}:{} public split installer `{}`",
                        idx + 1,
                        trimmed
                    ));
                }
                if trimmed.starts_with("pub use ")
                    && (trimmed.contains("register_actions")
                        || trimmed.contains("register_runtime")
                        || trimmed.contains("register_") && !trimmed.contains("register,"))
                {
                    violations.push(format!(
                        "{rel}:{} public split installer re-export `{}`",
                        idx + 1,
                        trimmed
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "protocol crates must not expose split public installer helpers:\n{}",
        violations.join("\n")
    );
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn read_all_rs(root: &Path) -> String {
    collect_rs_files(root)
        .into_iter()
        .map(|path| read(&path))
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_rs_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rs_files_inner(root, &mut files);
    files
}

fn collect_rs_files_inner(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files_inner(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}
