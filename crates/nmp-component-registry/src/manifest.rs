//! Shared loader for the split component-registry manifest.
//!
//! The catalog is split by platform target to honor the AGENTS.md 500-LOC
//! file-size ceiling: `registry.toml` carries `schema_version` +
//! `registry_id`; each `registry.<target>.toml` carries ONLY `[[components]]`
//! blocks, so plain string concatenation yields one valid TOML document.
//! Both the component installer and the jsrepo exporter merge through here —
//! one mechanism, no drift.

use std::fs;
use std::path::Path;

/// Per-target manifest sections merged after `registry.toml`, in catalog
/// order. Adding a new platform target means adding its file here AND in the
/// builtin `include_str!` list in `registry.rs`.
pub const REGISTRY_SECTION_FILES: &[&str] = &[
    "registry.swiftui.toml",
    "registry.compose.toml",
    "registry.tui.toml",
    "registry.desktop.toml",
    "registry.web.toml",
];

/// Read `registry.toml` at `manifest_path` and append every per-target
/// section file present in the same directory.
///
/// Section files are optional: test-fixture registries (and any external
/// `--registry` dir) may ship a single self-contained `registry.toml`.
pub fn read_manifest_with_sections(manifest_path: &Path) -> Result<String, String> {
    let mut content = fs::read_to_string(manifest_path)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;
    let dir = manifest_path.parent().unwrap_or(Path::new("."));
    for name in REGISTRY_SECTION_FILES {
        let section = dir.join(name);
        if section.exists() {
            let s =
                fs::read_to_string(&section).map_err(|e| format!("{}: {e}", section.display()))?;
            content.push('\n');
            content.push_str(&s);
        }
    }
    Ok(content)
}
