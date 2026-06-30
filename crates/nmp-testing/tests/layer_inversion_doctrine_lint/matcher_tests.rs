use crate::rule_a::has_formatted_field;
use crate::rule_b_matchers::{
    rejected_relation_token, storage_aggregation_token, storage_nip10_marker_classifier,
    storage_relation_kind_classifier,
};
use crate::rule_c::{nip29_namespaces, RULE_C_NS_ALLOWLIST};
use crate::rule_d::is_nip19_entity_ident;
use crate::support::{decl_name, field_ident, scan_blocks, Lang};

#[test]
fn matchers_are_correct() {
    assert_eq!(
        field_ident("author_display:AuthorDisplay;", Lang::Fbs).as_deref(),
        Some("author_display")
    );
    assert_eq!(
        field_ident("pub content_preview: String,", Lang::Rust).as_deref(),
        Some("content_preview")
    );
    assert!(field_ident("table AuthorDisplay {", Lang::Fbs).is_none());
    assert!(field_ident("pub fn parse() -> Foo {", Lang::Rust).is_none());
    assert!(field_ident("use crate::Foo::bar;", Lang::Rust).is_none());
    assert!(field_ident("Self::Variant => x,", Lang::Fbs).is_none());

    assert_eq!(
        decl_name("pub struct TargetInteractionCounts {").as_deref(),
        Some("TargetInteractionCounts")
    );
    assert_eq!(decl_name("table RootCard {").as_deref(), Some("RootCard"));
    assert_eq!(decl_name("pub(crate) enum Foo {").as_deref(), Some("Foo"));
    assert!(decl_name("// struct Foo {").is_none());

    assert_eq!(
        rejected_relation_token("pub struct NoteRelationCounts {"),
        Some("NoteRelationCounts")
    );
    assert_eq!(
        rejected_relation_token("pub relation_counts: NoteRelationCounts,"),
        Some("NoteRelationCounts")
    );
    assert!(rejected_relation_token("// NoteRelationCounts docs").is_none());
    assert_eq!(
        storage_aggregation_token("CounterKind::Zap"),
        Some("CounterKind")
    );
    assert_eq!(
        storage_relation_kind_classifier("9735 => first_e_tag(tags),"),
        Some("kind:9735 zap classifier")
    );
    assert_eq!(
        storage_relation_kind_classifier("AND e.kind IN (1, 6, 7, 9735)"),
        Some("kind IN (1, 6, 7, 9735)")
    );
    assert!(storage_nip10_marker_classifier(
        "if marker == \"reply\" && reply_id.is_none()"
    ));
    assert!(storage_relation_kind_classifier("19735 => x,").is_none());

    assert_eq!(
        nip29_namespaces(r#"const NS: &str = "nmp.nip29.react_in_group";"#),
        vec!["react_in_group".to_string()]
    );
    assert!(!RULE_C_NS_ALLOWLIST.contains(&"react_in_group"));
    assert!(RULE_C_NS_ALLOWLIST.contains(&"publish_group_event"));

    assert!(is_nip19_entity_ident("Nip19Entity"));
    assert!(is_nip19_entity_ident("NprofileData"));
    assert!(is_nip19_entity_ident("NeventData"));
    assert!(is_nip19_entity_ident("NaddrData"));
    assert!(!is_nip19_entity_ident("NostrUri"));
    assert!(!is_nip19_entity_ident("Nip21Error"));

    assert!(has_formatted_field("pub formatted_amount: String,"));
    assert!(!has_formatted_field("pub amount: String,"));

    let src = "pub struct Agg {\n    pub replies: u64,\n    pub zaps: u64,\n}\nfn f() {\n    let x = Agg { replies: 1, zaps: 2 };\n}\n";
    let scan = scan_blocks(src);
    let agg = scan
        .blocks
        .iter()
        .find(|b| b.name == "Agg")
        .expect("Agg block");
    let mut nouns = std::collections::BTreeSet::new();
    for lc in &scan.lines {
        if lc.block == Some(agg.id) {
            if let Some(f) = field_ident(lc.text.trim_start(), Lang::Rust) {
                if ["replies", "reactions", "reposts", "zaps", "comments"].contains(&f.as_str()) {
                    nouns.insert(f);
                }
            }
        }
    }
    assert_eq!(nouns.len(), 2, "struct def must count both noun fields");
    let lit_line = scan
        .lines
        .iter()
        .find(|lc| lc.text.contains("Agg { replies"))
        .expect("literal line");
    assert_eq!(lit_line.block, None);
}
