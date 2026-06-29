//! `nmp init <app-name>` — scaffold a new, immediately-buildable NMP app.
//!
//! Layout produced at the target root:
//!
//! ```text
//! <root>/
//!   Cargo.toml                 # workspace: members = ["crates/<name>-core"]
//!   nmp.toml                   # app manifest (NMP dependency policy; read by
//!                              # `nmp doctor` / `nmp upgrade`)
//!   README.md                  # next steps
//!   crates/<name>-core/
//!     Cargo.toml               # depends on nmp-defaults + nmp-native-runtime + nmp-core
//!     src/lib.rs               # explicit app composition root
//!     examples/shell.rs        # NmpAppBuilder → app register → start
//! ```
//!
//! # ADR-0069 — explicit feature composition
//!
//! The scaffolded `<name>-core` crate is a **thin composition shell**: it
//! depends on the reusable installer library [`nmp-defaults`] and installs the
//! substrate floor explicitly before app-owned modules. It does NOT generate an
//! FFI crate or hide production policy behind a preset. The app Rust
//! composition root is the readable source of truth for installed substrate,
//! protocol features, app features, capability contracts, and defaults.
//!
//! # Dependency policy
//!
//! * Default / `--nmp-path DIR` — local path dependencies on the NMP checkout,
//!   so the scaffold `cargo check`s against the in-tree crates.
//! * `--nmp-version VERSION` — git-rev pins on
//!   `github.com/pablof7z/nostr-multi-platform` at tag `vVERSION`, matching the
//!   external-consumer contract (consumers pin NMP by git rev; see
//!   `docs/architecture/external-consumers.md`).

use std::fs;
use std::path::{Path, PathBuf};

const WORKSPACE_TMPL: &str = include_str!("../templates/workspace_cargo.toml.tmpl");
const APP_CARGO_TMPL: &str = include_str!("../templates/app_cargo.toml.tmpl");
const LIB_TMPL: &str = include_str!("../templates/lib.rs.tmpl");
const NMP_TOML_TMPL: &str = include_str!("../templates/nmp.toml.tmpl");
const SHELL_TMPL: &str = include_str!("../templates/shell.rs.tmpl");
const README_TMPL: &str = include_str!("../templates/README.md.tmpl");

/// The canonical upstream git remote for git-rev pins (`--nmp-version`).
/// Matches the external-consumer contract in
/// `docs/architecture/external-consumers.md`.
const NMP_GIT_REMOTE: &str = "https://github.com/pablof7z/nostr-multi-platform";

pub fn run(args: &[String]) -> Result<(), String> {
    let mut name: Option<String> = None;
    let mut path: Option<PathBuf> = None;
    let mut nmp_dependency = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--path" => {
                index += 1;
                path = Some(
                    args.get(index)
                        .map(PathBuf::from)
                        .ok_or_else(|| "--path requires a directory".to_string())?,
                );
            }
            "--nmp-version" => {
                index += 1;
                let version = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--nmp-version requires a semver version".to_string())?;
                if nmp_dependency
                    .replace(NmpDependency::Version(version))
                    .is_some()
                {
                    return Err("pass only one of --nmp-version or --nmp-path".to_string());
                }
            }
            "--nmp-path" => {
                index += 1;
                let raw = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--nmp-path requires a directory".to_string())?;
                let path = fs::canonicalize(&raw)
                    .map_err(|e| format!("cannot resolve --nmp-path {}: {e}", raw.display()))?;
                if nmp_dependency.replace(NmpDependency::Path(path)).is_some() {
                    return Err("pass only one of --nmp-version or --nmp-path".to_string());
                }
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown argument {flag}"));
            }
            positional => {
                if name.is_some() {
                    return Err("unexpected extra argument".to_string());
                }
                name = Some(positional.to_string());
            }
        }
        index += 1;
    }

    let name = name.ok_or_else(|| "usage: nmp init <app-name> [--path DIR]".to_string())?;
    validate_name(&name)?;

    let root = path.unwrap_or_else(|| PathBuf::from(&name));
    if root.exists()
        && fs::read_dir(&root)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
    {
        return Err(format!("target {} exists and is not empty", root.display()));
    }

    let pkg = format!("{name}-core");
    let crate_ident = pkg.replace('-', "_");
    let display = title_case(&name);
    let nmp_dependency = nmp_dependency.unwrap_or(NmpDependency::Path(nmp_checkout_path()?));
    let nmp_core_dep = nmp_crate_dependency(&nmp_dependency, "nmp-core");
    let nmp_defaults_dep = nmp_crate_dependency(&nmp_dependency, "nmp-defaults");
    let nmp_native_runtime_dep = nmp_crate_dependency(&nmp_dependency, "nmp-native-runtime");
    let nmp_manifest = nmp_manifest_block(&nmp_dependency);

    let render = |tmpl: &str| -> String {
        tmpl.replace("{{name}}", &name)
            .replace("{{pkg}}", &pkg)
            .replace("{{crate_ident}}", &crate_ident)
            .replace("{{display}}", &display)
            .replace("{{nmp_core_dep}}", &nmp_core_dep)
            .replace("{{nmp_defaults_dep}}", &nmp_defaults_dep)
            .replace("{{nmp_native_runtime_dep}}", &nmp_native_runtime_dep)
            .replace("{{nmp_manifest}}", &nmp_manifest)
    };

    let crate_dir = root.join("crates").join(&pkg);
    write(&root.join("Cargo.toml"), &render(WORKSPACE_TMPL))?;
    write(&root.join("nmp.toml"), &render(NMP_TOML_TMPL))?;
    write(&root.join("README.md"), &render(README_TMPL))?;
    write(&crate_dir.join("Cargo.toml"), &render(APP_CARGO_TMPL))?;
    write(&crate_dir.join("src").join("lib.rs"), &render(LIB_TMPL))?;
    write(
        &crate_dir.join("examples").join("shell.rs"),
        &render(SHELL_TMPL),
    )?;

    println!("scaffolded `{name}` at {}", root.display());
    println!("next:");
    println!("  cd {}", root.display());
    println!("  cargo check                          # the {pkg} shell compiles as-is");
    println!("  cargo run --example shell -p {pkg}   # build → app register → start");
    Ok(())
}

enum NmpDependency {
    Path(PathBuf),
    Version(String),
}

fn validate_name(name: &str) -> Result<(), String> {
    let invalid = name.is_empty()
        || !name.starts_with(|c: char| c.is_ascii_lowercase())
        || !name.ends_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        || name.contains("--")
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if invalid {
        return Err(format!(
            "invalid app name `{name}`: use lowercase letters, digits and single hyphens \
             (e.g. `my-app`), starting with a letter"
        ));
    }
    Ok(())
}

fn title_case(name: &str) -> String {
    name.split('-')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut chars = p.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Absolute path to this checkout, derived from the nmp-cli crate location.
fn nmp_checkout_path() -> Result<PathBuf, String> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidate = here
        .parent()
        .ok_or_else(|| "cannot locate crates/ directory".to_string())?
        .parent()
        .ok_or_else(|| "cannot locate nmp checkout".to_string())?;
    fs::canonicalize(candidate).map_err(|e| {
        format!(
            "cannot resolve nmp checkout at {}: {e}",
            candidate.display()
        )
    })
}

/// Render the `Cargo.toml` dependency spec for an in-tree NMP crate
/// (`crates/<crate>`), honoring the chosen dependency policy.
///
/// * `Version` → git-rev pin on the upstream remote at tag `v<version>`
///   (the external-consumer contract: consumers pin NMP by git rev).
/// * `Path` → local path dependency into the NMP checkout.
fn nmp_crate_dependency(dependency: &NmpDependency, krate: &str) -> String {
    match dependency {
        NmpDependency::Version(version) => {
            format!("{{ git = \"{NMP_GIT_REMOTE}\", tag = \"v{version}\", package = \"{krate}\" }}")
        }
        NmpDependency::Path(path) => format!(
            "{{ path = \"{}\" }}",
            path.join("crates").join(krate).to_string_lossy()
        ),
    }
}

fn nmp_manifest_block(dependency: &NmpDependency) -> String {
    match dependency {
        NmpDependency::Version(version) => {
            format!("[nmp]\ndependency_mode = \"version\"\nversion = \"{version}\"\n")
        }
        NmpDependency::Path(path) => format!(
            "[nmp]\ndependency_mode = \"path\"\npath = \"{}\"\n",
            path.to_string_lossy()
        ),
    }
}

fn write(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, content).map_err(|e| format!("{}: {e}", path.display()))
}
