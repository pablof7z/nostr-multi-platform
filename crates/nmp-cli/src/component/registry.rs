use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const BUILTIN_REGISTRY: &str = include_str!("../../registry/registry.toml");
const BUILTIN_FILES: &[(&str, &str)] = &[
    (
        "swiftui/content-core/NostrContentRenderer.swift",
        include_str!("../../registry/swiftui/content-core/NostrContentRenderer.swift"),
    ),
    (
        "swiftui/content-minimal/NostrMinimalContentView.swift",
        include_str!("../../registry/swiftui/content-minimal/NostrMinimalContentView.swift"),
    ),
    (
        "swiftui/content-minimal/Examples/NostrMinimalContentPreview.swift",
        include_str!(
            "../../registry/swiftui/content-minimal/Examples/NostrMinimalContentPreview.swift"
        ),
    ),
];

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
                let content = fs::read_to_string(&manifest)
                    .map_err(|e| format!("{}: {e}", manifest.display()))?;
                (content, RegistryRoot::Filesystem(root))
            }
            None => (BUILTIN_REGISTRY.to_string(), RegistryRoot::Builtin),
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
