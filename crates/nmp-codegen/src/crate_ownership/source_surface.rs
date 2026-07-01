use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};

use super::{OwnershipAuditIssue, OwnershipDescriptor};

const RAW_FRAMEWORK_PROJECTION: &str = "NMP-SURFACE-RAW-PROJECTION";
const APP_DYNAMIC_FRAMEWORK_PREFIX: &str = "NMP-SURFACE-DYNAMIC-FRAMEWORK-PREFIX";
const UNCLAIMED_DECLARED_SURFACE: &str = "NMP-SURFACE-UNCLAIMED-DECLARATION";

pub(super) fn audit_source_surfaces(
    workspace_root: &Path,
    descriptors: &[OwnershipDescriptor],
) -> Vec<OwnershipAuditIssue> {
    let claim_ids = descriptors
        .iter()
        .flat_map(|descriptor| descriptor.claims.iter().map(|claim| claim.id.as_str()))
        .collect::<BTreeSet<_>>();
    let mut issues = Vec::new();
    for source_root in ["crates", "apps"] {
        let root = workspace_root.join(source_root);
        if root.exists() {
            scan_dir(workspace_root, &root, &claim_ids, &mut issues);
        }
    }
    issues
}

fn scan_dir(
    workspace_root: &Path,
    dir: &Path,
    claim_ids: &BTreeSet<&str>,
    issues: &mut Vec<OwnershipAuditIssue>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if should_skip_path(workspace_root, &path) {
            continue;
        }
        if path.is_dir() {
            scan_dir(workspace_root, &path, claim_ids, issues);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            scan_file(workspace_root, &path, claim_ids, issues);
        }
    }
}

fn scan_file(
    workspace_root: &Path,
    path: &Path,
    claim_ids: &BTreeSet<&str>,
    issues: &mut Vec<OwnershipAuditIssue>,
) {
    let Ok(source) = fs::read_to_string(path) else {
        return;
    };
    let lines = source.lines().collect::<Vec<_>>();
    for (idx, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]" {
            break;
        }
        if line.contains("register_typed_snapshot_projection") {
            let first_arg = first_argument(&lines, idx, 6);
            if contains_framework_literal(&first_arg)
                && !contains_declared_projection_token(&first_arg)
            {
                issues.push(issue(
                    RAW_FRAMEWORK_PROJECTION,
                    workspace_root,
                    path,
                    idx + 1,
                    "raw `nmp.*` framework projection registration bypasses declared projection tokens",
                ));
            }
        }

        if line.contains("ProjectionKey::app_owned(")
            || line.contains("DynamicProjectionKey::app_owned(")
        {
            let window = window(&lines, idx, 3);
            if contains_framework_literal(&window) {
                issues.push(issue(
                    APP_DYNAMIC_FRAMEWORK_PREFIX,
                    workspace_root,
                    path,
                    idx + 1,
                    "`nmp.*` framework projection key used through the app-owned dynamic path",
                ));
            }
        }

        if line.contains("DeclaredProjectionKey::framework(")
            || line.contains("FrameworkProjectionKey::declared(")
            || line.contains("DeclaredActionNamespace::framework(")
            || line.contains("DeclaredSchemaId::framework(")
        {
            let window = window(&lines, idx, 6);
            if let Some(claim) = declared_claim(&window) {
                if !claim_ids.contains(claim) {
                    issues.push(issue(
                        UNCLAIMED_DECLARED_SURFACE,
                        workspace_root,
                        path,
                        idx + 1,
                        &format!("declared framework surface cites unowned claim `{claim}`"),
                    ));
                }
            } else {
                issues.push(issue(
                    UNCLAIMED_DECLARED_SURFACE,
                    workspace_root,
                    path,
                    idx + 1,
                    "declared framework surface is missing an ownership claim literal",
                ));
            }
        }
    }
}

fn window(lines: &[&str], start: usize, len: usize) -> String {
    lines
        .iter()
        .skip(start)
        .take(len)
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_argument(lines: &[&str], start: usize, len: usize) -> String {
    let mut out = String::new();
    for line in lines.iter().skip(start).take(len) {
        if !out.is_empty() {
            out.push('\n');
        }
        if let Some(comma) = line.find(',') {
            out.push_str(&line[..comma]);
            break;
        }
        out.push_str(line);
    }
    out
}

fn contains_framework_literal(source: &str) -> bool {
    string_literals(source)
        .iter()
        .any(|literal| literal.starts_with("nmp."))
}

fn contains_declared_projection_token(source: &str) -> bool {
    source.contains("DeclaredProjectionKey::framework(")
        || source.contains("FrameworkProjectionKey::declared(")
        || source.contains("ProjectionRegistrationKey::")
}

fn declared_claim(source: &str) -> Option<&str> {
    string_literals(source).into_iter().find(|literal| {
        literal.starts_with("action.")
            || literal.starts_with("projection.")
            || literal.starts_with("schema.")
    })
}

fn string_literals(source: &str) -> Vec<&str> {
    let mut literals = Vec::new();
    let bytes = source.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'"' {
            cursor += 1;
            continue;
        }
        let start = cursor + 1;
        cursor = start;
        let mut escaped = false;
        while cursor < bytes.len() {
            match (bytes[cursor], escaped) {
                (b'\\', false) => escaped = true,
                (b'"', false) => break,
                _ => escaped = false,
            }
            cursor += 1;
        }
        if cursor < bytes.len() && bytes[cursor] == b'"' {
            if let Some(literal) = source.get(start..cursor) {
                literals.push(literal);
            }
        }
        cursor += 1;
    }
    literals
}

fn issue(
    code: &str,
    workspace_root: &Path,
    path: &Path,
    line: usize,
    message: &str,
) -> OwnershipAuditIssue {
    OwnershipAuditIssue {
        code: code.to_string(),
        message: format!("{}:{line}: {message}", display_path(workspace_root, path)),
    }
}

fn display_path(workspace_root: &Path, path: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn should_skip_path(workspace_root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(workspace_root).unwrap_or(path);
    if has_component(rel, "target")
        || has_component(rel, "generated")
        || has_component(rel, "fixtures")
    {
        return true;
    }
    let text = rel.to_string_lossy();
    text.starts_with("crates/nmp-codegen/")
        || text.starts_with("crates/nmp-testing/bin/doctrine-lint/")
        || text.contains("/tests/")
        || text.ends_with("_tests.rs")
}

fn has_component(path: &Path, needle: &str) -> bool {
    path.components()
        .any(|component| matches!(component, Component::Normal(value) if value == needle))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::super::{OwnershipClaim, OwnershipDescriptor};
    use super::*;

    #[test]
    fn crate_ownership_audit_reports_raw_nmp_feed_home_projection_registration() {
        let root = fixture_root("raw_projection");
        write_source(
            &root,
            "crates/example/src/lib.rs",
            r#"
pub fn register(app: &mut App) {
    app.register_typed_snapshot_projection("nmp.feed.home", || None);
}
"#,
        );

        let issues = audit_source_surfaces(&root, &[]);

        assert!(
            issues.iter().any(|issue| {
                issue.code == RAW_FRAMEWORK_PROJECTION
                    && issue.message.contains("crates/example/src/lib.rs:3")
                    && issue.message.contains("declared projection tokens")
            }),
            "expected raw projection issue, got {issues:?}"
        );
    }

    #[test]
    fn crate_ownership_audit_accepts_dynamic_app_projection_keys() {
        let root = fixture_root("dynamic_app_projection");
        write_source(
            &root,
            "crates/example/src/lib.rs",
            r#"
pub fn register(key: String) {
    let key = DynamicProjectionKey::app_owned(key).unwrap();
    app.register_typed_snapshot_projection(key, || None);
}
"#,
        );

        let issues = audit_source_surfaces(&root, &[]);

        assert!(issues.is_empty(), "unexpected audit issues: {issues:?}");
    }

    #[test]
    fn crate_ownership_audit_rejects_declared_framework_surface_without_claim() {
        let root = fixture_root("unclaimed_surface");
        write_source(
            &root,
            "crates/example/src/lib.rs",
            r#"
const KEY: DeclaredProjectionKey =
    DeclaredProjectionKey::framework("nmp.feed.home", "projection.nmp.feed.home");
"#,
        );

        let issues = audit_source_surfaces(&root, &[]);

        assert!(
            issues
                .iter()
                .any(|issue| issue.code == UNCLAIMED_DECLARED_SURFACE),
            "expected unclaimed declaration issue, got {issues:?}"
        );
    }

    #[test]
    fn crate_ownership_audit_accepts_declared_framework_surface_with_claim() {
        let root = fixture_root("claimed_surface");
        write_source(
            &root,
            "crates/example/src/lib.rs",
            r#"
const KEY: DeclaredProjectionKey =
    DeclaredProjectionKey::framework("nmp.follow_list", "projection.nmp.follow_list");
"#,
        );
        let descriptors = vec![descriptor(vec![claim("projection.nmp.follow_list")])];

        let issues = audit_source_surfaces(&root, &descriptors);

        assert!(issues.is_empty(), "unexpected audit issues: {issues:?}");
    }

    fn fixture_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after UNIX_EPOCH")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("nmp-{name}-{unique}"));
        fs::create_dir_all(&root).expect("create temp fixture root");
        root
    }

    fn write_source(root: &Path, relative: &str, source: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("source parent")).expect("create source parent");
        fs::write(path, source).expect("write fixture source");
    }

    fn descriptor(claims: Vec<OwnershipClaim>) -> OwnershipDescriptor {
        OwnershipDescriptor {
            owner_id: "nmp.example".to_string(),
            crate_name: "nmp-example".to_string(),
            summary: "summary".to_string(),
            claims,
            notes: Vec::new(),
            source_path: PathBuf::new(),
        }
    }

    fn claim(id: &str) -> OwnershipClaim {
        OwnershipClaim {
            claim_type: "namespace".to_string(),
            id: id.to_string(),
            exclusive: true,
            scope_kind: "projection".to_string(),
            scope_value: "nmp.follow_list".to_string(),
            context: String::new(),
            owns: vec!["test ownership".to_string()],
        }
    }
}
