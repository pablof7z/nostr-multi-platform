//! `nmp add component <id>` — install app-owned source components.
//!
//! Components are copied source, not linked framework packages. The lock file
//! records the upstream baseline so a later update command can preserve local
//! app edits instead of overwriting them.

mod lock;
mod registry;

use lock::{ComponentLock, LockedComponent, LockedFile};
use registry::{Registry, RegistryComponent, RegistryFile};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const LOCK_FILE: &str = "nmp.components.lock";

pub fn run_add(args: &[String]) -> Result<(), String> {
    if args.first().map(String::as_str) != Some("component") {
        return Err(format!("usage: {USAGE}"));
    }

    let request = AddRequest::parse(&args[1..])?;
    let registry = Registry::load(request.registry_path.clone())?;
    let order = registry.resolve(&request.id)?;
    let mut lock = ComponentLock::read(&request.root, LOCK_FILE)?;

    for component in &order {
        if lock.components.iter().any(|entry| entry.id == component.id) {
            return Err(format!("component `{}` is already installed", component.id));
        }
    }

    let planned = plan_files(&request, &registry, &order)?;
    write_files(&request.root, &planned)?;
    write_lock_entries(&request.root, &registry, &mut lock, &order, &planned)?;

    println!(
        "installed component `{}` into {}",
        request.id,
        request.root.display()
    );
    Ok(())
}

fn plan_files<'a>(
    request: &AddRequest,
    registry: &Registry,
    order: &[&'a RegistryComponent],
) -> Result<Vec<PlannedFile<'a>>, String> {
    let mut planned = Vec::new();
    for component in order {
        for file in component
            .files
            .iter()
            .filter(|file| include_role(file, &request.roles))
        {
            let source = safe_relative(&file.source)?;
            let target = safe_relative(&file.target)?;
            let content = registry.read_source(source)?;
            let destination = request.root.join(target);
            if destination.exists() {
                return Err(format!(
                    "target file already exists: {}",
                    destination.display()
                ));
            }
            planned.push(PlannedFile {
                component,
                role: file.role.clone(),
                source: source.to_string_lossy().into_owned(),
                target: target.to_path_buf(),
                content,
            });
        }
    }
    Ok(planned)
}

fn write_files(root: &Path, planned: &[PlannedFile<'_>]) -> Result<(), String> {
    for file in planned {
        let destination = root.join(&file.target);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&destination, file.content.as_bytes())
            .map_err(|e| format!("{}: {e}", destination.display()))?;
    }
    Ok(())
}

fn write_lock_entries(
    root: &Path,
    registry: &Registry,
    lock: &mut ComponentLock,
    order: &[&RegistryComponent],
    planned: &[PlannedFile<'_>],
) -> Result<(), String> {
    for component in order {
        let files = planned
            .iter()
            .filter(|file| file.component.id == component.id)
            .map(|file| LockedFile {
                path: file.target.to_string_lossy().into_owned(),
                role: file.role.clone(),
                source: file.source.clone(),
                source_sha256: sha256_hex(&file.content),
            })
            .collect::<Vec<_>>();
        lock.components.push(LockedComponent {
            id: component.id.clone(),
            version: component.version.clone(),
            registry: registry.id.clone(),
            target: component.target.clone(),
            files,
        });
    }
    lock.write(root, LOCK_FILE)
}

fn include_role(file: &RegistryFile, roles: &HashSet<String>) -> bool {
    file.role == "source" || roles.contains(&file.role)
}

struct PlannedFile<'a> {
    component: &'a RegistryComponent,
    role: String,
    source: String,
    target: PathBuf,
    content: String,
}

struct AddRequest {
    id: String,
    root: PathBuf,
    registry_path: Option<PathBuf>,
    roles: HashSet<String>,
}

impl AddRequest {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut id: Option<String> = None;
        let mut root = PathBuf::from(".");
        let mut registry_path: Option<PathBuf> = None;
        let mut roles = HashSet::new();
        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--path" => {
                    index += 1;
                    root = args
                        .get(index)
                        .map(PathBuf::from)
                        .ok_or_else(|| "--path requires a directory".to_string())?;
                }
                "--registry" => {
                    index += 1;
                    registry_path = Some(
                        args.get(index)
                            .map(PathBuf::from)
                            .ok_or_else(|| "--registry requires a directory".to_string())?,
                    );
                }
                "--with" => {
                    index += 1;
                    let value = args
                        .get(index)
                        .ok_or_else(|| "--with requires comma-separated roles".to_string())?;
                    roles.extend(
                        value
                            .split(',')
                            .map(str::trim)
                            .filter(|role| !role.is_empty())
                            .map(ToOwned::to_owned),
                    );
                }
                flag if flag.starts_with('-') => return Err(format!("unknown argument {flag}")),
                positional => {
                    if id.is_some() {
                        return Err("unexpected extra component id".to_string());
                    }
                    id = Some(positional.to_string());
                }
            }
            index += 1;
        }

        Ok(Self {
            id: id.ok_or_else(|| format!("usage: {USAGE}"))?,
            root,
            registry_path,
            roles,
        })
    }
}

fn safe_relative(path: &str) -> Result<&Path, String> {
    let path = Path::new(path);
    let valid = path
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    if path.as_os_str().is_empty() || path.is_absolute() || !valid {
        return Err(format!("invalid relative path `{}`", path.display()));
    }
    Ok(path)
}

fn sha256_hex(content: &str) -> String {
    Sha256::digest(content.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

const USAGE: &str =
    "nmp add component <id> [--path DIR] [--registry DIR] [--with doc,example,test,fixture]";
