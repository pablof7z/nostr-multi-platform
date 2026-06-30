use crate::support::{collect_files, crates_dir, is_comment, read, rel};

/// Baseline (tracked debt). The owning fix PR removes its line when it lands.
/// Do NOT add new entries.
const RULE_C_BASELINE: &[&str] = &[
    // #2513 — kind-specific react/repost/share verbs in the kind-blind transport.
    "crates/nmp-nip29/src/action/composed.rs", // react_in_group / unreact_in_group + REACTION_KIND
    "crates/nmp-nip29/src/action/group_event.rs", // share_event_in_group / repost_in_group + REPOST_KIND
    "crates/nmp-nip29/src/wire/action_payload/group.rs", // react/unreact payload namespaces
    "crates/nmp-nip29/src/wire/action_payload/group_event.rs", // share/repost payload namespaces
    "crates/nmp-nip29/schema/react_in_group_action.fbs",
    "crates/nmp-nip29/schema/unreact_in_group_action.fbs",
    "crates/nmp-nip29/schema/repost_in_group_action.fbs",
    "crates/nmp-nip29/schema/share_event_in_group_action.fbs",
];

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

    let mut violations = Vec::new();
    for file in &files {
        let content = read(file);
        let baselined = RULE_C_BASELINE.contains(&rel(file).as_str());
        for (i, raw) in content.lines().enumerate() {
            let trimmed = raw.trim_start();
            if is_comment(trimmed) {
                continue;
            }
            for ns in nip29_namespaces(raw) {
                if !RULE_C_NS_ALLOWLIST.contains(&ns.as_str()) && !baselined {
                    violations.push(format!(
                        "{}:{}: Rule C (kind-blind-transport) — kind-specific action namespace `nmp.nip29.{}`",
                        rel(file),
                        i + 1,
                        ns
                    ));
                }
            }
            if (trimmed.contains("REACTION_KIND") || trimmed.contains("REPOST_KIND"))
                && trimmed.contains("const ")
                && !baselined
            {
                violations.push(format!(
                    "{}:{}: Rule C (kind-blind-transport) — kind constant in transport: {}",
                    rel(file),
                    i + 1,
                    trimmed
                ));
            }
        }
    }

    for file in &schema_files {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let kind_specific =
            name.contains("react") || name.contains("repost") || name.contains("share_event");
        if kind_specific && !RULE_C_BASELINE.contains(&rel(file).as_str()) {
            violations.push(format!(
                "{}:1: Rule C (kind-blind-transport) — kind-specific schema file `{}`",
                rel(file),
                name
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "Rule C: nmp-nip29 is kind-blind h-tag transport — it owns ONE generic publish \
         verb plus pure envelope ops, never kind-specific react/repost/share verbs or \
         kind constants. New violation(s) — fix, do NOT baseline:\n{}",
        violations.join("\n")
    );
}
