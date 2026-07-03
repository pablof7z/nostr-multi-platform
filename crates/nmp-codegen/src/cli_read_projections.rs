//! CLI handler for app-local read-projection registries.

use std::path::{Path, PathBuf};

use nmp_codegen::ReadProjectionPlatform;

pub fn run_gen_read_projections(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut platform_arg: Option<String> = None;
    let mut check = false;
    let mut out: Option<PathBuf> = None;
    let mut registry_path: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--platform" => {
                index += 1;
                platform_arg = Some(args.get(index).cloned().ok_or_else(|| {
                    "--platform requires a read-projections platform".to_string()
                })?);
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

    let registry_path = registry_path
        .ok_or_else(|| format!("--registry is required for read-projections\n{help}"))?;
    if check && platform_arg.is_none() {
        if out.is_some() {
            return Err("--out is only valid with --platform".to_string());
        }
        return run_check_app_registry(&registry_path);
    }

    let platform_arg = platform_arg.ok_or_else(|| {
        format!("--platform is required unless --registry is checked as a whole\n{help}")
    })?;
    let platform = ReadProjectionPlatform::parse(&platform_arg)?;
    let loaded = nmp_codegen::load_app_read_projection_registry(&registry_path)?;
    nmp_codegen::validate_app_read_projection_schema_files(&registry_path, &loaded)?;
    let out =
        out.unwrap_or_else(|| resolve_registry_output(&registry_path, loaded.output_for(platform)));
    let source_label = registry_path.to_string_lossy();

    if check {
        let outcome = nmp_codegen::check_read_projections_from_registry(
            platform,
            &loaded,
            &source_label,
            &out,
        )?;
        if outcome.up_to_date {
            println!(
                "nmp gen read-projections --platform {} --check: ok ({})",
                platform.name(),
                out.display()
            );
            Ok(())
        } else {
            let where_diff = outcome
                .first_diff_line
                .map(|n| format!(" (first differing line {n})"))
                .unwrap_or_else(|| " (file missing)".to_string());
            Err(format!(
                "read-projections ({}) codegen stale at {}{where_diff}.\n\
                 Regenerate with:\n  \
                 cargo run -p nmp-codegen -- gen read-projections --registry {} --platform {}",
                platform.name(),
                out.display(),
                registry_path.display(),
                platform.name()
            ))
        }
    } else {
        nmp_codegen::generate_read_projections_from_registry(
            platform,
            &loaded,
            &source_label,
            &out,
        )?;
        println!("wrote {}", out.display());
        Ok(())
    }
}

fn run_check_app_registry(registry_path: &Path) -> Result<(), String> {
    let outcome = nmp_codegen::check_app_read_projection_registry(registry_path)?;
    if outcome.up_to_date() {
        println!(
            "nmp gen read-projections --registry {} --check: ok ({} schemas, {} outputs)",
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
                output.platform.name(),
                output.out_path.display()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "app read-projections registry drift for {}:\n{}\n\
         Regenerate with:\n  \
         cargo run -p nmp-codegen -- gen read-projections --registry {} --platform swift-typed-decoders\n  \
         cargo run -p nmp-codegen -- gen read-projections --registry {} --platform swift-projection-cache\n  \
         cargo run -p nmp-codegen -- gen read-projections --registry {} --platform kotlin-projection-cache",
        registry_path.display(),
        stale,
        registry_path.display(),
        registry_path.display(),
        registry_path.display()
    ))
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
