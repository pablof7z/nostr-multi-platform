//! CLI parsing for read-side projection helper generators.

use std::path::{Path, PathBuf};

/// `nmp gen keyed-ref-cache --platform swift|kotlin|ts --out <path> [--check]`.
///
/// ADR-0070 Lane A (#1671) — generates the per-key (row-keyed) reference cache
/// (`KeyedRefCache`) for keyed projections (`refs.profile` / `refs.event`).
/// Driven by `KEYED_PROJECTIONS`; takes no schema stdin. Mirrors
/// `gen projection-cache`: `--out` required, `--check` diffs on disk.
/// `--platform ts` (#2722) is the `@nmpis/runtime-web` twin.
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
                    .ok_or_else(|| "--platform requires swift|kotlin|ts".to_string())?;
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
                        "keyed-ref-cache",
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
                        "keyed-ref-cache --platform kotlin",
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
        "ts" => {
            if check {
                let outcome =
                    nmp_codegen::check_ts_keyed_ref_cache(&out).map_err(|e| e.to_string())?;
                if outcome.up_to_date {
                    println!(
                        "nmp gen keyed-ref-cache --platform ts --check: ok ({})",
                        out.display()
                    );
                    Ok(())
                } else {
                    Err(stale_message(
                        "keyed-ref-cache (ts)",
                        "keyed-ref-cache --platform ts",
                        &out,
                        outcome.first_diff_line,
                    ))
                }
            } else {
                nmp_codegen::generate_ts_keyed_ref_cache(&out).map_err(|e| e.to_string())?;
                println!("wrote {}", out.display());
                Ok(())
            }
        }
        other => Err(format!(
            "unknown --platform {other:?}: expected swift, kotlin, or ts\n{help}"
        )),
    }
}

/// `nmp gen projection-contract --platform ts --out <path> [--check]`.
///
/// #2722 — generates the read-side TypeScript `PROJECTION_CONTRACT` table for
/// `@nmpis/runtime-web` from the SAME neutral manifest the Swift typed decoders
/// consume via `projection_contract::contract_for`. Takes no schema stdin.
/// `--platform` is required and currently accepts only `ts` (the only
/// consumer); the flag is spelled out so a future platform can be added
/// without a breaking CLI shape change.
pub fn run_gen_projection_contract(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut platform: Option<String> = None;
    let mut check = false;
    let mut out: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--platform" => {
                index += 1;
                platform = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--platform requires ts".to_string())?,
                );
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
    match platform.as_deref() {
        Some("ts") => {
            if check {
                let outcome =
                    nmp_codegen::check_ts_projection_contract(&out).map_err(|e| e.to_string())?;
                if outcome.up_to_date {
                    println!(
                        "nmp gen projection-contract --check: ok ({})",
                        out.display()
                    );
                    Ok(())
                } else {
                    Err(stale_message(
                        "projection-contract (ts)",
                        "projection-contract --platform ts",
                        &out,
                        outcome.first_diff_line,
                    ))
                }
            } else {
                nmp_codegen::generate_ts_projection_contract(&out).map_err(|e| e.to_string())?;
                println!("wrote {}", out.display());
                Ok(())
            }
        }
        other => Err(format!("unknown --platform {other:?}: expected ts\n{help}")),
    }
}

fn stale_message(what: &str, command: &str, out: &Path, first_diff_line: Option<usize>) -> String {
    let where_diff = first_diff_line
        .map(|n| format!(" (first differing line {n})"))
        .unwrap_or_else(|| " (file missing)".to_string());
    format!(
        "{what} codegen stale at {}{where_diff}.\nRegenerate with:\n  \
         cargo run -p nmp-codegen -- gen {command} --out {}",
        out.display(),
        out.display()
    )
}
