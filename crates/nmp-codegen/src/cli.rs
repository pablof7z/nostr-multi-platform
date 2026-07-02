//! CLI argument-parsing helpers — one `run_gen_<subcommand>` function per
//! `nmp gen <subcommand>`. The dispatcher (`run()`) and the top-level
//! `help()` string live in `main.rs`; only the per-subcommand arg-parsing +
//! dispatch to `nmp_codegen::*` lives here.

use std::io::Read;
use std::path::PathBuf;

/// `nmp gen swift [--schemas <path>] --out <path> [--check]`.
///
/// `--schemas` defaults to `-` (stdin). The expected input is one or more
/// whitespace-separated schema documents from schema-owner dump binaries.
///
/// `--out` is required: the caller supplies the app-owned destination path.
/// For Chirp the path is
/// `apps/chirp/ios/Chirp/Bridge/Generated/KernelTypes.generated.swift`.
/// (Previously this was the hardcoded default; it is now explicit so no
/// app identity is baked into the generic tool — issue #1613.)
///
/// `--check` diffs against the file on disk and exits non-zero on drift.
/// The CI gate at `.github/workflows/codegen-drift.yml` uses this mode
/// and supplies `--out` explicitly.
pub fn run_gen_swift(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut schemas_path = PathBuf::from("-");
    let mut out: Option<PathBuf> = None;
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--schemas" => {
                index += 1;
                schemas_path = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--schemas requires a path or `-`".to_string())?;
            }
            "--out" => {
                index += 1;
                out = Some(
                    args.get(index)
                        .map(PathBuf::from)
                        .ok_or_else(|| "--out requires a path".to_string())?,
                );
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{help}")),
        }
        index += 1;
    }

    let out = out.ok_or_else(|| {
        "--out is required (e.g. --out ios/MyApp/Bridge/Generated/KernelTypes.generated.swift)"
            .to_string()
    })?;

    let json = read_schemas(&schemas_path)?;

    if check {
        let outcome = nmp_codegen::check_swift(&json, &out).map_err(|e| e.to_string())?;
        if outcome.up_to_date {
            println!("nmp gen swift --check: ok ({})", out.display());
            Ok(())
        } else {
            let where_diff = outcome
                .first_diff_line
                .map(|n| format!(" (first differing line {n})"))
                .unwrap_or_else(|| " (file missing)".to_string());
            Err(format!(
                "Swift codegen stale at {}{where_diff}.\n\
                 Regenerate with:\n  \
                 cargo run -p nmp-core --features codegen-schema --bin dump_projection_schemas \
                 | cargo run -p nmp-codegen -- gen swift --out {}",
                out.display(),
                out.display()
            ))
        }
    } else {
        nmp_codegen::generate_swift(&json, &out).map_err(|e| e.to_string())?;
        println!("wrote {}", out.display());
        Ok(())
    }
}

/// `nmp gen typed-decoders --out <path> [--check]`.
///
/// Generates `TypedProjectionDecoders.generated.swift` — the per-projection
/// typed-FlatBuffer-sidecar decoders (consumer side). Driven entirely by the
/// registry's `typed_sidecar` metadata in
/// `crates/nmp-codegen/src/swift_projections_registry.rs`; takes no schema
/// stdin.
///
/// `--out` is required: the caller supplies the app-owned destination path.
/// For Chirp the path is
/// `apps/chirp/ios/Chirp/Bridge/Generated/TypedProjectionDecoders.generated.swift`.
/// (Previously this was the hardcoded default; it is now explicit so no
/// app identity is baked into the generic tool — issue #1613.)
///
/// `--check` diffs against the file on disk and exits non-zero on drift. The
/// CI gate at `.github/workflows/codegen-drift.yml` uses this mode and
/// supplies `--out` explicitly.
pub fn run_gen_typed_decoders(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut out: Option<PathBuf> = None;
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                out = Some(
                    args.get(index)
                        .map(PathBuf::from)
                        .ok_or_else(|| "--out requires a path".to_string())?,
                );
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{help}")),
        }
        index += 1;
    }

    let out = out.ok_or_else(|| {
        "--out is required (e.g. --out ios/MyApp/Bridge/Generated/TypedProjectionDecoders.generated.swift)"
            .to_string()
    })?;

    if check {
        let outcome = nmp_codegen::check_typed_decoders(&out).map_err(|e| e.to_string())?;
        if outcome.up_to_date {
            println!("nmp gen typed-decoders --check: ok ({})", out.display());
            Ok(())
        } else {
            let where_diff = outcome
                .first_diff_line
                .map(|n| format!(" (first differing line {n})"))
                .unwrap_or_else(|| " (file missing)".to_string());
            Err(format!(
                "typed-decoder codegen stale at {}{where_diff}.\n\
                 Regenerate with:\n  \
                 cargo run -p nmp-codegen -- gen typed-decoders --out {}",
                out.display(),
                out.display()
            ))
        }
    } else {
        nmp_codegen::generate_typed_decoders(&out).map_err(|e| e.to_string())?;
        println!("wrote {}", out.display());
        Ok(())
    }
}

/// `nmp gen projection-cache --platform swift|kotlin --out <path> [--check]`.
///
/// Generates the NMP-owned rev-aware projection cache implementing the
/// ADR-0055 D3-3 merge algorithm. Driven by the same registry as
/// `typed-decoders`; takes no schema stdin.
///
/// `--platform swift` (default): generates `ProjectionCache.generated.swift`.
/// For Chirp: `apps/chirp/ios/Chirp/Bridge/Generated/ProjectionCache.generated.swift`.
///
/// `--platform kotlin`: generates `ProjectionCache.kt`.
/// For Chirp Android: `apps/chirp/android/app/src/main/java/org/nmp/android/ProjectionCache.kt`.
///
/// `--out` is required: the caller supplies the app-owned destination path.
/// (Previously per-platform paths were hardcoded as defaults; they are now
/// explicit so no app identity is baked into the generic tool — issue #1613.)
///
/// `--check` diffs against the file on disk and exits non-zero on drift. The
/// CI gate at `.github/workflows/codegen-drift.yml` uses this mode and
/// supplies `--out` explicitly.
pub fn run_gen_projection_cache(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut platform = "swift".to_string();
    let mut check = false;
    let mut out: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--platform" => {
                index += 1;
                platform = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| "--platform requires swift|kotlin".to_string())?;
            }
            "--out" => {
                index += 1;
                out = Some(
                    args.get(index)
                        .map(PathBuf::from)
                        .ok_or_else(|| "--out requires a path".to_string())?,
                );
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{help}")),
        }
        index += 1;
    }

    let out = out.ok_or_else(|| {
        format!(
            "--out is required (e.g. --out <app-path>/ProjectionCache.generated.swift \
             for --platform swift, or --out <app-path>/ProjectionCache.kt for --platform kotlin)"
        )
    })?;

    match platform.as_str() {
        "swift" => {
            if check {
                let outcome =
                    nmp_codegen::check_projection_cache(&out).map_err(|e| e.to_string())?;
                if outcome.up_to_date {
                    println!("nmp gen projection-cache --check: ok ({})", out.display());
                    Ok(())
                } else {
                    let where_diff = outcome
                        .first_diff_line
                        .map(|n| format!(" (first differing line {n})"))
                        .unwrap_or_else(|| " (file missing)".to_string());
                    Err(format!(
                        "projection-cache (swift) codegen stale at {}{where_diff}.\n\
                         Regenerate with:\n  \
                         cargo run -p nmp-codegen -- gen projection-cache --out {}",
                        out.display(),
                        out.display()
                    ))
                }
            } else {
                nmp_codegen::generate_projection_cache(&out).map_err(|e| e.to_string())?;
                println!("wrote {}", out.display());
                Ok(())
            }
        }
        "kotlin" => {
            if check {
                let outcome =
                    nmp_codegen::check_kotlin_projection_cache(&out).map_err(|e| e.to_string())?;
                if outcome.up_to_date {
                    println!(
                        "nmp gen projection-cache --platform kotlin --check: ok ({})",
                        out.display()
                    );
                    Ok(())
                } else {
                    let where_diff = outcome
                        .first_diff_line
                        .map(|n| format!(" (first differing line {n})"))
                        .unwrap_or_else(|| " (file missing)".to_string());
                    Err(format!(
                        "projection-cache (kotlin) codegen stale at {}{where_diff}.\n\
                         Regenerate with:\n  \
                         cargo run -p nmp-codegen -- gen projection-cache --platform kotlin \
                         --out {}",
                        out.display(),
                        out.display()
                    ))
                }
            } else {
                nmp_codegen::generate_kotlin_projection_cache(&out).map_err(|e| e.to_string())?;
                println!("wrote {}", out.display());
                Ok(())
            }
        }
        other => Err(format!(
            "unknown --platform {:?}: expected swift or kotlin\n{help}",
            other,
        )),
    }
}

/// `nmp gen signer-catalog [--catalog - | <path>] [--check]`.
///
/// `--catalog` defaults to `-` (stdin). The expected input is whatever
/// `dump_signer_catalog` writes (`cargo run -p nmp-core --bin
/// dump_signer_catalog`) — a top-level JSON array of known signer apps.
///
/// Without `--check`, writes the generated native lists (three Kotlin
/// `KnownSigners.generated.kt` copies + two Swift `KnownSigners.generated.swift`
/// copies). With `--check`, diffs each against a fresh render AND asserts the
/// `AndroidManifest <queries>` / `Info.plist LSApplicationQueriesSchemes`
/// schemes match the catalog, exiting non-zero on any drift. The CI gate at
/// `.github/workflows/codegen-drift.yml` uses `--check`.
pub fn run_gen_signer_catalog(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut catalog_path = PathBuf::from("-");
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--catalog" => {
                index += 1;
                catalog_path = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--catalog requires a path or `-`".to_string())?;
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{help}")),
        }
        index += 1;
    }

    let json = read_schemas(&catalog_path)?;

    if check {
        let outcome = nmp_codegen::check_signer_catalog(&json)?;
        if outcome.up_to_date {
            println!("nmp gen signer-catalog --check: ok");
            Ok(())
        } else {
            Err(format!(
                "signer-catalog codegen stale / drifted:\n  {}\n\
                 Regenerate with:\n  \
                 cargo run -q -p nmp-core --bin dump_signer_catalog \
                 | cargo run -q -p nmp-codegen -- gen signer-catalog\n\
                 (manifest/plist scheme mismatches must be fixed by hand to match the catalog)",
                outcome.problems.join("\n  ")
            ))
        }
    } else {
        let written = nmp_codegen::generate_signer_catalog(&json)?;
        for path in written {
            println!("wrote {}", path.display());
        }
        Ok(())
    }
}

/// Read the schema JSON from `path` (or stdin if `path == "-"`).
pub fn read_schemas(path: &std::path::Path) -> Result<String, String> {
    if path == std::path::Path::new("-") {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| format!("reading stdin: {e}"))?;
        if s.trim().is_empty() {
            return Err(
                "no schema input on stdin. Pipe `dump_projection_schemas` output, or pass \
                 --schemas <path>."
                    .to_string(),
            );
        }
        Ok(s)
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))
    }
}
