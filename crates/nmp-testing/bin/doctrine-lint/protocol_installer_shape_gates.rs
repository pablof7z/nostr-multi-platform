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
        let surface = crate_root_installer_surface(&lib);

        if surface.register_exposures != 1 {
            violations.push(format!(
                "{krate}: expected exactly one crate-root public canonical `register`, found {}",
                surface.register_exposures
            ));
        }
        if !surface.config_exposed {
            violations.push(format!("{krate}: missing crate-root public `Config`"));
        }
        if !surface.handles_exposed {
            violations.push(format!("{krate}: missing crate-root public `Handles`"));
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
                if starts_public_split_installer(trimmed)
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

#[test]
fn crate_root_surface_parser_counts_public_exports_only() {
    let direct = r#"
pub struct Config {}
pub struct Handles {}
pub fn register(
"#;
    let surface = crate_root_installer_surface(direct);
    assert_eq!(surface.register_exposures, 1);
    assert!(surface.config_exposed);
    assert!(surface.handles_exposed);

    let reexported = r#"
mod installer;
pub use installer::{
    register,
    Config,
    Handles,
};
pub use register::wallet_typed_projection;
"#;
    let surface = crate_root_installer_surface(reexported);
    assert_eq!(surface.register_exposures, 1);
    assert!(surface.config_exposed);
    assert!(surface.handles_exposed);
}

#[derive(Default)]
struct InstallerSurface {
    register_exposures: usize,
    config_exposed: bool,
    handles_exposed: bool,
}

fn crate_root_installer_surface(lib: &str) -> InstallerSurface {
    let public_uses = public_use_statements(lib);
    InstallerSurface {
        register_exposures: count_crate_root_register_fns(lib)
            + public_uses
                .iter()
                .filter(|stmt| public_use_exports(stmt, "register"))
                .count(),
        config_exposed: crate_root_declares_pub_struct(lib, "Config")
            || public_uses
                .iter()
                .any(|stmt| public_use_exports(stmt, "Config")),
        handles_exposed: crate_root_declares_pub_struct(lib, "Handles")
            || public_uses
                .iter()
                .any(|stmt| public_use_exports(stmt, "Handles")),
    }
}

fn count_crate_root_register_fns(lib: &str) -> usize {
    lib.lines()
        .filter(|line| line.trim_start().starts_with("pub fn register("))
        .count()
}

fn crate_root_declares_pub_struct(lib: &str, name: &str) -> bool {
    let prefix = format!("pub struct {name}");
    lib.lines()
        .any(|line| line.trim_start().starts_with(&prefix))
}

fn public_use_statements(lib: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut collecting = false;

    for line in lib.lines() {
        let trimmed = line.trim();
        if !collecting && !trimmed.starts_with("pub use ") {
            continue;
        }

        collecting = true;
        current.push_str(trimmed);
        current.push(' ');

        if trimmed.ends_with(';') {
            statements.push(std::mem::take(&mut current));
            collecting = false;
        }
    }

    statements
}

fn public_use_exports(statement: &str, name: &str) -> bool {
    let exported = if let Some(start) = statement.find('{') {
        let end = statement[start + 1..]
            .find('}')
            .map(|offset| start + 1 + offset)
            .unwrap_or(statement.len());
        &statement[start + 1..end]
    } else if let Some((_, exported)) = statement.rsplit_once("::") {
        exported
    } else {
        statement
    };
    exported
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .any(|token| token == name)
}

fn starts_public_split_installer(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix("pub ") else {
        return false;
    };
    let rest = rest
        .strip_prefix("async ")
        .or_else(|| rest.strip_prefix("const "))
        .or_else(|| rest.strip_prefix("unsafe "))
        .unwrap_or(rest);
    rest.starts_with("fn register_")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
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
