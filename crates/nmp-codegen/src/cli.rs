//! CLI argument-parsing helpers — one `run_gen_<subcommand>` function per
//! `nmp gen <subcommand>`. The dispatcher (`run()`) and the top-level
//! `help()` string live in `main.rs`; only the per-subcommand arg-parsing +
//! dispatch to `nmp_codegen::*` lives here.

use std::path::{Path, PathBuf};

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
/// ADR-0070 D3-3 merge algorithm. Driven by the same registry as
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

/// `nmp gen keyed-ref-cache --platform swift|kotlin --out <path> [--check]`.
///
/// ADR-0070 Lane A (#1671) — generates the per-key (row-keyed) reference cache
/// (`KeyedRefCache`) for keyed projections (`refs.profile` / `refs.event`).
/// Driven by `KEYED_PROJECTIONS`; takes no schema stdin. Mirrors
/// `gen projection-cache`: `--out` required, `--check` diffs on disk.
pub fn run_gen_keyed_ref_cache(args: Vec<String>, help: &str) -> Result<(), String> {
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
    let out =
        out.ok_or_else(|| "--out is required (the app-owned destination path)".to_string())?;

    match platform.as_str() {
        "swift" => {
            if check {
                let outcome =
                    nmp_codegen::check_keyed_ref_cache(&out).map_err(|e| e.to_string())?;
                if outcome.up_to_date {
                    println!("nmp gen keyed-ref-cache --check: ok ({})", out.display());
                    Ok(())
                } else {
                    Err(stale_message(
                        "keyed-ref-cache (swift)",
                        &out,
                        outcome.first_diff_line,
                    ))
                }
            } else {
                nmp_codegen::generate_keyed_ref_cache(&out).map_err(|e| e.to_string())?;
                println!("wrote {}", out.display());
                Ok(())
            }
        }
        "kotlin" => {
            if check {
                let outcome =
                    nmp_codegen::check_kotlin_keyed_ref_cache(&out).map_err(|e| e.to_string())?;
                if outcome.up_to_date {
                    println!(
                        "nmp gen keyed-ref-cache --platform kotlin --check: ok ({})",
                        out.display()
                    );
                    Ok(())
                } else {
                    Err(stale_message(
                        "keyed-ref-cache (kotlin)",
                        &out,
                        outcome.first_diff_line,
                    ))
                }
            } else {
                nmp_codegen::generate_kotlin_keyed_ref_cache(&out).map_err(|e| e.to_string())?;
                println!("wrote {}", out.display());
                Ok(())
            }
        }
        other => Err(format!(
            "unknown --platform {other:?}: expected swift or kotlin\n{help}"
        )),
    }
}

/// Shared stale-codegen `--check` error message.
pub fn stale_message(what: &str, out: &Path, first_diff_line: Option<usize>) -> String {
    let where_diff = first_diff_line
        .map(|n| format!(" (first differing line {n})"))
        .unwrap_or_else(|| " (file missing)".to_string());
    format!(
        "{what} codegen stale at {}{where_diff}.\nRegenerate with:\n  \
         cargo run -p nmp-codegen -- gen keyed-ref-cache --out {}",
        out.display(),
        out.display()
    )
}
