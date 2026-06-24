use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

// The catalog is split by platform target (file-size rule); merge order must
// match REGISTRY_SECTION_FILES in crate::registry_manifest.
const BUILTIN_REGISTRY_SECTIONS: &[&str] = &[
    include_str!("../../registry/registry.toml"),
    include_str!("../../registry/registry.swiftui.toml"),
    include_str!("../../registry/registry.compose.toml"),
    include_str!("../../registry/registry.tui.toml"),
    include_str!("../../registry/registry.desktop.toml"),
    include_str!("../../registry/registry.web.toml"),
];
#[path = "registry/builtin_files.rs"]
mod builtin_files;
use builtin_files::BUILTIN_FILES;

#[derive(Deserialize)]
struct RegistryManifest {
    registry_id: String,
    components: Vec<RegistryComponent>,
}

#[derive(Deserialize)]
pub(super) struct RegistryComponent {
    pub(super) id: String,
    pub(super) version: String,
    pub(super) target: String,
    #[serde(default)]
    dependencies: Vec<String>,
    pub(super) files: Vec<RegistryFile>,
}

#[derive(Deserialize)]
pub(super) struct RegistryFile {
    pub(super) source: String,
    pub(super) target: String,
    pub(super) role: String,
}

pub(super) struct Registry {
    pub(super) id: String,
    root: RegistryRoot,
    components: Vec<RegistryComponent>,
}

enum RegistryRoot {
    Builtin,
    Filesystem(PathBuf),
}

impl Registry {
    pub(super) fn load(path: Option<PathBuf>) -> Result<Self, String> {
        let (manifest, root) = match path {
            Some(path) => {
                let manifest = if path.is_dir() {
                    path.join("registry.toml")
                } else {
                    path.clone()
                };
                let root = manifest.parent().unwrap_or(Path::new(".")).to_path_buf();
                let content = crate::registry_manifest::read_manifest_with_sections(&manifest)?;
                (content, RegistryRoot::Filesystem(root))
            }
            None => (BUILTIN_REGISTRY_SECTIONS.join("\n"), RegistryRoot::Builtin),
        };
        let parsed = toml::from_str::<RegistryManifest>(&manifest)
            .map_err(|e| format!("invalid component registry: {e}"))?;
        Ok(Self {
            id: parsed.registry_id,
            root,
            components: parsed.components,
        })
    }

    pub(super) fn resolve(&self, id: &str) -> Result<Vec<&RegistryComponent>, String> {
        let mut seen = HashSet::new();
        let mut order = Vec::new();
        self.collect(id, &mut seen, &mut order)?;
        Ok(order)
    }

    pub(super) fn read_source(&self, path: &Path) -> Result<String, String> {
        match &self.root {
            RegistryRoot::Builtin => BUILTIN_FILES
                .iter()
                .find(|(candidate, _)| Path::new(candidate) == path)
                .map(|(_, content)| (*content).to_string())
                .ok_or_else(|| format!("builtin component source missing: {}", path.display())),
            RegistryRoot::Filesystem(root) => fs::read_to_string(root.join(path))
                .map_err(|e| format!("{}: {e}", root.join(path).display())),
        }
    }

    fn collect<'a>(
        &'a self,
        id: &str,
        seen: &mut HashSet<String>,
        order: &mut Vec<&'a RegistryComponent>,
    ) -> Result<(), String> {
        if !seen.insert(id.to_string()) {
            return Ok(());
        }
        let component = self
            .components
            .iter()
            .find(|component| component.id == id)
            .ok_or_else(|| format!("unknown component `{id}`"))?;
        for dependency in &component.dependencies {
            self.collect(dependency, seen, order)?;
        }
        order.push(component);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression gate: every `source` file declared in the builtin manifest must
    // be embedded in BUILTIN_FILES so `read_source` resolves it. Without this,
    // manifest-vs-BUILTIN_FILES drift ships silently (a component declares a file
    // the embedded binary can't install — install fails with
    // "builtin component source missing"). This caught the unwired
    // swiftui/user-*, compose/user-*, desktop/user-*, login-block, and
    // content-kind-* sources.
    #[test]
    fn every_declared_builtin_source_resolves() {
        let registry = Registry::load(None).expect("builtin registry must load");
        let mut missing = Vec::new();
        for component in &registry.components {
            for file in &component.files {
                if file.role != "source" {
                    continue;
                }
                if registry.read_source(Path::new(&file.source)).is_err() {
                    missing.push(format!("{} ({})", file.source, component.id));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "builtin manifest declares source files not embedded in BUILTIN_FILES \
             (add include_str! entries in registry.rs):\n{}",
            missing.join("\n")
        );
    }
}
