use crate::support::{collect_files, crates_dir, is_comment, read, rel};

/// Baseline (tracked debt). The owning fix PR removes its line when it lands.
/// Do NOT add new entries.
const RULE_D_BASELINE: &[&str] = &[
    // #2515 — NIP-19 entity codecs in the substrate kernel.
    "crates/nmp-core/src/lib.rs",   // pub mod nip19
    "crates/nmp-core/src/nip19.rs", // Nip19Entity / Nprofile/Nevent/Naddr surfaces
];

/// `true` if `ident` (a declared type/module name) names a NIP-19 entity
/// codec. NIP-21 `NostrUri` and `parse_nip10` are legitimate generic codecs
/// and are NOT matched.
pub(crate) fn is_nip19_entity_ident(ident: &str) -> bool {
    let l = ident.to_ascii_lowercase();
    l.contains("nip19") || l.contains("nprofile") || l.contains("nevent") || l.contains("naddr")
}

#[test]
fn rule_d_nmp_core_names_no_nip19_entity() {
    let core_src = crates_dir().join("nmp-core").join("src");
    let mut files = Vec::new();
    collect_files(&core_src, &["rs"], &mut files);
    assert!(
        !files.is_empty(),
        "Rule D scanned zero files — gate would be vacuous"
    );

    let mut violations = Vec::new();
    for file in &files {
        let content = read(file);
        let baselined = RULE_D_BASELINE.contains(&rel(file).as_str());
        for (i, raw) in content.lines().enumerate() {
            let trimmed = raw.trim_start();
            if is_comment(trimmed) {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("pub mod ") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if name == "nip19" && !baselined {
                    violations.push(format!(
                        "{}:{}: Rule D (substrate-protocol-noun) — `pub mod {}` in nmp-core",
                        rel(file),
                        i + 1,
                        name
                    ));
                }
            }
            for kw in ["pub enum ", "pub struct "] {
                if let Some(rest) = trimmed.strip_prefix(kw) {
                    let ident: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if is_nip19_entity_ident(&ident) && !baselined {
                        violations.push(format!(
                            "{}:{}: Rule D (substrate-protocol-noun) — NIP-19 entity surface `{}` in nmp-core",
                            rel(file),
                            i + 1,
                            ident
                        ));
                    }
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Rule D: nmp-core is substrate — it must not own NIP-19 entity codecs (nip19, \
         Nip19Entity, Nprofile/Nevent/Naddr). NIP-21 NostrUri / parse_nip10 are legitimate \
         generic codecs. New violation(s) — fix, do NOT baseline:\n{}",
        violations.join("\n")
    );
}
