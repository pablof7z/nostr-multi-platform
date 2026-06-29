//! ADR-0064 §3 (#1783) — `nmp gen action-builders` CLI handler.
//!
//! Kept in its own module (not `cli.rs`) purely as a size-management seam so
//! `cli.rs` stays under the 500-LOC ceiling (AGENTS.md / V-12). Mirrors the
//! `run_gen_projection_cache` arg-parsing shape: `--platform swift|kotlin`,
//! required `--out`, optional `--registry`, `--check` drift mode.

use std::path::{Path, PathBuf};

use nmp_codegen::ActionBuilderPlatform;

/// `nmp gen action-builders --platform swift|kotlin|ts --out <path> [--check]`.
///
/// Generates the typed write builders (`GeneratedActionBuilders`) for the byte
/// doorway from the `ACTION_BUILDERS` registry; takes no schema stdin.
///
/// `--platform swift`: generates `ActionBuilders.generated.swift`.
/// For Chirp: `apps/chirp/ios/Chirp/Bridge/Generated/ActionBuilders.generated.swift`.
///
/// `--platform kotlin`: generates `ActionBuilders.kt`.
/// For Chirp Android: `apps/chirp/android/app/src/main/java/org/nmp/android/ActionBuilders.kt`.
///
/// `--platform ts`: generates `actionBuilders.generated.ts`.
/// For Chirp Web: `web/packages/runtime-web/src/actionBuilders.generated.ts`.
///
/// `--out` is required (no app identity baked into the generic tool — #1613).
///
/// `--check` diffs against the file on disk and exits non-zero on drift. The CI
/// gate at `.github/workflows/codegen-drift.yml` uses this mode.
///
/// With an app-local `--registry`, `--check` may omit `--platform` to validate
/// the registry's schema facts and diff all declared Swift/Kotlin/TS outputs:
/// `nmp gen action-builders --registry apps/<app>/action-builders.json --check`.
pub fn run_gen_action_builders(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut platform_arg: Option<String> = None;
    let mut check = false;
    let mut out: Option<PathBuf> = None;
    let mut registry_path: Option<PathBuf> = None;
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
            "--registry" => {
                index += 1;
                registry_path = Some(
                    args.get(index)
                        .map(PathBuf::from)
                        .ok_or_else(|| "--registry requires a path".to_string())?,
                );
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{help}")),
        }
        index += 1;
    }

    if check && platform_arg.is_none() {
        let registry_path = registry_path.ok_or_else(|| {
            format!("--registry is required when --check omits --platform\n{help}")
        })?;
        if out.is_some() {
            return Err("--out is only valid with --platform".to_string());
        }
        return run_check_app_registry(&registry_path);
    }

    let platform_arg = platform_arg.ok_or_else(|| {
        format!(
            "--platform is required (swift|kotlin|ts), unless --registry is checked as a whole\n{help}"
        )
    })?;
    let platform =
        ActionBuilderPlatform::parse(&platform_arg).map_err(|e| format!("{e}\n{help}"))?;

    let loaded_registry = registry_path
        .as_ref()
        .map(|path| nmp_codegen::load_app_action_builder_registry(path))
        .transpose()?;

    let out = match (out, loaded_registry.as_ref(), registry_path.as_ref()) {
        (Some(out), _, _) => out,
        (None, Some(registry), Some(path)) => {
            resolve_registry_output(path, registry.output_for(platform))
        }
        (None, _, _) => {
            return Err(
                "--out is required unless --registry points to a contract with platform outputs"
                    .to_string(),
            );
        }
    };

    if check {
        let outcome = if let Some(registry) = loaded_registry.as_ref() {
            nmp_codegen::check_action_builders_from_registry(
                platform,
                &registry.as_registry(),
                &out,
            )
        } else {
            nmp_codegen::check_action_builders(platform, &out)
        }
        .map_err(|e| e.to_string())?;
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
                 cargo run -p nmp-codegen -- gen action-builders --platform {platform_arg}{} \
                 --out {}",
                out.display(),
                registry_arg(registry_path.as_ref()),
                out.display()
            ))
        }
    } else {
        if let Some(registry) = loaded_registry.as_ref() {
            nmp_codegen::generate_action_builders_from_registry(
                platform,
                &registry.as_registry(),
                &out,
            )
        } else {
            nmp_codegen::generate_action_builders(platform, &out)
        }
        .map_err(|e| e.to_string())?;
        println!("wrote {}", out.display());
        Ok(())
    }
}

fn resolve_registry_output(registry_path: &Path, output: &Path) -> PathBuf {
    if output.is_absolute() {
        return output.to_path_buf();
    }
    registry_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(output)
}

fn registry_arg(path: Option<&PathBuf>) -> String {
    path.map(|path| format!(" --registry {}", path.display()))
        .unwrap_or_default()
}

fn run_check_app_registry(registry_path: &Path) -> Result<(), String> {
    let outcome = nmp_codegen::check_app_action_builder_registry(registry_path)?;
    if outcome.up_to_date() {
        println!(
            "nmp gen action-builders --registry {} --check: ok ({} schemas, {} outputs)",
            registry_path.display(),
            outcome.schema_count,
            outcome.outputs.len()
        );
        return Ok(());
    }

    let stale = outcome
        .outputs
        .iter()
        .filter(|output| !output.outcome.up_to_date)
        .map(|output| {
            let where_diff = output
                .outcome
                .first_diff_line
                .map(|n| format!("first differing line {n}"))
                .unwrap_or_else(|| "file missing".to_string());
            format!(
                "- {}: {} ({where_diff})",
                platform_name(output.platform),
                output.out_path.display()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "app action-builder registry drift for {}:\n{}\n\
         Regenerate with:\n  \
         cargo run -p nmp-codegen -- gen action-builders --registry {} --platform swift\n  \
         cargo run -p nmp-codegen -- gen action-builders --registry {} --platform kotlin\n  \
         cargo run -p nmp-codegen -- gen action-builders --registry {} --platform ts",
        registry_path.display(),
        stale,
        registry_path.display(),
        registry_path.display(),
        registry_path.display()
    ))
}

fn platform_name(platform: ActionBuilderPlatform) -> &'static str {
    match platform {
        ActionBuilderPlatform::Swift => "swift",
        ActionBuilderPlatform::Kotlin => "kotlin",
        ActionBuilderPlatform::Ts => "ts",
    }
}
