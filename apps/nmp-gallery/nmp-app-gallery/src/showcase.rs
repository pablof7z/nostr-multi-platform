//! Shared showcase references for every NmpGallery host.
//!
//! The gallery must prove component-owned reactivity with real Nostr
//! references, and every host must use the same references. This module embeds
//! `apps/nmp-gallery/showcase-references.json` as the single source of truth;
//! Rust hosts read the typed value directly, iOS reads the same JSON through a
//! C ABI accessor, and Android reads it through JNI.

use std::{ffi::c_char, sync::OnceLock};

use serde::Deserialize;

const RAW_JSON: &str = include_str!("../../showcase-references.json");
const RAW_JSON_C: &str = concat!(include_str!("../../showcase-references.json"), "\0");

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct GalleryShowcaseReferences {
    pub schema: String,
    pub profile: ShowcaseProfileReference,
    pub article: ShowcaseEventReference,
    pub note: ShowcaseEventReference,
    pub highlight: ShowcaseEventReference,
    pub relays: Vec<ShowcaseRelayReference>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ShowcaseProfileReference {
    pub pubkey_hex: String,
    pub npub: String,
    pub npub_short: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ShowcaseEventReference {
    pub uri: String,
    pub primary_id: String,
    pub kind: u32,
    pub label: String,
    pub expected_title: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ShowcaseRelayReference {
    pub url: String,
    pub role: String,
}

pub fn raw_json() -> &'static str {
    RAW_JSON
}

pub fn references() -> &'static GalleryShowcaseReferences {
    static REFERENCES: OnceLock<GalleryShowcaseReferences> = OnceLock::new();
    REFERENCES.get_or_init(|| {
        serde_json::from_str(RAW_JSON).expect("showcase-references.json must match schema")
    })
}

pub fn pubkey_hex() -> &'static str {
    &references().profile.pubkey_hex
}

pub fn npub() -> &'static str {
    &references().profile.npub
}

pub fn article_uri() -> &'static str {
    &references().article.uri
}

pub fn article_primary_id() -> &'static str {
    &references().article.primary_id
}

pub fn note_uri() -> &'static str {
    &references().note.uri
}

pub fn note_primary_id() -> &'static str {
    &references().note.primary_id
}

pub fn highlight_uri() -> &'static str {
    &references().highlight.uri
}

pub fn highlight_primary_id() -> &'static str {
    &references().highlight.primary_id
}

/// Borrowed pointer to the same JSON used by Rust hosts.
///
/// The pointer is process-static and must not be freed by the caller.
#[no_mangle]
pub extern "C" fn nmp_app_gallery_showcase_references_json() -> *const c_char {
    RAW_JSON_C.as_ptr().cast()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn shared_references_parse() {
        let refs = references();
        assert_eq!(refs.schema, "nmp.gallery.showcase-references/1");
        assert_eq!(refs.profile.pubkey_hex.len(), 64);
        assert!(refs
            .profile
            .pubkey_hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(refs.note.kind, 1);
        assert_eq!(refs.article.kind, 30023);
        assert_eq!(refs.highlight.kind, 9802);
        assert!(refs
            .relays
            .iter()
            .all(|relay| relay.url.starts_with("wss://")));
        assert!(refs
            .relays
            .iter()
            .any(|relay| relay.role.contains("indexer")));
    }

    #[test]
    fn app_hosts_do_not_copy_shared_reference_literals() {
        let refs = references();
        let mut needles: Vec<(&str, String)> = vec![
            ("profile pubkey", refs.profile.pubkey_hex.clone()),
            ("profile npub", refs.profile.npub.clone()),
            ("profile npub short", refs.profile.npub_short.clone()),
            ("article uri", refs.article.uri.clone()),
            ("article primary id", refs.article.primary_id.clone()),
            ("note uri", refs.note.uri.clone()),
            ("note primary id", refs.note.primary_id.clone()),
            ("highlight uri", refs.highlight.uri.clone()),
            ("highlight primary id", refs.highlight.primary_id.clone()),
        ];
        needles.extend(
            refs.relays
                .iter()
                .map(|relay| ("relay url", relay.url.clone())),
        );

        let gallery_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("nmp-app-gallery lives under apps/nmp-gallery");
        let source_file = gallery_root.join("showcase-references.json");
        let mut offenders = Vec::new();
        visit_source_files(gallery_root, &source_file, &needles, &mut offenders)
            .expect("scan gallery source files");

        assert!(
            offenders.is_empty(),
            "shared showcase references must only live in showcase-references.json:\n{}",
            offenders.join("\n")
        );
    }

    fn visit_source_files(
        dir: &Path,
        source_file: &Path,
        needles: &[(&str, String)],
        offenders: &mut Vec<String>,
    ) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            let file_name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                if matches!(file_name, ".git" | ".gradle" | "build" | "target") {
                    continue;
                }
                visit_source_files(&path, source_file, needles, offenders)?;
                continue;
            }
            if path == source_file {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (label, needle) in needles {
                if text.contains(needle) {
                    offenders.push(format!("{} contains {label}", path.display()));
                }
            }
        }
        Ok(())
    }
}
