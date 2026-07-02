//! Deprecated compatibility attribute ratchet (#2770).
//!
//! Owner rule: deprecated compatibility surfaces are removed, not carried with
//! a warning label. Generated bindings are outside this rule's scope; production
//! workspace source must not introduce Rust deprecated attributes.

use std::path::Path;

pub const ID: &str = "no_deprecated";

pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.ends_with("_generated.rs") || s.contains("/generated/") {
        return false;
    }
    if s.contains("/fixtures/") {
        return false;
    }
    ((s.contains("/crates/") || s.starts_with("crates/")) && s.contains("/src/"))
        || ((s.contains("/apps/") || s.starts_with("apps/")) && s.contains("/src/"))
}

pub fn check(line: &str, is_comment: bool) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let Some(pos) = line.find(&deprecated_prefix()) else {
        return Vec::new();
    };
    vec![(
        pos + 1,
        format!(
            "`{}` is banned: deprecated compatibility surfaces are removed, not blessed (#2770)",
            deprecated_attr_display()
        ),
        "delete the obsolete surface and update in-repo callers; do not add aliases, shims, or deprecation periods"
            .to_string(),
    )]
}

fn deprecated_prefix() -> String {
    ["#[", "deprecated"].concat()
}

fn deprecated_attr_display() -> String {
    ["#[", "deprecated]"].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_deprecated_attributes() {
        let line = ["#[", "deprecated(note = \"use new_api\")]"].concat();
        let hits = check(&line, false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains(&deprecated_attr_display()));
    }

    #[test]
    fn ignores_comments() {
        let line = ["// #[", "deprecated(note = \"old\")]"].concat();
        assert!(check(&line, true).is_empty());
    }

    #[test]
    fn scope_is_workspace_source_not_generated_bindings() {
        assert!(file_in_scope(Path::new(
            "crates/nmp-native-runtime/src/lib.rs"
        )));
        assert!(file_in_scope(Path::new("apps/example/src/lib.rs")));
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/transport/generated/nmp_update_generated.rs"
        )));
        assert!(!file_in_scope(Path::new(
            "crates/nmp-uniffi/generated/swift/nmp_uniffi.swift"
        )));
    }
}
