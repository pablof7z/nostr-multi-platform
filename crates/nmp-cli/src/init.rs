//! `nmp init <app-name>` — scaffold a new, immediately-buildable NMP app.
//!
//! Layout produced at the target root:
//!
//! ```text
//! <root>/
//!   Cargo.toml                 # workspace: core + app-owned UniFFI facade
//!   nmp.toml                   # app manifest (NMP dependency policy; read by
//!                              # `nmp upgrade`)
//!   action-builders.json       # app-local typed action-builder contract
//!   generated/                 # Swift/Kotlin/TS action builders
//!   ci/check-uniffi-bindings.sh # app facade binding-generation check
//!   README.md                  # next steps
//!   crates/<name>-core/
//!     Cargo.toml               # depends on owner NMP crates + native runtime + nmp-core
//!     src/lib.rs               # explicit app composition root
//!     src/entry_action.rs      # app-owned typed action module
//!     src/entry_view.rs        # app-owned reactive read model
//!     schema/add_entry.fbs     # app-owned action payload schema
//!     examples/shell.rs        # NmpAppBuilder → app register → start
//!   crates/<name>-app/
//!     Cargo.toml               # app-owned UniFFI facade cdylib/staticlib/rlib
//!     src/lib.rs               # setup_scaffolding! + facade-local types
//! ```
//!
//! # ADR-0069 — explicit feature composition
//!
//! The scaffolded `<name>-core` crate is a **thin composition shell**: it
//! depends on explicit owner crates and installs each selected substrate,
//! protocol, and runtime layer directly before app-owned modules. The generated
//! `<name>-app` crate is an app-owned UniFFI facade over that composition; it
//! owns `NmpApp` by value and delegates runtime mechanics to
//! `nmp-uniffi-support` instead of hand-rolling a native doorway.
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
const FACADE_CARGO_TMPL: &str = include_str!("../templates/facade_cargo.toml.tmpl");
const LIB_TMPL: &str = include_str!("../templates/lib.rs.tmpl");
const ENTRY_ACTION_TMPL: &str = include_str!("../templates/entry_action.rs.tmpl");
const ENTRY_VIEW_TMPL: &str = include_str!("../templates/entry_view.rs.tmpl");
const ADD_ENTRY_SCHEMA_TMPL: &str = include_str!("../templates/add_entry.fbs.tmpl");
const ADD_ENTRY_GENERATED_TMPL: &str = include_str!("../templates/add_entry_generated.rs.tmpl");
const ACTION_BUILDERS_TMPL: &str = include_str!("../templates/action-builders.json.tmpl");
const FACADE_LIB_TMPL: &str = include_str!("../templates/facade_lib.rs.tmpl");
const CHECK_UNIFFI_BINDINGS_TMPL: &str = include_str!("../templates/check-uniffi-bindings.sh.tmpl");
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
    let facade_pkg = format!("{name}-app");
    let facade_crate_ident = facade_pkg.replace('-', "_");
    let facade_struct = format!("{}App", pascal_ident(&name));
    let nmp_dependency = nmp_dependency.unwrap_or(NmpDependency::Path(nmp_checkout_path()?));
    let nmp_core_dep = nmp_crate_dependency(&nmp_dependency, "nmp-core");
    let nmp_native_runtime_dep = nmp_crate_dependency(&nmp_dependency, "nmp-native-runtime");
    let nmp_uniffi_support_dep = nmp_crate_dependency(&nmp_dependency, "nmp-uniffi-support");
    let nmp_substrate_dep = nmp_crate_dependency(&nmp_dependency, "nmp-substrate");
    let nmp_nip50_dep = nmp_crate_dependency(&nmp_dependency, "nmp-nip50");
    let nmp_nip02_dep = nmp_crate_dependency(&nmp_dependency, "nmp-nip02");
    let nmp_replies_dep = nmp_crate_dependency(&nmp_dependency, "nmp-replies");
    let nmp_nip25_dep = nmp_crate_dependency(&nmp_dependency, "nmp-nip25");
    let nmp_nip18_dep = nmp_crate_dependency(&nmp_dependency, "nmp-nip18");
    let nmp_nip23_dep = nmp_crate_dependency(&nmp_dependency, "nmp-nip23");
    let nmp_nip84_dep = nmp_crate_dependency(&nmp_dependency, "nmp-nip84");
    let nmp_nip29_dep = nmp_crate_dependency(&nmp_dependency, "nmp-nip29");
    let nmp_wot_dep = nmp_crate_dependency(&nmp_dependency, "nmp-wot");
    let nmp_nip51_dep = nmp_crate_dependency(&nmp_dependency, "nmp-nip51");
    let nmp_nip17_dep = nmp_crate_dependency(&nmp_dependency, "nmp-nip17");
    let nmp_nip22_dep = nmp_crate_dependency(&nmp_dependency, "nmp-nip22");
    let nmp_content_dep = nmp_crate_dependency(&nmp_dependency, "nmp-content");
    let nmp_signer_iface_dep = nmp_crate_dependency(&nmp_dependency, "nmp-signer-iface");
    let nmp_codegen_manifest = nmp_codegen_manifest(&nmp_dependency)?
        .to_string_lossy()
        .to_string();
    let nmp_manifest = nmp_manifest_block(&nmp_dependency);

    let render = |tmpl: &str| -> String {
        tmpl.replace("{{name}}", &name)
            .replace("{{pkg}}", &pkg)
            .replace("{{facade_pkg}}", &facade_pkg)
            .replace("{{crate_ident}}", &crate_ident)
            .replace("{{facade_crate_ident}}", &facade_crate_ident)
            .replace("{{facade_struct}}", &facade_struct)
            .replace("{{display}}", &display)
            .replace("{{nmp_core_dep}}", &nmp_core_dep)
            .replace("{{nmp_native_runtime_dep}}", &nmp_native_runtime_dep)
            .replace("{{nmp_uniffi_support_dep}}", &nmp_uniffi_support_dep)
            .replace("{{nmp_substrate_dep}}", &nmp_substrate_dep)
            .replace("{{nmp_nip50_dep}}", &nmp_nip50_dep)
            .replace("{{nmp_nip02_dep}}", &nmp_nip02_dep)
            .replace("{{nmp_replies_dep}}", &nmp_replies_dep)
            .replace("{{nmp_nip25_dep}}", &nmp_nip25_dep)
            .replace("{{nmp_nip18_dep}}", &nmp_nip18_dep)
            .replace("{{nmp_nip23_dep}}", &nmp_nip23_dep)
            .replace("{{nmp_nip84_dep}}", &nmp_nip84_dep)
            .replace("{{nmp_nip29_dep}}", &nmp_nip29_dep)
            .replace("{{nmp_wot_dep}}", &nmp_wot_dep)
            .replace("{{nmp_nip51_dep}}", &nmp_nip51_dep)
            .replace("{{nmp_nip17_dep}}", &nmp_nip17_dep)
            .replace("{{nmp_nip22_dep}}", &nmp_nip22_dep)
            .replace("{{nmp_content_dep}}", &nmp_content_dep)
            .replace("{{nmp_signer_iface_dep}}", &nmp_signer_iface_dep)
            .replace("{{nmp_codegen_manifest}}", &nmp_codegen_manifest)
            .replace("{{nmp_manifest}}", &nmp_manifest)
    };

    let crate_dir = root.join("crates").join(&pkg);
    let facade_dir = root.join("crates").join(&facade_pkg);
    let registry_path = root.join("action-builders.json");
    write(&root.join("Cargo.toml"), &render(WORKSPACE_TMPL))?;
    write(&root.join("nmp.toml"), &render(NMP_TOML_TMPL))?;
    write(&registry_path, &render(ACTION_BUILDERS_TMPL))?;
    write(&root.join("README.md"), &render(README_TMPL))?;
    write(&crate_dir.join("Cargo.toml"), &render(APP_CARGO_TMPL))?;
    write(&crate_dir.join("src").join("lib.rs"), &render(LIB_TMPL))?;
    write(
        &crate_dir.join("src").join("entry_action.rs"),
        &render(ENTRY_ACTION_TMPL),
    )?;
    write(
        &crate_dir.join("src").join("entry_view.rs"),
        &render(ENTRY_VIEW_TMPL),
    )?;
    write(
        &crate_dir
            .join("src")
            .join("entry_action")
            .join("generated")
            .join("add_entry_generated.rs"),
        &render(ADD_ENTRY_GENERATED_TMPL),
    )?;
    write(
        &crate_dir.join("schema").join("add_entry.fbs"),
        &render(ADD_ENTRY_SCHEMA_TMPL),
    )?;
    write(
        &crate_dir.join("examples").join("shell.rs"),
        &render(SHELL_TMPL),
    )?;
    write(&facade_dir.join("Cargo.toml"), &render(FACADE_CARGO_TMPL))?;
    write(
        &facade_dir.join("src").join("lib.rs"),
        &render(FACADE_LIB_TMPL),
    )?;
    write(
        &root.join("ci").join("check-uniffi-bindings.sh"),
        &render(CHECK_UNIFFI_BINDINGS_TMPL),
    )?;
    generate_action_builders(&registry_path)?;

    println!("scaffolded `{name}` at {}", root.display());
    println!("next:");
    println!("  cd {}", root.display());
    println!("  cargo check                          # core + facade compile as-is");
    println!(
        "  cargo run --manifest-path {nmp_codegen_manifest} -p nmp-codegen -- gen action-builders --registry action-builders.json --check"
    );
    println!("  bash ci/check-uniffi-bindings.sh # generate Swift/Kotlin facade bindings");
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

fn pascal_ident(name: &str) -> String {
    name.split('-')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut chars = p.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<String>()
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

fn nmp_codegen_manifest(dependency: &NmpDependency) -> Result<PathBuf, String> {
    match dependency {
        NmpDependency::Path(path) => Ok(path.join("Cargo.toml")),
        NmpDependency::Version(_) => Ok(nmp_checkout_path()?.join("Cargo.toml")),
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

fn generate_action_builders(registry_path: &Path) -> Result<(), String> {
    let loaded = nmp_codegen::load_app_action_builder_registry(registry_path)?;
    nmp_codegen::validate_app_action_builder_schema_files(registry_path, &loaded)?;
    let registry = loaded.as_registry();
    for platform in [
        nmp_codegen::ActionBuilderPlatform::Swift,
        nmp_codegen::ActionBuilderPlatform::Kotlin,
        nmp_codegen::ActionBuilderPlatform::Ts,
    ] {
        let out_path = resolve_registry_path(registry_path, loaded.output_for(platform));
        nmp_codegen::generate_action_builders_from_registry(platform, &registry, &out_path)
            .map_err(|e| format!("generate {}: {e}", out_path.display()))?;
    }
    Ok(())
}

fn resolve_registry_path(registry_path: &Path, output: &Path) -> PathBuf {
    if output.is_absolute() {
        return output.to_path_buf();
    }
    registry_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(output)
}

fn write(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, content).map_err(|e| format!("{}: {e}", path.display()))
}
