use crate::support::{
    collect_files, crates_dir, evaluate, field_ident, lang_of, nmp_nip_crates, read, rel,
    scan_blocks, Occurrence,
};

/// Fine-grained baseline (tracked debt): `(file, field-name)`. The owning fix
/// PR removes each line when it deletes the field. Do NOT add new entries — a
/// new banned field with a different name fires even inside a file that already
/// carries a *different* baselined field (no file-level masking).
const RULE_A_BASELINE: &[(&str, &str)] = &[];

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

#[test]
fn rule_a_nip01_must_not_reintroduce_feed_render_contracts() {
    let banned = [
        "TimelineEventCard",
        "RootCard",
        "OpFeedSnapshot",
        "ContentRenderData",
        "content_render",
        "cards:[",
        "card:TimelineEventCard",
        "nmp.nip01.opfeed",
        "NOFS",
        "register_op_feed",
        "FlatFeed",
        "Nip10ReplyAttribution",
    ];
    let mut files = Vec::new();
    let crate_dir = crates_dir().join("nmp-nip01");
    collect_files(&crate_dir.join("src"), &["rs"], &mut files);
    collect_files(&crate_dir.join("schema"), &["fbs"], &mut files);

    let mut violations = Vec::new();
    for file in files {
        if rel(&file).contains("wire/generated/") {
            continue;
        }
        let content = read(&file);
        for (line_idx, line) in content.lines().enumerate() {
            for token in banned {
                if line.contains(token) {
                    violations.push(format!(
                        "{}:{} contains `{token}`",
                        rel(&file),
                        line_idx + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "nmp-nip01 must not own feed/render contract vocabulary after #2510:\n{}",
        violations.join("\n")
    );
}
