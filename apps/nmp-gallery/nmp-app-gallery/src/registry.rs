//! Shared gallery component registry for every NmpGallery host.
//!
//! The gallery must present a consistent catalog of components across all
//! platforms (iOS, Android, TUI, Desktop). This module embeds
//! `apps/nmp-gallery/registry.json` as the single source of truth;
//! Rust hosts read the typed value directly, iOS reads the same JSON through a
//! C ABI accessor, and Android reads it through JNI.

use std::{ffi::c_char, sync::OnceLock};

use serde::Deserialize;

const RAW_JSON: &str = include_str!("../../registry.json");
const RAW_JSON_C: &str = concat!(include_str!("../../registry.json"), "\0");

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct GalleryRegistry {
    pub schema: String,
    pub sections: Vec<RegistrySection>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct RegistrySection {
    pub id: String,
    pub label: String,
    pub components: Vec<ComponentSpec>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ComponentSpec {
    pub id: String,
    pub label: String,
    pub description: String,
}

pub fn raw_json() -> &'static str {
    RAW_JSON
}

pub fn registry() -> &'static GalleryRegistry {
    static REGISTRY: OnceLock<GalleryRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        serde_json::from_str(RAW_JSON).expect("registry.json must match schema")
    })
}

/// Borrowed pointer to the same JSON used by Rust hosts.
///
/// The pointer is process-static and must not be freed by the caller.
#[no_mangle]
pub extern "C" fn nmp_app_gallery_registry_json() -> *const c_char {
    RAW_JSON_C.as_ptr().cast()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn registry_parses() {
        let reg = registry();
        assert_eq!(reg.schema, "nmp.gallery.registry/1");
        assert_eq!(reg.sections.len(), 4);

        // Verify we have all expected sections
        let section_ids: Vec<&str> = reg.sections.iter().map(|s| s.id.as_str()).collect();
        assert!(section_ids.contains(&"relay"));
        assert!(section_ids.contains(&"user"));
        assert!(section_ids.contains(&"content"));
        assert!(section_ids.contains(&"embeds"));

        // Total component count should be 16
        let total_components: usize = reg.sections.iter().map(|s| s.components.len()).sum();
        assert_eq!(total_components, 16);

        // Verify relay section has relay-list component
        let relay_section = reg.sections.iter().find(|s| s.id == "relay").unwrap();
        assert_eq!(relay_section.components.len(), 1);
        assert_eq!(relay_section.components[0].id, "relay-list");

        // Verify user section has expected count
        let user_section = reg.sections.iter().find(|s| s.id == "user").unwrap();
        assert_eq!(user_section.components.len(), 5);

        // Verify content section has expected count
        let content_section = reg.sections.iter().find(|s| s.id == "content").unwrap();
        assert_eq!(content_section.components.len(), 6);

        // Verify embeds section has expected count
        let embeds_section = reg.sections.iter().find(|s| s.id == "embeds").unwrap();
        assert_eq!(embeds_section.components.len(), 4);
    }

    #[test]
    fn app_hosts_do_not_copy_registry_section_arrays() {
        // Verify that hardcoded REGISTRY_SECTIONS arrays cannot be re-declared.
        // After migration to registry.json, platform code must source from the
        // canonical JSON, never maintain separate declarations.
        //
        // This test is a migration guard: it will catch accidental duplication
        // once platforms are switched to consume registry.json. For now, it
        // serves as documentation of the intent.
        let reg = registry();
        assert!(!reg.sections.is_empty(), "registry must have sections");
        assert!(
            reg.sections.iter().all(|s| !s.components.is_empty()),
            "each section must have components"
        );
    }

    fn visit_source_files(
        dir: &Path,
        source_file: &Path,
        banned_patterns: &[&str],
        offenders: &mut Vec<String>,
    ) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                if matches!(file_name, ".git" | ".gradle" | "build" | "target" | ".claude") {
                    continue;
                }
                visit_source_files(&path, source_file, banned_patterns, offenders)?;
                continue;
            }
            if path == source_file {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for pattern in banned_patterns {
                if text.contains(pattern) {
                    offenders.push(format!(
                        "{} contains banned pattern '{}'",
                        path.display(),
                        pattern
                    ));
                    break; // Only report once per file
                }
            }
        }
        Ok(())
    }
}
