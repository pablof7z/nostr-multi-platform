use crate::support::{
    collect_files, crates_dir, field_ident, lang_of, nmp_nip_crates, read, rel, scan_blocks,
};

/// Baseline (tracked debt). The owning fix PR removes its line when it lands.
/// Do NOT add new entries.
const RULE_A_BASELINE: &[&str] = &[
    // #2510 / #2508 — op-centric timeline render cards in nmp-nip01.
    "crates/nmp-nip01/schema/timeline_snapshot.fbs", // TimelineEventCard display mirrors
    "crates/nmp-nip01/src/timeline_projection.rs",   // re-export of render_data surfaces
    "crates/nmp-nip01/src/timeline_projection/render_data.rs", // ContentEventRenderData fields
    "crates/nmp-nip01/schema/op_feed.fbs",           // RootCard author_display/content_preview
    "crates/nmp-nip01/src/op_feed/attribution.rs",   // RepostAttribution.author_display
    // #2514 — embed/longform render previews in nmp-content.
    "crates/nmp-content/src/embed_projection/variants.rs", // author_display_name/picture_url fields
    "crates/nmp-content/schema/embed_sidecar.fbs",
    "crates/nmp-content/schema/longform.fbs",
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

    let mut violations = Vec::new();
    for file in &files {
        let lang = lang_of(file);
        let content = read(file);
        let scan = scan_blocks(&content);
        let baselined = RULE_A_BASELINE.contains(&rel(file).as_str());
        for lc in &scan.lines {
            let trimmed = lc.text.trim_start();
            if lc
                .def_stack
                .iter()
                .any(|n| n.to_ascii_lowercase().contains("profile"))
            {
                continue;
            }
            if field_ident(trimmed, lang).is_none() {
                continue;
            }
            let hit = RULE_A_BANNED
                .iter()
                .find(|b| lc.text.contains(**b))
                .map(|b| b.to_string())
                .or_else(|| has_formatted_field(&lc.text).then(|| "formatted_*".to_string()));
            if let Some(token) = hit {
                if !baselined {
                    violations.push(format!(
                        "{}:{}: Rule A (display-enrichment-in-primitive) — banned field token `{}`: {}",
                        rel(file),
                        lc.no,
                        token,
                        trimmed
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Rule A: sub-L5 protocol primitives must not carry display/render fields \
         (crate-boundaries.md §display-separation). New violation(s) — fix, do NOT \
         baseline:\n{}",
        violations.join("\n")
    );
}
