//! ADR-0064 §3 (#1783) — `nmp gen action-builders` CLI handler.
//!
//! Kept in its own module (not `cli.rs`) purely as a size-management seam so
//! `cli.rs` stays under the 500-LOC ceiling (AGENTS.md / V-12). Mirrors the
//! `run_gen_projection_cache` arg-parsing shape: `--platform swift|kotlin`,
//! required `--out`, `--check` drift mode.

use std::path::PathBuf;

use nmp_codegen::ActionBuilderPlatform;

/// `nmp gen action-builders --platform swift|kotlin|ts --out <path> [--check]`.
///
/// Generates the typed write builders (`GeneratedActionBuilders`) for the byte
/// doorway from the `ACTION_BUILDERS` registry; takes no schema stdin.
///
/// `--platform swift`: generates `ActionBuilders.generated.swift`.
/// For Chirp: `ios/Chirp/Chirp/Bridge/Generated/ActionBuilders.generated.swift`.
///
/// `--platform kotlin`: generates `ActionBuilders.kt`.
/// For Chirp Android: `android/app/src/main/java/org/nmp/android/ActionBuilders.kt`.
///
/// `--platform ts`: generates `actionBuilders.generated.ts`.
/// For Chirp Web: `web/packages/runtime-web/src/actionBuilders.generated.ts`.
///
/// `--out` is required (no app identity baked into the generic tool — #1613).
///
/// `--check` diffs against the file on disk and exits non-zero on drift. The CI
/// gate at `.github/workflows/codegen-drift.yml` uses this mode.
pub fn run_gen_action_builders(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut platform_arg: Option<String> = None;
    let mut check = false;
    let mut out: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--platform" => {
                index += 1;
                platform_arg = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--platform requires swift|kotlin|ts".to_string())?,
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

    let platform_arg = platform_arg
        .ok_or_else(|| format!("--platform is required (swift|kotlin|ts)\n{help}"))?;
    let platform = ActionBuilderPlatform::parse(&platform_arg).map_err(|e| format!("{e}\n{help}"))?;

    let out = out.ok_or_else(|| {
        "--out is required (e.g. --out <app-path>/ActionBuilders.generated.swift for \
         --platform swift, or --out <app-path>/ActionBuilders.kt for --platform kotlin)"
            .to_string()
    })?;

    if check {
        let outcome =
            nmp_codegen::check_action_builders(platform, &out).map_err(|e| e.to_string())?;
        if outcome.up_to_date {
            println!(
                "nmp gen action-builders --platform {platform_arg} --check: ok ({})",
                out.display()
            );
            Ok(())
        } else {
            let where_diff = outcome
                .first_diff_line
                .map(|n| format!(" (first differing line {n})"))
                .unwrap_or_else(|| " (file missing)".to_string());
            Err(format!(
                "action-builders ({platform_arg}) codegen stale at {}{where_diff}.\n\
                 Regenerate with:\n  \
                 cargo run -p nmp-codegen -- gen action-builders --platform {platform_arg} \
                 --out {}",
                out.display(),
                out.display()
            ))
        }
    } else {
        nmp_codegen::generate_action_builders(platform, &out).map_err(|e| e.to_string())?;
        println!("wrote {}", out.display());
        Ok(())
    }
}
