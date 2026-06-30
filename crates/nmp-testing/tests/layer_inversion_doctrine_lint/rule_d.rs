use crate::support::{collect_files, crates_dir, evaluate, is_comment, read, rel, Occurrence};

/// Fine-grained baseline (tracked debt): `(file, symbol)`. The owning fix PR
/// removes each line when it deletes the symbol. Do NOT add new entries.
const RULE_D_BASELINE: &[(&str, &str)] = &[
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

    let mut occs = Vec::new();
    for file in &files {
        let content = read(file);
        for (i, raw) in content.lines().enumerate() {
            let trimmed = raw.trim_start();
            if is_comment(trimmed) {
                continue;
            }
            // `pub mod nip19` — the entity-codec module.
            if let Some(rest) = trimmed.strip_prefix("pub mod ") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if name == "nip19" {
                    occs.push(Occurrence {
                        file: rel(file),
                        key: format!("mod:{name}"),
                        line: i + 1,
                        detail: format!("`pub mod {name}` in nmp-core"),
                    });
                }
            }
            // `pub enum`/`pub struct` entity surfaces.
            for kw in ["pub enum ", "pub struct "] {
                if let Some(rest) = trimmed.strip_prefix(kw) {
                    let ident: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if is_nip19_entity_ident(&ident) {
                        occs.push(Occurrence {
                            file: rel(file),
                            key: format!("type:{ident}"),
                            line: i + 1,
                            detail: format!("NIP-19 entity surface `{ident}` in nmp-core"),
                        });
                    }
                }
            }
        }
    }

    evaluate(
        "Rule D (substrate-protocol-noun)",
        "nmp-core is substrate — it must not own NIP-19 entity codecs (nip19, Nip19Entity, \
         Nprofile/Nevent/Naddr). NIP-21 NostrUri / parse_nip10 are legitimate generic codecs.",
        RULE_D_BASELINE,
        &occs,
    );
}
