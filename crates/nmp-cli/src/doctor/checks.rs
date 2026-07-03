use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use toml::Value;

use super::model::{DepSource, Dependency, Diagnostic, DoctorInput, Level};

pub fn run_checks(input: &DoctorInput) -> Vec<Diagnostic> {
    let deps = nmp_dependencies(input);
    let mut diagnostics = Vec::new();
    check_same_source(&deps, &mut diagnostics);
    check_lockfile(input, &deps, &mut diagnostics);
    check_retired(input, &deps, &mut diagnostics);
    check_paths(&deps, &mut diagnostics);
    report_baseline(&deps, &mut diagnostics);
    check_companions(input, &deps, &mut diagnostics);
    check_nmp_toml(input, &deps, &mut diagnostics);
    diagnostics
}

fn nmp_dependencies(input: &DoctorInput) -> Vec<&Dependency> {
    input
        .cargo_manifests
        .iter()
        .flat_map(|manifest| manifest.dependencies.iter())
        .filter(|dep| dep.name.starts_with("nmp-"))
        .collect()
}

fn check_same_source(deps: &[&Dependency], out: &mut Vec<Diagnostic>) {
    let sources: BTreeSet<String> = deps.iter().map(|dep| dep.source.source_key()).collect();
    if sources.len() > 1 {
        out.push(Diagnostic::new(
            "D01",
            Level::Error,
            "nmp dependency sources".to_string(),
            format!(
                "nmp-* dependencies use mixed sources: {}",
                sources.into_iter().collect::<Vec<_>>().join(", ")
            ),
        ));
    }
}

fn check_lockfile(input: &DoctorInput, deps: &[&Dependency], out: &mut Vec<Diagnostic>) {
    if deps.is_empty() {
        return;
    }
    if input.lock_packages.is_empty() {
        out.push(Diagnostic::new(
            "D02",
            Level::Error,
            "Cargo.lock".to_string(),
            "Cargo.lock is missing or contains no locked nmp-* packages".to_string(),
        ));
        return;
    }
    for dep in deps {
        let Some(locked) = input.lock_packages.get(&dep.package) else {
            out.push(Diagnostic::new(
                "D02",
                Level::Error,
                dep.package.clone(),
                format!("{} is declared but absent from Cargo.lock", dep.package),
            ));
            continue;
        };
        let distinct: BTreeSet<Option<String>> = locked
            .iter()
            .map(|package| package.source.clone())
            .collect();
        if distinct.len() > 1 {
            out.push(Diagnostic::new(
                "D02",
                Level::Error,
                dep.package.clone(),
                format!("Cargo.lock has multiple sources for {}", dep.package),
            ));
            continue;
        }
        let source = locked.first().and_then(|package| package.source.as_deref());
        if !lock_source_matches(&dep.source, source) {
            out.push(Diagnostic::new(
                "D02",
                Level::Error,
                dep.package.clone(),
                format!(
                    "Cargo.lock source for {} does not match manifest source {}",
                    dep.package,
                    dep.source.display()
                ),
            ));
        }
    }
}

fn lock_source_matches(source: &DepSource, locked: Option<&str>) -> bool {
    match source {
        DepSource::Path { .. } => locked.is_none(),
        DepSource::Git {
            url,
            rev,
            tag,
            branch,
        } => {
            let Some(locked) = locked else {
                return false;
            };
            if !locked.starts_with(&format!("git+{url}")) {
                return false;
            }
            if let Some(rev) = rev {
                locked.contains(&format!("?rev={rev}")) || locked.ends_with(&format!("#{rev}"))
            } else if let Some(tag) = tag {
                locked.contains(&format!("?tag={tag}"))
            } else if let Some(branch) = branch {
                locked.contains(&format!("?branch={branch}"))
            } else {
                true
            }
        }
        DepSource::Version(_) => locked.is_some_and(|locked| locked.starts_with("registry+")),
        DepSource::Workspace | DepSource::Unknown => true,
    }
}

fn check_retired(input: &DoctorInput, deps: &[&Dependency], out: &mut Vec<Diagnostic>) {
    for dep in deps {
        if let Some(migration) = input.retired_crates.get(&dep.package) {
            out.push(Diagnostic::new(
                "D03",
                Level::Error,
                dep.package.clone(),
                format!("{} is retired; {migration}", dep.package),
            ));
        }
    }
}

fn check_paths(deps: &[&Dependency], out: &mut Vec<Diagnostic>) {
    for dep in deps {
        let DepSource::Path { absolute, .. } = &dep.source else {
            continue;
        };
        let cargo_toml = absolute.join("Cargo.toml");
        if !cargo_toml.exists() {
            out.push(Diagnostic::new(
                "D04",
                Level::Error,
                dep.name.clone(),
                format!(
                    "path dependency {} does not point at a crate",
                    absolute.display()
                ),
            ));
            continue;
        }
        match package_name(&cargo_toml) {
            Ok(name) if name == dep.package => {}
            Ok(name) => out.push(Diagnostic::new(
                "D04",
                Level::Error,
                dep.name.clone(),
                format!(
                    "path dependency package is `{name}`, expected `{}`",
                    dep.package
                ),
            )),
            Err(error) => out.push(Diagnostic::new(
                "D04",
                Level::Error,
                dep.name.clone(),
                error,
            )),
        }
    }
}

fn package_name(path: &Path) -> Result<String, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let value = raw
        .parse::<Value>()
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("{} has no [package].name", path.display()))
}

fn report_baseline(deps: &[&Dependency], out: &mut Vec<Diagnostic>) {
    let mut sources: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for dep in deps {
        sources
            .entry(dep.source.display())
            .or_default()
            .push(dep.package.clone());
    }
    let message = if sources.is_empty() {
        "no nmp-* dependencies found".to_string()
    } else {
        sources
            .into_iter()
            .map(|(source, mut crates)| {
                crates.sort();
                crates.dedup();
                format!("{source}: {}", crates.join(", "))
            })
            .collect::<Vec<_>>()
            .join("; ")
    };
    out.push(Diagnostic::new(
        "D05",
        Level::Info,
        "nmp dependency baseline".to_string(),
        message,
    ));
}

fn check_companions(input: &DoctorInput, deps: &[&Dependency], out: &mut Vec<Diagnostic>) {
    let by_name: BTreeMap<&str, &Dependency> = deps
        .iter()
        .map(|dep| (dep.package.as_str(), *dep))
        .collect();
    let Some(companions) = input.nmp_toml.get("companions").and_then(Value::as_table) else {
        return;
    };
    for (group, members) in companions {
        let Some(members) = members.as_array() else {
            out.push(Diagnostic::new(
                "D07",
                Level::Error,
                format!("companions.{group}"),
                "companion groups must be arrays of crate names".to_string(),
            ));
            continue;
        };
        let mut pins = BTreeSet::new();
        for member in members.iter().filter_map(Value::as_str) {
            if let Some(dep) = by_name.get(member) {
                pins.insert(dep.source.pin_key());
            }
        }
        if pins.len() > 1 {
            out.push(Diagnostic::new(
                "D06",
                Level::Warning,
                format!("companions.{group}"),
                format!(
                    "companion crates are pinned to different NMP revisions: {}",
                    pins.into_iter().collect::<Vec<_>>().join(", ")
                ),
            ));
        }
    }
}

fn check_nmp_toml(input: &DoctorInput, deps: &[&Dependency], out: &mut Vec<Diagnostic>) {
    if input
        .nmp_toml
        .get("app")
        .and_then(|app| app.get("name"))
        .and_then(Value::as_str)
        .is_none()
    {
        out.push(Diagnostic::new(
            "D07",
            Level::Error,
            "app.name".to_string(),
            "nmp.toml must declare [app].name".to_string(),
        ));
    }

    let declared = declared_modules(input, out);
    let deps: BTreeSet<&str> = deps.iter().map(|dep| dep.package.as_str()).collect();
    for module in declared {
        if !deps.contains(module.as_str()) {
            out.push(Diagnostic::new(
                "D07",
                Level::Error,
                module.clone(),
                format!("nmp.toml references `{module}`, but no Cargo.toml depends on it"),
            ));
        }
    }
}

fn declared_modules(input: &DoctorInput, out: &mut Vec<Diagnostic>) -> Vec<String> {
    let mut modules = vec!["nmp-core".to_string()];
    let Some(table) = input.nmp_toml.get("modules").and_then(Value::as_table) else {
        return modules;
    };
    if let Some(kernel) = table.get("kernel") {
        match kernel.as_str() {
            Some(value) => modules[0] = value.to_string(),
            None => out.push(Diagnostic::new(
                "D07",
                Level::Error,
                "modules.kernel".to_string(),
                "[modules].kernel must be a crate-name string".to_string(),
            )),
        }
    }
    for key in ["protocol", "app"] {
        let Some(value) = table.get(key) else {
            continue;
        };
        let Some(values) = value.as_array() else {
            out.push(Diagnostic::new(
                "D07",
                Level::Error,
                format!("modules.{key}"),
                format!("[modules].{key} must be an array of crate-name strings"),
            ));
            continue;
        };
        for value in values {
            if let Some(value) = value.as_str() {
                modules.push(value.to_string());
            } else {
                out.push(Diagnostic::new(
                    "D07",
                    Level::Error,
                    format!("modules.{key}"),
                    format!("[modules].{key} contains a non-string value"),
                ));
            }
        }
    }
    modules
}
