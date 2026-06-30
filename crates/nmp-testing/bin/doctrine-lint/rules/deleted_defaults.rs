//! Deleted defaults gate.
//!
//! `nmp-defaults` was a hidden composition bundle. After deletion, production
//! and scaffold code must compose explicit owners directly. This rule bans the
//! deleted crate/module/API names and common renamed-default-preset shapes so a
//! replacement bundle cannot re-enter under a softer name.

use std::path::Path;

pub const ID: &str = "deleted_defaults";

const DELETED_DEFAULTS_TOKENS: &[&str] = &[
    "register_defaults_with_handles",
    "register_defaults_with",
    "register_defaults",
    "nmp-defaults",
    "nmp_defaults",
    "NmpDefaults",
];

const RENAMED_PRESET_TOKENS: &[&str] = &[
    "register_defaults_preset",
    "register_default_preset",
    "register_defaults_bundle",
    "register_default_bundle",
    "defaults_preset",
    "default_preset",
    "defaults_bundle",
    "default_bundle",
    "DefaultsPreset",
    "DefaultPreset",
    "DefaultsBundle",
    "DefaultBundle",
    "TestingDefaults",
    "TestDefaults",
    "testing_defaults",
    "test_defaults",
];

/// True iff deleted-defaults should scan `path`.
///
/// The rule covers production Rust sources and scaffold generator/template
/// code. It deliberately excludes doctrine-lint itself and test infrastructure;
/// fixtures exercise the rule directly.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.contains("/bin/doctrine-lint/") || s.contains("/generated/") {
        return false;
    }
    if s.contains("/crates/nmp-testing/") || s.starts_with("crates/nmp-testing/") {
        return false;
    }
    if s.contains("/crates/nmp-content-fixtures/") || s.starts_with("crates/nmp-content-fixtures/")
    {
        return false;
    }
    if s.contains("/crates/nmp-cli/src/") || s.starts_with("crates/nmp-cli/src/") {
        return true;
    }
    if s.contains("/crates/nmp-cli/templates/") || s.starts_with("crates/nmp-cli/templates/") {
        return true;
    }
    let in_crate_src = (s.contains("/crates/") || s.starts_with("crates/")) && s.contains("/src/");
    let in_app_src = (s.contains("/apps/") || s.starts_with("apps/")) && s.contains("/src/");
    in_crate_src || in_app_src
}

pub fn check(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }

    let mut hits = Vec::new();
    for token in DELETED_DEFAULTS_TOKENS {
        collect_hits(
            line,
            token,
            "`nmp-defaults` is deleted; production and scaffold code must compose explicit owners directly",
            "replace the deleted defaults call/dependency with explicit app-owned or protocol-owned composition",
            &mut hits,
        );
    }
    for token in RENAMED_PRESET_TOKENS {
        collect_hits(
            line,
            token,
            "renamed defaults presets are forbidden; deleting `nmp-defaults` must not create a replacement bundle",
            "wire the concrete feature owners explicitly instead of introducing a test helper, preset, or bundle",
            &mut hits,
        );
    }
    hits
}

fn collect_hits(
    line: &str,
    token: &str,
    message: &str,
    suggested: &str,
    hits: &mut Vec<(usize, String, String)>,
) {
    let mut start = 0;
    while let Some(rel) = line[start..].find(token) {
        let begin = start + rel + 1;
        let end = begin + token.len();
        if hits.iter().any(|(col, existing, _)| {
            let existing_token = existing
                .strip_prefix('`')
                .and_then(|rest| rest.split_once('`'))
                .map(|(name, _)| name)
                .unwrap_or("");
            let existing_begin = *col;
            let existing_end = existing_begin + existing_token.len();
            begin < existing_end && existing_begin < end
        }) {
            start += rel + token.len();
            continue;
        }
        hits.push((
            begin,
            format!("`{token}` is banned: {message}"),
            suggested.to_string(),
        ));
        start += rel + token.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_deleted_api_names() {
        let hits = check(
            "nmp_defaults::register_defaults_with_handles(&mut app);",
            false,
            false,
        );
        assert!(hits.iter().any(|hit| hit.1.contains("nmp_defaults")));
        assert!(hits
            .iter()
            .any(|hit| hit.1.contains("register_defaults_with_handles")));
    }

    #[test]
    fn flags_renamed_preset_names() {
        let hits = check("let preset = TestDefaults::new();", false, false);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("replacement bundle"));
    }

    #[test]
    fn skips_comments_and_test_cfg() {
        assert!(check("// nmp_defaults::register_defaults(app)", true, false).is_empty());
        assert!(check("let _ = TestDefaults::new();", false, true).is_empty());
    }
}
