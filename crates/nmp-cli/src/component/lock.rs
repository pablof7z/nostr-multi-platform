use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Default, Deserialize)]
pub(super) struct ComponentLock {
    #[serde(default)]
    pub(super) components: Vec<LockedComponent>,
}

#[derive(Deserialize)]
pub(super) struct LockedComponent {
    pub(super) id: String,
    pub(super) version: String,
    pub(super) registry: String,
    pub(super) target: String,
    #[serde(default)]
    pub(super) files: Vec<LockedFile>,
}

#[derive(Deserialize)]
pub(super) struct LockedFile {
    pub(super) path: String,
    pub(super) role: String,
    pub(super) source: String,
    pub(super) source_sha256: String,
}

impl ComponentLock {
    pub(super) fn read(root: &Path, lock_file: &str) -> Result<Self, String> {
        let path = root.join(lock_file);
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&content).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub(super) fn write(&self, root: &Path, lock_file: &str) -> Result<(), String> {
        let mut out = String::from("schema_version = 1\n\n");
        for component in &self.components {
            out.push_str("[[components]]\n");
            out.push_str(&format!("id = \"{}\"\n", quote(&component.id)));
            out.push_str(&format!("version = \"{}\"\n", quote(&component.version)));
            out.push_str(&format!("registry = \"{}\"\n", quote(&component.registry)));
            out.push_str(&format!("target = \"{}\"\n", quote(&component.target)));
            for file in &component.files {
                out.push_str("\n[[components.files]]\n");
                out.push_str(&format!("path = \"{}\"\n", quote(&file.path)));
                out.push_str(&format!("role = \"{}\"\n", quote(&file.role)));
                out.push_str(&format!("source = \"{}\"\n", quote(&file.source)));
                out.push_str(&format!(
                    "source_sha256 = \"{}\"\n",
                    quote(&file.source_sha256)
                ));
            }
            out.push('\n');
        }
        fs::write(root.join(lock_file), out).map_err(|e| format!("{lock_file}: {e}"))
    }
}

fn quote(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
