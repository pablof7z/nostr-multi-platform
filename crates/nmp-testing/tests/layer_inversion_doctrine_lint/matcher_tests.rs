use crate::rule_a::has_formatted_field;
use crate::rule_b_matchers::{
    rejected_relation_token, storage_aggregation_token, storage_nip10_marker_classifier,
    storage_relation_kind_classifier,
};
use crate::rule_c::{nip29_namespaces, RULE_C_NS_ALLOWLIST};
use crate::rule_d::is_nip19_entity_ident;
use crate::rule_e::{crate_layer, dep_name, manifest_runtime_deps, upward_edge};
use crate::support::{classify, decl_name, field_ident, scan_blocks, Lang, Occurrence};

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

    // --- Rule E helpers: layer map + edge direction + manifest parsing ------
    assert_eq!(crate_layer("nmp-core"), Some(3));
    assert_eq!(crate_layer("nmp-router"), Some(2));
    assert_eq!(crate_layer("nmp-nip01"), Some(4)); // default-L4 NIP crate
    assert_eq!(crate_layer("nmp-nip42-types"), Some(0)); // -types is L0 vocabulary
    assert_eq!(crate_layer("nmp-defaults"), Some(5));
    assert_eq!(crate_layer("serde"), None); // external crate is unmapped
    // Upward edges fire; downward / same-layer / unmapped do not.
    assert_eq!(upward_edge("nmp-router", "nmp-core"), Some((2, 3))); // L2 -> L3 upward
    assert_eq!(upward_edge("nmp-kinds", "nmp-core"), Some((0, 3))); // synthetic upward
    assert!(upward_edge("nmp-core", "nmp-store").is_none()); // L3 -> L1 downward
    assert!(upward_edge("nmp-nip01", "nmp-core").is_none()); // L4 -> L3 downward
    assert!(upward_edge("nmp-router", "nmp-planner").is_none()); // L2 -> L2 same layer
    assert!(upward_edge("nmp-core", "serde").is_none()); // unmapped target
    // dep_name: plain, dotted-workspace, and renamed forms.
    assert_eq!(
        dep_name(r#"nmp-core = { path = "../nmp-core" }"#).as_deref(),
        Some("nmp-core")
    );
    assert_eq!(
        dep_name("nmp-store.workspace = true").as_deref(),
        Some("nmp-store")
    );
    assert_eq!(
        dep_name(r#"alias = { package = "nmp-core", path = "../nmp-core" }"#).as_deref(),
        Some("nmp-core")
    );
    assert!(dep_name("# nmp-core is great").is_none()); // comment
    // manifest_runtime_deps: includes [dependencies]/[build-dependencies],
    // excludes [dev-dependencies].
    let toml = "[package]\nname = \"x\"\n\n[dependencies]\nnmp-core = { path = \"../nmp-core\" }\n\n[dev-dependencies]\nnmp-testing = { path = \"../nmp-testing\" }\n\n[build-dependencies]\nnmp-codegen = { path = \"../nmp-codegen\" }\n";
    let deps = manifest_runtime_deps(toml);
    assert!(deps.contains(&"nmp-core".to_string()));
    assert!(deps.contains(&"nmp-codegen".to_string()));
    assert!(
        !deps.contains(&"nmp-testing".to_string()),
        "dev-dependencies must be excluded from the layer graph"
    );

    // --- Baseline harness: fine-grained masking-resistance + stale detection -
    // (a) A NEW symbol in an already-baselined file is NOT masked: the file has
    //     a baselined `author_display`, but a fresh `author_display_name` field
    //     in the same file is a distinct key and still fires.
    let baseline: &[(&str, &str)] = &[("f.fbs", "author_display")];
    let occs = vec![
        Occurrence {
            file: "f.fbs".into(),
            key: "author_display".into(),
            line: 10,
            detail: "known".into(),
        },
        Occurrence {
            file: "f.fbs".into(),
            key: "author_display_name".into(),
            line: 11,
            detail: "new field in baselined file".into(),
        },
    ];
    let (new_v, stale) = classify("Rule X", baseline, &occs);
    assert_eq!(
        new_v.len(),
        1,
        "fine-grained baseline must NOT mask a new symbol in a baselined file"
    );
    assert!(new_v[0].contains("author_display_name"));
    assert!(stale.is_empty(), "every baseline entry is still present");

    // (b) Stale detection: a baseline entry with no live occurrence fails.
    let (new_v2, stale2) = classify("Rule X", &[("gone.rs", "OldType")], &[]);
    assert!(new_v2.is_empty(), "no occurrences => no new violations");
    assert_eq!(stale2.len(), 1, "missing occurrence => one stale entry");
    assert!(stale2[0].contains("OldType"));

    // (c) Exactly-baselined occurrence is green (no new, no stale).
    let (new_v3, stale3) = classify(
        "Rule X",
        &[("a.rs", "K")],
        &[Occurrence {
            file: "a.rs".into(),
            key: "K".into(),
            line: 1,
            detail: "d".into(),
        }],
    );
    assert!(new_v3.is_empty() && stale3.is_empty());
}
