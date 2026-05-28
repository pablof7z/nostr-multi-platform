use crate::manifest_edit;
use nmp_codegen::AppManifest;
use std::path::PathBuf;

pub fn run(args: &[String]) -> Result<(), String> {
    let mut manifest = PathBuf::from("nmp.toml");
    let mut to = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--manifest" => {
                index += 1;
                manifest = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--manifest requires a path".to_string())?;
            }
            "--to" => {
                index += 1;
                to = Some(
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| "--to requires a version".to_string())?,
                );
            }
            other => return Err(format!("unknown argument {other}")),
        }
        index += 1;
    }

    let version =
        to.ok_or_else(|| "usage: nmp upgrade --to VERSION [--manifest nmp.toml]".to_string())?;
    validate_version(&version)?;
    let body = manifest_edit::read(&manifest)?;
    let parsed = AppManifest::parse(&body)?;
    let next = manifest_edit::replace_nmp_section(&body, &version);
    manifest_edit::write(&manifest, &next)?;
    rewrite_app_module_dependencies(&manifest, &parsed, &version)?;

    println!("upgraded {} to NMP {version}", manifest.display());
    println!("next:");
    println!("  nmp gen modules");
    println!("  nmp gen modules --check");
    println!("  nmp doctor --manifest {}", manifest.display());
    Ok(())
}

fn rewrite_app_module_dependencies(
    manifest_path: &std::path::Path,
    manifest: &AppManifest,
    version: &str,
) -> Result<(), String> {
    let root = manifest_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    for module in &manifest.modules.app {
        let cargo_toml = root.join("crates").join(module).join("Cargo.toml");
        if !cargo_toml.is_file() {
            continue;
        }
        let body = manifest_edit::read(&cargo_toml)?;
        let mut changed = false;
        let mut out = String::new();
        for line in body.lines() {
            if let Some((name, _rest)) = line.split_once('=') {
                let dep = name.trim();
                if dep == "nmp-core" || dep.starts_with("nmp-") {
                    out.push_str(&format!("{dep} = \"{version}\"\n"));
                    changed = true;
                    continue;
                }
            }
            out.push_str(line);
            out.push('\n');
        }
        if changed {
            manifest_edit::write(&cargo_toml, &out)?;
        }
    }
    Ok(())
}

fn validate_version(version: &str) -> Result<(), String> {
    let parts = version.split('.').collect::<Vec<_>>();
    let valid = parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
    if valid {
        Ok(())
    } else {
        Err(format!(
            "invalid NMP version `{version}`: expected MAJOR.MINOR.PATCH"
        ))
    }
}
