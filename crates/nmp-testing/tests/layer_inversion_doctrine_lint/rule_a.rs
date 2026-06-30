use crate::support::{
    collect_files, crates_dir, evaluate, field_ident, lang_of, nmp_nip_crates, read, rel,
    scan_blocks, Occurrence,
};

/// Fine-grained baseline (tracked debt): `(file, field-name)`. The owning fix
/// PR removes each line when it deletes the field. Do NOT add new entries — a
/// new banned field with a different name fires even inside a file that already
/// carries a *different* baselined field (no file-level masking).
const RULE_A_BASELINE: &[(&str, &str)] = &[
    // #2510 / #2508 — op-centric timeline render cards in nmp-nip01.
    (
        "crates/nmp-nip01/schema/timeline_snapshot.fbs",
        "author_display",
    ),
    (
        "crates/nmp-nip01/schema/timeline_snapshot.fbs",
        "author_display_name",
    ),
    (
        "crates/nmp-nip01/schema/timeline_snapshot.fbs",
        "author_picture_url",
    ),
    (
        "crates/nmp-nip01/schema/timeline_snapshot.fbs",
        "content_preview",
    ),
    (
        "crates/nmp-nip01/schema/timeline_snapshot.fbs",
        "content_render",
    ),
    (
        "crates/nmp-nip01/schema/timeline_snapshot.fbs",
        "has_author_display_name",
    ),
    (
        "crates/nmp-nip01/schema/timeline_snapshot.fbs",
        "has_author_picture_url",
    ),
    (
        "crates/nmp-nip01/src/timeline_projection/render_data.rs",
        "author_display",
    ),
    (
        "crates/nmp-nip01/src/timeline_projection/render_data.rs",
        "content_preview",
    ),
    ("crates/nmp-nip01/schema/op_feed.fbs", "author_display"),
    (
        "crates/nmp-nip01/src/op_feed/attribution.rs",
        "author_display",
    ),
];

/// Banned tokens for a display/render FIELD declaration (substring match).
const RULE_A_BANNED: &[&str] = &[
    "author_display_name",
    "author_picture_url",
    "author_display",
    "AuthorDisplay",
    "content_preview",
    "content_render",
    "ContentRenderData",
];

/// `true` if `line` declares a field named `formatted_<something>`.
pub(crate) fn has_formatted_field(line: &str) -> bool {
    let mut idx = 0;
    while let Some(pos) = line[idx..].find("formatted_") {
        let start = idx + pos;
        let after = &line[start + "formatted_".len()..];
        if after
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        {
            return true;
        }
        idx = start + "formatted_".len();
    }
    false
}

#[test]
fn rule_a_no_display_enrichment_in_primitives() {
    let mut dirs: Vec<String> = vec![
        "nmp-content".to_string(),
        "nmp-feed".to_string(),
        "nmp-threading".to_string(),
    ];
    dirs.extend(nmp_nip_crates());

    let mut files = Vec::new();
    for d in &dirs {
        let crate_dir = crates_dir().join(d);
        collect_files(&crate_dir.join("src"), &["rs"], &mut files);
        collect_files(&crate_dir.join("schema"), &["fbs"], &mut files);
    }
    assert!(
        !files.is_empty(),
        "Rule A scanned zero files — gate would be vacuous"
    );

    let mut occs = Vec::new();
    for file in &files {
        let lang = lang_of(file);
        let content = read(file);
        let scan = scan_blocks(&content);
        for lc in &scan.lines {
            let trimmed = lc.text.trim_start();
            // Profile carve-out: the kind:0 ProfileProjection vocabulary owns
            // display data legitimately.
            if lc
                .def_stack
                .iter()
                .any(|n| n.to_ascii_lowercase().contains("profile"))
            {
                continue;
            }
            let Some(field) = field_ident(trimmed, lang) else {
                continue;
            };
            let hit = RULE_A_BANNED
                .iter()
                .find(|b| lc.text.contains(**b))
                .map(|b| b.to_string())
                .or_else(|| has_formatted_field(&lc.text).then(|| "formatted_*".to_string()));
            if let Some(token) = hit {
                occs.push(Occurrence {
                    file: rel(file),
                    // Fine-grained key: the field name. A new banned field with
                    // a different name in an already-baselined file is a fresh
                    // key, so it is NOT masked.
                    key: field,
                    line: lc.no,
                    detail: format!("banned display/render field token `{token}`: {trimmed}"),
                });
            }
        }
    }

    evaluate(
        "Rule A (display-enrichment-in-primitive)",
        "sub-L5 protocol primitives must not carry display/render fields \
         (crate-boundaries.md §display-separation).",
        RULE_A_BASELINE,
        &occs,
    );
}
