//! `nmp export jsrepo [--output DIR]` — emit a jsrepo/shadcn-compatible
//! registry from the NMP component manifest.
//!
//! The output directory receives:
//!   - `registry.json`        — full index (`/registry.json`)
//!   - `r/<slug>.json`        — per-item files (`/r/<slug>.json`)
//!
//! The slug is the component id with `/` replaced by `-`
//! (e.g. `swiftui/content-core` → `swiftui-content-core`).
//!
//! Files include `content` inline (jsrepo supports this for self-hosted
//! registries) so consumers get one-shot downloads without separate file
//! requests.

use crate::registry::Registry;
use serde::Serialize;
use std::fs;
use std::path::{Component as PathComponent, Path, PathBuf};

// ---------------------------------------------------------------------------
// jsrepo output types
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct JsrepoRegistry {
    #[serde(rename = "$schema")]
    schema: &'static str,
    name: &'static str,
    homepage: &'static str,
    items: Vec<JsrepoItem>,
}

#[derive(Serialize, Clone)]
struct JsrepoItem {
    name: String,
    #[serde(rename = "type")]
    item_type: &'static str,
    title: String,
    description: String,
    dependencies: Vec<String>,
    #[serde(rename = "registryDependencies")]
    registry_dependencies: Vec<String>,
    files: Vec<JsrepoFile>,
}

#[derive(Serialize, Clone)]
struct JsrepoFile {
    /// Source path prefixed with `registry/` (the registry sub-tree root
    /// relative to the web host). Matches the example in the task spec which
    /// shows `"registry/swiftui/content-core/NostrContentRenderer.swift"`.
    path: String,
    #[serde(rename = "type")]
    file_type: &'static str,
    target: String,
    content: String,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

const EXPORT_USAGE: &str = "nmp export jsrepo [--output DIR] [--registry DIR]";

/// Convert a component id to a jsrepo slug: `swiftui/content-core` →
/// `swiftui-content-core`.
pub fn id_to_slug(id: &str) -> String {
    id.replace('/', "-")
}

pub fn run(args: &[String]) -> Result<(), String> {
    let (output_dir, registry_path) = parse_args(args)?;

    let registry = Registry::load(registry_path)?;
    let items = build_items(&registry)?;

    let registry = JsrepoRegistry {
        schema: "https://ui.shadcn.com/schema/registry.json",
        name: "nmp",
        homepage: "https://nmpui.f7z.io",
        items: items.clone(),
    };

    let json =
        serde_json::to_string_pretty(&registry).map_err(|e| format!("JSON serialisation: {e}"))?;

    fs::create_dir_all(&output_dir).map_err(|e| format!("{}: {e}", output_dir.display()))?;
    let index_path = output_dir.join("registry.json");
    write_file(&index_path, &json)?;
    println!("wrote {}", index_path.display());

    // Per-item files under `r/<slug>.json`.
    let r_dir = output_dir.join("r");
    fs::create_dir_all(&r_dir).map_err(|e| format!("{}: {e}", r_dir.display()))?;
    for item in &items {
        let item_json = serde_json::to_string_pretty(item)
            .map_err(|e| format!("JSON serialisation for {}: {e}", item.name))?;
        let item_path = r_dir.join(format!("{}.json", item.name));
        write_file(&item_path, &item_json)?;
        println!("wrote {}", item_path.display());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

fn parse_args(args: &[String]) -> Result<(PathBuf, Option<PathBuf>), String> {
    let mut output_dir = PathBuf::from(".");
    let mut registry_path: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--output" => {
                index += 1;
                output_dir = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--output requires a directory".to_string())?;
            }
            "--registry" => {
                index += 1;
                registry_path = Some(
                    args.get(index)
                        .map(PathBuf::from)
                        .ok_or_else(|| "--registry requires a directory".to_string())?,
                );
            }
            flag if flag.starts_with('-') => {
                return Err(format!("unknown argument {flag}\nusage: {EXPORT_USAGE}"))
            }
            // positional args not expected
            other => {
                return Err(format!(
                    "unexpected argument `{other}`\nusage: {EXPORT_USAGE}"
                ))
            }
        }
        index += 1;
    }
    Ok((output_dir, registry_path))
}

fn build_items(registry: &Registry) -> Result<Vec<JsrepoItem>, String> {
    let mut items = Vec::with_capacity(registry.components().len());
    for component in registry.components() {
        let slug = id_to_slug(&component.id);
        // Convert dep ids to slugs for registryDependencies.
        let registry_deps = component
            .dependencies
            .iter()
            .map(|dep| id_to_slug(dep))
            .collect();

        let mut files = Vec::with_capacity(component.files.len());
        for file in &component.files {
            let source_rel = safe_relative(&file.source)?;
            let content = registry.read_source(source_rel)?;

            // `path` uses the `registry/` prefix so it matches the URL path
            // that the self-hosted website serves under `/registry/<source>`.
            files.push(JsrepoFile {
                path: format!("registry/{}", file.source),
                file_type: "registry:ui",
                target: file.target.clone(),
                content,
            });
        }

        items.push(JsrepoItem {
            name: slug,
            item_type: "registry:ui",
            title: title_from_id(&component.id),
            description: component.description.clone(),
            dependencies: vec![],
            registry_dependencies: registry_deps,
            files,
        });
    }
    Ok(items)
}

/// Derive a human title from a component id.
/// `swiftui/content-core` → `Content Core (SwiftUI)`
fn title_from_id(id: &str) -> String {
    let (platform, name) = id.split_once('/').unwrap_or(("", id));
    let name_titled = name
        .split('-')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let platform_label = match platform {
        "swiftui" => "SwiftUI",
        "compose" => "Compose",
        _ => platform,
    };
    if platform.is_empty() {
        name_titled
    } else {
        format!("{name_titled} ({platform_label})")
    }
}

fn safe_relative(path: &str) -> Result<&Path, String> {
    let p = Path::new(path);
    let valid = p
        .components()
        .all(|part| matches!(part, PathComponent::Normal(_)));
    if p.as_os_str().is_empty() || p.is_absolute() || !valid {
        return Err(format!("invalid relative path `{}`", p.display()));
    }
    Ok(p)
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    // Always write with a trailing newline for clean diffs.
    let mut out = content.to_owned();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}
