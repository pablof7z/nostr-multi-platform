//! `nmp add component <id>` — install app-owned source components.
//! `nmp update component <id>` — refresh installed sources from the registry
//! while preserving locally edited files.
//!
//! Components are copied source, not linked framework packages. The lock file
//! records the upstream baseline so the update command can preserve local
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

const ADD_USAGE: &str =
    "nmp add component <id> [--path DIR] [--registry DIR] [--with doc,example,test,fixture]";
const UPDATE_USAGE: &str = "nmp update component <id> [--path DIR] [--registry DIR]";

pub fn run_add(args: &[String]) -> Result<(), String> {
    if args.first().map(String::as_str) != Some("component") {
        return Err(format!("usage: {ADD_USAGE}"));
    }

    let request = AddRequest::parse(&args[1..])?;
    let registry = Registry::load(request.registry_path.clone())?;
    let order = registry.resolve(&request.id)?;
    let mut lock = ComponentLock::read(&request.root, LOCK_FILE)?;

    // Only the explicitly-requested component is rejected when already
    // installed. Already-installed transitive dependencies are skipped
    // silently so a user can install sibling components that share a
    // common dep without manually de-duping.
    if lock
        .components
        .iter()
        .any(|entry| entry.id == request.id)
    {
        return Err(format!("component `{}` is already installed", request.id));
    }

    let to_install: Vec<&RegistryComponent> = order
        .iter()
        .filter(|component| {
            !lock
                .components
                .iter()
                .any(|entry| entry.id == component.id)
        })
        .copied()
        .collect();

    let planned = plan_files(&request, &registry, &to_install)?;
    write_files(&request.root, &planned)?;
    write_lock_entries(&request.root, &registry, &mut lock, &to_install, &planned)?;

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
            id: id.ok_or_else(|| format!("usage: {ADD_USAGE}"))?,
            root,
            registry_path,
            roles,
        })
    }
}

pub fn run_update(args: &[String]) -> Result<(), String> {
    if args.first().map(String::as_str) != Some("component") {
        return Err(format!("usage: {UPDATE_USAGE}"));
    }

    let request = UpdateRequest::parse(&args[1..])?;
    let registry = Registry::load(request.registry_path.clone())?;
    let mut lock = ComponentLock::read(&request.root, LOCK_FILE)?;

    let entry_index = lock
        .components
        .iter()
        .position(|entry| entry.id == request.id)
        .ok_or_else(|| format!("component `{}` is not installed", request.id))?;

    // The registry entry for this component (does not resolve dependencies —
    // update is per-component; the user runs it again for each dep they care
    // about).
    let target_component = registry
        .resolve(&request.id)?
        .into_iter()
        .find(|component| component.id == request.id)
        .ok_or_else(|| format!("unknown component `{}` in registry", request.id))?;

    let mut updated = 0usize;
    let mut conflicts = 0usize;

    // Snapshot the current locked files; we rebuild the entry in place so the
    // borrow checker doesn't catch us holding two refs into `lock` at once.
    let locked_files: Vec<LockedFile> = std::mem::take(&mut lock.components[entry_index].files);
    let mut new_locked_files = Vec::with_capacity(locked_files.len());

    for locked in locked_files {
        let outcome =
            update_one_file(&request.root, &registry, target_component, &locked)?;
        let next_entry = match outcome {
            FileUpdate::Refresh { new_content } => {
                updated += 1;
                LockedFile {
                    path: locked.path,
                    role: locked.role,
                    source: locked.source,
                    source_sha256: sha256_hex(&new_content),
                }
            }
            FileUpdate::Conflict => {
                conflicts += 1;
                println!("conflict: {} — local edits preserved", locked.path);
                locked
            }
        };
        new_locked_files.push(next_entry);
    }

    lock.components[entry_index].files = new_locked_files;
    // Always advance `version` to the registry's current rev. Per-file
    // divergence is tracked at the file level via `source_sha256` (we leave
    // a conflicted file's hash pinned to its install-time baseline) — the
    // component-level version is just "what upstream rev are we tracking",
    // and the user's intent in running update is to track the new one.
    lock.components[entry_index].version = target_component.version.clone();
    lock.components[entry_index].target = target_component.target.clone();
    lock.components[entry_index].registry = registry.id.clone();

    lock.write(&request.root, LOCK_FILE)?;

    println!(
        "{}: updated {updated}, {conflicts} conflicts",
        request.id
    );
    Ok(())
}

enum FileUpdate {
    Refresh { new_content: String },
    Conflict,
}

fn update_one_file(
    root: &Path,
    registry: &Registry,
    component: &RegistryComponent,
    locked: &LockedFile,
) -> Result<FileUpdate, String> {
    // Locate the upstream source for this locked file. We match on the
    // registry's `source` path — that's the field the lock copied at install
    // time. If the upstream renamed or dropped the file, surface a hard
    // error rather than silently lose track.
    let registry_file = component
        .files
        .iter()
        .find(|file| file.source == locked.source)
        .ok_or_else(|| {
            format!(
                "registry no longer ships `{}` for component `{}`",
                locked.source, component.id
            )
        })?;

    let target_relative = safe_relative(&registry_file.target)?;
    let on_disk_path = root.join(target_relative);

    // If the file is missing from disk, treat it as a conflict: the user
    // chose to delete it, and we should not resurrect it without their say.
    let current = match fs::read_to_string(&on_disk_path) {
        Ok(content) => content,
        Err(_) => return Ok(FileUpdate::Conflict),
    };

    if sha256_hex(&current) != locked.source_sha256 {
        return Ok(FileUpdate::Conflict);
    }

    let source = safe_relative(&registry_file.source)?;
    let new_content = registry.read_source(source)?;

    if let Some(parent) = on_disk_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(&on_disk_path, new_content.as_bytes())
        .map_err(|e| format!("{}: {e}", on_disk_path.display()))?;

    Ok(FileUpdate::Refresh { new_content })
}

struct UpdateRequest {
    id: String,
    root: PathBuf,
    registry_path: Option<PathBuf>,
}

impl UpdateRequest {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut id: Option<String> = None;
        let mut root = PathBuf::from(".");
        let mut registry_path: Option<PathBuf> = None;
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
            id: id.ok_or_else(|| format!("usage: {UPDATE_USAGE}"))?,
            root,
            registry_path,
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

