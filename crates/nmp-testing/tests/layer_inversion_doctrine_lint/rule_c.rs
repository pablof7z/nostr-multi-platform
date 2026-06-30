use crate::support::{collect_files, crates_dir, evaluate, is_comment, read, rel, Occurrence};

/// Fine-grained baseline (tracked debt): `(file, symbol)`. The owning fix PR
/// removes each line when it deletes the symbol. Do NOT add new entries.
///
/// EMPTY: the #2513 kind-blind-transport fix landed on master (the
/// react/unreact/repost/share verbs, their `nmp.nip29.*` namespaces, the
/// REACTION_KIND/REPOST_KIND constants, and the kind-specific `*_action.fbs`
/// schemas are all gone). Self-pruning stale detection flagged every former
/// entry, so the baseline is now empty and Rule C is a pure forward gate: any
/// reintroduced kind-specific verb fails immediately.
const RULE_C_BASELINE: &[(&str, &str)] = &[];

/// Legitimate `nmp.nip29.<suffix>` namespaces: the ONE generic publish verb,
/// the pure envelope/admin action ops (per `register.rs`), and the
/// projection/cache/wire snapshot keys. Anything else is a kind-specific verb
/// the kind-blind transport must not own. The audited debt
/// (`react`/`unreact`/`repost`/`share` verbs) is intentionally NOT here.
pub(crate) const RULE_C_NS_ALLOWLIST: &[&str] = &[
    "publish_group_event",
    "put_user",
    "create_invite",
    "create_public_group",
    "discover",
    "edit_metadata",
    "join",
    "leave",
    "set_parent",
    "group_defaults",
    "joined_groups",
    "joined_hosts",
    "tofu_signer",
    "group_roster",
    "group_events",
    "discovered_groups",
];

/// Extract every `nmp.nip29.<suffix>` namespace suffix appearing as a string
/// literal on `line`.
pub(crate) fn nip29_namespaces(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let prefix = "nmp.nip29.";
    let mut idx = 0;
    while let Some(pos) = line[idx..].find(prefix) {
        let start = idx + pos + prefix.len();
        let suffix: String = line[start..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !suffix.is_empty() {
            out.push(suffix);
        }
        idx = start;
    }
    out
}

#[test]
fn rule_c_nip29_is_kind_blind_transport() {
    let nip29 = crates_dir().join("nmp-nip29");
    let mut files = Vec::new();
    collect_files(&nip29.join("src"), &["rs"], &mut files);
    let mut schema_files = Vec::new();
    collect_files(&nip29.join("schema"), &["fbs"], &mut schema_files);
    assert!(
        !files.is_empty() && !schema_files.is_empty(),
        "Rule C scanned zero src/schema files — gate would be vacuous"
    );

    let mut occs = Vec::new();

    // src/**: banned namespace verbs + REACTION_KIND/REPOST_KIND constants.
    for file in &files {
        let content = read(file);
        for (i, raw) in content.lines().enumerate() {
            let trimmed = raw.trim_start();
            if is_comment(trimmed) {
                continue;
            }
            for ns in nip29_namespaces(raw) {
                if !RULE_C_NS_ALLOWLIST.contains(&ns.as_str()) {
                    occs.push(Occurrence {
                        file: rel(file),
                        key: format!("ns:{ns}"),
                        line: i + 1,
                        detail: format!("kind-specific action namespace `nmp.nip29.{ns}`"),
                    });
                }
            }
            for konst in ["REACTION_KIND", "REPOST_KIND"] {
                if trimmed.contains(konst) && trimmed.contains("const ") {
                    occs.push(Occurrence {
                        file: rel(file),
                        key: format!("const:{konst}"),
                        line: i + 1,
                        detail: format!("kind constant in transport: {trimmed}"),
                    });
                }
            }
        }
    }

    // schema/**: react/repost/share .fbs filenames are kind-specific verbs.
    for file in &schema_files {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let kind_specific =
            name.contains("react") || name.contains("repost") || name.contains("share_event");
        if kind_specific {
            occs.push(Occurrence {
                file: rel(file),
                key: "schema-file".to_string(),
                line: 1,
                detail: format!("kind-specific schema file `{name}`"),
            });
        }
    }

    evaluate(
        "Rule C (kind-blind-transport)",
        "nmp-nip29 is kind-blind h-tag transport — it owns ONE generic publish verb plus \
         pure envelope ops, never kind-specific react/repost/share verbs or kind constants.",
        RULE_C_BASELINE,
        &occs,
    );
}
