use crate::manifest_edit;
use nmp_codegen::{AppManifest, NmpDependency};
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
    let note_hint = migration_note_hint(&parsed.nmp, &version);
    let next = manifest_edit::replace_nmp_section(&body, &version);
    manifest_edit::write(&manifest, &next)?;
    rewrite_app_module_dependencies(&manifest, &parsed, &version)?;

    println!("upgraded {} to NMP {version}", manifest.display());
    println!("{note_hint}");
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
                // Repoint every `nmp-*` dependency at the new release tag.
                // Consumers pin NMP by git rev (ADR-0046 /
                // `docs/architecture/external-consumers.md`), so the rewrite
                // emits a git-tag pin — the same shape `nmp init --nmp-version`
                // produces — rather than a bare crates.io version.
                if dep == "nmp-core" || dep.starts_with("nmp-") {
                    out.push_str(&format!("{dep} = {}\n", git_tag_dependency(dep, version)));
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

/// The canonical upstream git remote for NMP git-rev pins. Kept in sync with
/// `init::NMP_GIT_REMOTE`.
const NMP_GIT_REMOTE: &str = "https://github.com/pablof7z/nostr-multi-platform";

/// Render the `Cargo.toml` value for an `nmp-*` git-tag dependency at
/// `v<version>`. Matches the shape `nmp init --nmp-version` emits.
fn git_tag_dependency(krate: &str, version: &str) -> String {
    format!("{{ git = \"{NMP_GIT_REMOTE}\", tag = \"v{version}\", package = \"{krate}\" }}")
}

fn migration_note_url(version: &str) -> String {
    let release_tag = format!("nmp-v{version}");
    format!("{NMP_GIT_REMOTE}/blob/{release_tag}/docs/migration-notes/{release_tag}.md")
}

fn migration_notes_index_url(version: &str) -> String {
    let release_tag = format!("nmp-v{version}");
    format!("{NMP_GIT_REMOTE}/tree/{release_tag}/docs/migration-notes")
}

fn migration_note_hint(current: &NmpDependency, target: &str) -> String {
    let target_note = migration_note_url(target);
    match current {
        NmpDependency::Version { version } if version != target => format!(
            "migration notes: range nmp-v{version}..nmp-v{target}; read {} and target note {target_note}",
            migration_notes_index_url(target)
        ),
        _ => format!("migration note: {target_note}"),
    }
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
