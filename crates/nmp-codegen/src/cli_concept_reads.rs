//! #2899 — `nmp gen concept-reads` CLI handler.
//!
//! Kept separate from `main.rs` so the top-level CLI dispatcher stays small.

use std::path::{Path, PathBuf};

use nmp_codegen::{ConceptReadPlatform, LoadedAppConceptReadRegistry};

/// `nmp gen concept-reads --registry <path> --platform rust|swift|kotlin [--out <path>] [--check]`.
pub fn run_gen_concept_reads(args: Vec<String>, help: &str) -> Result<(), String> {
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
                        .ok_or_else(|| "--platform requires rust|swift|kotlin".to_string())?,
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
        let registry_path =
            registry_path.ok_or_else(|| format!("--registry is required\n{help}"))?;
        if out.is_some() {
            return Err("--out is only valid with --platform".to_string());
        }
        return run_check_app_registry(&registry_path);
    }

    let platform_arg = platform_arg.ok_or_else(|| {
        format!(
            "--platform is required (rust|swift|kotlin), unless registry is checked as a whole\n{help}"
        )
    })?;
    let platform = ConceptReadPlatform::parse(&platform_arg).map_err(|e| format!("{e}\n{help}"))?;
    let registry_path = registry_path
        .ok_or_else(|| format!("--registry is required for concept-read generation\n{help}"))?;
    let loaded = nmp_codegen::load_app_concept_read_registry(&registry_path)?;
    validate_platform_ready(platform, &loaded)?;
    let out = match out {
        Some(out) => out,
        None => {
            resolve_registry_output(&registry_path, registry_platform_output(platform, &loaded)?)
        }
    };

    if check {
        let outcome = nmp_codegen::check_concept_reads_from_registry(platform, &loaded, &out)
            .map_err(|e| e.to_string())?;
        if outcome.up_to_date {
            println!(
                "nmp gen concept-reads --platform {platform_arg} --check: ok ({})",
                out.display()
            );
            Ok(())
        } else {
            let where_diff = outcome
                .first_diff_line
                .map(|n| format!(" (first differing line {n})"))
                .unwrap_or_else(|| " (file missing)".to_string());
            Err(format!(
                "concept-reads ({platform_arg}) codegen stale at {}{where_diff}.\n\
         Regenerate with:\n  \
                 cargo run -p nmp-codegen -- gen concept-reads --registry {} --platform {platform_arg}",
                out.display(),
                registry_path.display()
            ))
        }
    } else {
        nmp_codegen::generate_concept_reads_from_registry(platform, &loaded, &out)
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

fn registry_platform_output(
    platform: ConceptReadPlatform,
    registry: &LoadedAppConceptReadRegistry,
) -> Result<&Path, String> {
    match platform {
        ConceptReadPlatform::Rust => Ok(&registry.outputs.rust),
        ConceptReadPlatform::Swift => registry.outputs.swift.as_deref().ok_or_else(|| {
            "--out is required because outputs.swift is not declared in the registry".to_string()
        }),
        ConceptReadPlatform::Kotlin => registry.outputs.kotlin.as_deref().ok_or_else(|| {
            "--out is required because outputs.kotlin is not declared in the registry".to_string()
        }),
    }
}

fn validate_platform_ready(
    platform: ConceptReadPlatform,
    registry: &LoadedAppConceptReadRegistry,
) -> Result<(), String> {
    if matches!(platform, ConceptReadPlatform::Kotlin)
        && (registry.outputs.kotlin_package.is_none()
            || registry.outputs.kotlin_uniffi_package.is_none())
    {
        return Err(
            "outputs.kotlin_package and outputs.kotlin_uniffi_package are required for --platform kotlin"
                .to_string(),
        );
    }
    Ok(())
}

fn run_check_app_registry(registry_path: &Path) -> Result<(), String> {
    let outcome = nmp_codegen::check_app_concept_read_registry(registry_path)?;
    if outcome.up_to_date() {
        println!(
            "nmp gen concept-reads --registry {} --check: ok ({} reads, {} outputs)",
            registry_path.display(),
            outcome.read_count,
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
        "app concept-read registry drift for {}:\n{}\n\
         Regenerate with:\n  \
         cargo run -p nmp-codegen -- gen concept-reads --registry {} --platform rust\n  \
         cargo run -p nmp-codegen -- gen concept-reads --registry {} --platform swift\n  \
         cargo run -p nmp-codegen -- gen concept-reads --registry {} --platform kotlin",
        registry_path.display(),
        stale,
        registry_path.display(),
        registry_path.display(),
        registry_path.display()
    ))
}

fn platform_name(platform: ConceptReadPlatform) -> &'static str {
    match platform {
        ConceptReadPlatform::Rust => "rust",
        ConceptReadPlatform::Swift => "swift",
        ConceptReadPlatform::Kotlin => "kotlin",
    }
}
