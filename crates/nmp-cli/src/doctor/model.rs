use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use toml::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Level {
    Error,
    Warning,
    Info,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "info",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub id: &'static str,
    pub level: Level,
    pub subject: String,
    pub message: String,
}

impl Diagnostic {
    pub fn new(id: &'static str, level: Level, subject: String, message: String) -> Self {
        Self {
            id,
            level,
            subject,
            message,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DoctorInput {
    pub nmp_toml: Value,
    pub cargo_manifests: Vec<CargoManifest>,
    pub lock_packages: BTreeMap<String, Vec<LockPackage>>,
    pub retired_crates: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct CargoManifest {
    pub dependencies: Vec<Dependency>,
}

#[derive(Clone, Debug)]
pub struct Dependency {
    pub name: String,
    pub package: String,
    pub source: DepSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DepSource {
    Path {
        raw: String,
        absolute: PathBuf,
    },
    Git {
        url: String,
        rev: Option<String>,
        tag: Option<String>,
        branch: Option<String>,
    },
    Version(String),
    Workspace,
    Unknown,
}

impl DepSource {
    pub fn source_key(&self) -> String {
        match self {
            DepSource::Path { .. } => "path".to_string(),
            DepSource::Git { url, .. } => format!("git:{url}"),
            DepSource::Version(_) => "registry".to_string(),
            DepSource::Workspace => "workspace".to_string(),
            DepSource::Unknown => "unknown".to_string(),
        }
    }

    pub fn pin_key(&self) -> String {
        match self {
            DepSource::Path { .. } => "path".to_string(),
            DepSource::Git {
                url,
                rev,
                tag,
                branch,
            } => format!(
                "git:{url}:{}",
                rev.as_deref()
                    .map(|value| format!("rev={value}"))
                    .or_else(|| tag.as_deref().map(|value| format!("tag={value}")))
                    .or_else(|| branch.as_deref().map(|value| format!("branch={value}")))
                    .unwrap_or_else(|| "floating".to_string())
            ),
            DepSource::Version(version) => format!("registry:{version}"),
            DepSource::Workspace => "workspace".to_string(),
            DepSource::Unknown => "unknown".to_string(),
        }
    }

    pub fn display(&self) -> String {
        match self {
            DepSource::Path { raw, .. } => format!("path {raw}"),
            DepSource::Git {
                url,
                rev,
                tag,
                branch,
            } => {
                let pin = rev
                    .as_deref()
                    .map(|value| format!("rev {value}"))
                    .or_else(|| tag.as_deref().map(|value| format!("tag {value}")))
                    .or_else(|| branch.as_deref().map(|value| format!("branch {value}")))
                    .unwrap_or_else(|| "floating".to_string());
                format!("git {url} ({pin})")
            }
            DepSource::Version(version) => format!("registry version {version}"),
            DepSource::Workspace => "workspace dependency".to_string(),
            DepSource::Unknown => "unknown source".to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LockPackage {
    pub source: Option<String>,
}

pub fn load_input(manifest_path: &Path) -> Result<DoctorInput, String> {
    if !manifest_path.exists() {
        return Err(format!("{} does not exist", manifest_path.display()));
    }
    let root = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent".to_string())?
        .to_path_buf();
    let nmp_toml = parse_toml_file(manifest_path)?;
    let cargo_paths = collect_cargo_tomls(&root)?;
    let mut cargo_manifests = Vec::new();
    for path in cargo_paths {
        cargo_manifests.push(parse_cargo_manifest(&path)?);
    }
    let lock_packages = parse_cargo_lock(&root.join("Cargo.lock"))?;
    let retired_crates = parse_retired_crates()?;
    Ok(DoctorInput {
        nmp_toml,
        cargo_manifests,
        lock_packages,
        retired_crates,
    })
}

fn parse_toml_file(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    raw.parse::<Value>()
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn collect_cargo_tomls(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    collect_cargo_tomls_inner(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_cargo_tomls_inner(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in
        fs::read_dir(dir).map_err(|error| format!("failed to read {}: {error}", dir.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".git" || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect_cargo_tomls_inner(&path, out)?;
        } else if name == "Cargo.toml" {
            out.push(path);
        }
    }
    Ok(())
}

pub fn parse_cargo_manifest(path: &Path) -> Result<CargoManifest, String> {
    let value = parse_toml_file(path)?;
    let mut dependencies = Vec::new();
    collect_deps_from_table(&value, path, "dependencies", &mut dependencies);
    collect_deps_from_table(&value, path, "dev-dependencies", &mut dependencies);
    collect_deps_from_table(&value, path, "build-dependencies", &mut dependencies);
    if let Some(targets) = value.get("target").and_then(Value::as_table) {
        for (target, table) in targets {
            collect_deps_from_table(
                table,
                path,
                &format!("target.{target}.dependencies"),
                &mut dependencies,
            );
            collect_deps_from_table(
                table,
                path,
                &format!("target.{target}.dev-dependencies"),
                &mut dependencies,
            );
            collect_deps_from_table(
                table,
                path,
                &format!("target.{target}.build-dependencies"),
                &mut dependencies,
            );
        }
    }
    Ok(CargoManifest { dependencies })
}

fn collect_deps_from_table(
    root: &Value,
    manifest_path: &Path,
    section: &str,
    out: &mut Vec<Dependency>,
) {
    let Some(table) = root
        .get(section.rsplit('.').next().unwrap_or(section))
        .and_then(Value::as_table)
    else {
        return;
    };
    for (name, value) in table {
        if !name.starts_with("nmp-") {
            continue;
        }
        let package = value
            .as_table()
            .and_then(|table| table.get("package"))
            .and_then(Value::as_str)
            .unwrap_or(name)
            .to_string();
        out.push(Dependency {
            name: name.to_string(),
            package,
            source: dep_source(value, manifest_path),
        });
    }
}

fn dep_source(value: &Value, manifest_path: &Path) -> DepSource {
    match value {
        Value::String(version) => DepSource::Version(version.clone()),
        Value::Table(table) => {
            if table.get("workspace").and_then(Value::as_bool) == Some(true) {
                return DepSource::Workspace;
            }
            if let Some(path) = table.get("path").and_then(Value::as_str) {
                let base = manifest_path.parent().unwrap_or_else(|| Path::new("."));
                return DepSource::Path {
                    raw: path.to_string(),
                    absolute: normalize_path(&base.join(path)),
                };
            }
            if let Some(url) = table.get("git").and_then(Value::as_str) {
                return DepSource::Git {
                    url: url.to_string(),
                    rev: table
                        .get("rev")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    tag: table
                        .get("tag")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    branch: table
                        .get("branch")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                };
            }
            table
                .get("version")
                .and_then(Value::as_str)
                .map(|version| DepSource::Version(version.to_string()))
                .unwrap_or(DepSource::Unknown)
        }
        _ => DepSource::Unknown,
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn parse_cargo_lock(path: &Path) -> Result<BTreeMap<String, Vec<LockPackage>>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let value = parse_toml_file(path)?;
    let mut out: BTreeMap<String, Vec<LockPackage>> = BTreeMap::new();
    if let Some(packages) = value.get("package").and_then(Value::as_array) {
        for package in packages {
            let Some(name) = package.get("name").and_then(Value::as_str) else {
                continue;
            };
            if !name.starts_with("nmp-") {
                continue;
            }
            out.entry(name.to_string()).or_default().push(LockPackage {
                source: package
                    .get("source")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
            });
        }
    }
    Ok(out)
}

fn parse_retired_crates() -> Result<BTreeMap<String, String>, String> {
    const RELEASE: &str = include_str!("../../../../release/nmp-release.toml");
    let value = RELEASE
        .parse::<Value>()
        .map_err(|error| format!("failed to parse release/nmp-release.toml: {error}"))?;
    let mut out = BTreeMap::new();
    if let Some(items) = value.get("retired_crates").and_then(Value::as_array) {
        for item in items {
            let Some(name) = item.get("name").and_then(Value::as_str) else {
                continue;
            };
            let migration = item
                .get("migration")
                .and_then(Value::as_str)
                .unwrap_or("remove this dependency; the crate has been retired");
            out.insert(name.to_string(), migration.to_string());
        }
    }
    Ok(out)
}
