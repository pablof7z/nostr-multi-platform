//! `nmp init <app-name>` — scaffold a new, immediately-buildable NMP app.
//!
//! Layout produced at the target root:
//!
//! ```text
//! <root>/
//!   Cargo.toml                 # workspace: members = ["crates/<name>-core"]
//!   nmp.toml                   # app manifest consumed by `nmp gen modules`
//!   README.md                  # next steps
//!   crates/<name>-core/
//!     Cargo.toml               # depends on nmp-core (absolute path) + serde
//!     src/lib.rs               # one Domain/View/Action module + descriptors
//!     examples/shell.rs        # minimal headless shell stub
//! ```
//!
//! The `<name>-core` crate compiles standalone (`cargo check`) because its
//! `nmp-core` path dependency is resolved to the absolute location of this
//! checkout at init time. `nmp gen modules` then emits the per-app FFI crate
//! under `apps/<name>/` (see `docs/cli.md` for why that crate expects a
//! monorepo layout).

use std::fs;
use std::path::{Path, PathBuf};

const WORKSPACE_TMPL: &str = include_str!("../templates/workspace_cargo.toml.tmpl");
const APP_CARGO_TMPL: &str = include_str!("../templates/app_cargo.toml.tmpl");
const LIB_TMPL: &str = include_str!("../templates/lib.rs.tmpl");
const NMP_TOML_TMPL: &str = include_str!("../templates/nmp.toml.tmpl");
const SHELL_TMPL: &str = include_str!("../templates/shell.rs.tmpl");
const README_TMPL: &str = include_str!("../templates/README.md.tmpl");

pub fn run(args: &[String]) -> Result<(), String> {
    let mut name: Option<String> = None;
    let mut path: Option<PathBuf> = None;
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
    let nmp_core = nmp_core_path()?;

    let render = |tmpl: &str| -> String {
        tmpl.replace("{{name}}", &name)
            .replace("{{pkg}}", &pkg)
            .replace("{{crate_ident}}", &crate_ident)
            .replace("{{display}}", &display)
            .replace("{{nmp_core}}", &nmp_core)
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
    println!("  cargo check                 # the {pkg} skeleton compiles as-is");
    println!("  nmp gen modules             # emit apps/{name}/nmp-app-{name}");
    Ok(())
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

/// Absolute path to this checkout's `crates/nmp-core`, derived from the
/// nmp-cli crate location so scaffolded apps build from any directory.
fn nmp_core_path() -> Result<String, String> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidate = here
        .parent()
        .ok_or_else(|| "cannot locate crates/ directory".to_string())?
        .join("nmp-core");
    let resolved = fs::canonicalize(&candidate)
        .map_err(|e| format!("cannot resolve nmp-core at {}: {e}", candidate.display()))?;
    Ok(resolved.to_string_lossy().into_owned())
}

fn write(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, content).map_err(|e| format!("{}: {e}", path.display()))
}
