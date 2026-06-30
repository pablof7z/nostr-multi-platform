use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::rule_b_baseline::{baseline_for, RULE_B_BASELINE};
use crate::rule_b_matchers::{
    contains_token, rejected_relation_token, storage_aggregation_token,
    storage_nip10_marker_classifier, storage_relation_kind_classifier, ENGAGEMENT_NOUNS,
};
use crate::support::{
    collect_files, crates_dir, field_ident, lang_of, nmp_nip_crates, read, rel, scan_blocks,
};

fn is_test_source(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name == "tests.rs" || name.starts_with("tests_") || name.ends_with("_tests.rs")
}

fn collect_rule_b_files(crate_name: &str, out: &mut Vec<PathBuf>) {
    let cd = crates_dir().join(crate_name);
    let mut files = Vec::new();
    collect_files(&cd.join("src"), &["rs"], &mut files);
    collect_files(&cd.join("schema"), &["fbs"], &mut files);
    out.extend(files.into_iter().filter(|path| !is_test_source(path)));
}

fn record_hit(
    file: &Path,
    line: usize,
    detail: String,
    counts: &mut BTreeMap<String, usize>,
    violations: &mut Vec<String>,
) {
    let path = rel(file);
    if baseline_for(&path).is_some() {
        *counts.entry(path).or_default() += 1;
    } else {
        violations.push(format!(
            "{path}:{line}: Rule B (rejected relation summary) — {detail}"
        ));
    }
}

#[test]
fn rule_b_no_global_relation_summary_or_bucket_api() {
    let storage = ["nmp-store", "nmp-nostr-lmdb", "nmp-sqlite-wasm"];
    let mut relation_scope = vec!["nmp-relations".to_string()];
    relation_scope.extend(nmp_nip_crates());

    let mut storage_files = Vec::new();
    for d in &storage {
        collect_rule_b_files(d, &mut storage_files);
    }

    let mut all_files = storage_files.clone();
    for d in &relation_scope {
        collect_rule_b_files(d, &mut all_files);
    }
    all_files.sort();
    all_files.dedup();
    assert!(
        !all_files.is_empty(),
        "Rule B scanned zero files — gate would be vacuous"
    );

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut violations = Vec::new();

    for file in &all_files {
        let path = rel(file);
        let lang = lang_of(file);
        let content = read(file);
        let scan = scan_blocks(&content);
        let is_storage = storage_files.iter().any(|f| f == file);

        if path.starts_with("crates/nmp-relations/src/") {
            record_hit(
                file,
                1,
                "central `nmp-relations` production source; split/delete per #2508".to_string(),
                &mut counts,
                &mut violations,
            );
        }

        for block in &scan.blocks {
            if contains_token(&block.name, "InteractionCounts")
                || contains_token(&block.name, "RelationCounts")
                || contains_token(&block.name, "RelationSummary")
            {
                record_hit(
                    file,
                    block.first_line,
                    format!("aggregate relation/count type `{}`", block.name),
                    &mut counts,
                    &mut violations,
                );
            }
        }

        let mut per_block: BTreeMap<usize, BTreeSet<&str>> = BTreeMap::new();
        for lc in &scan.lines {
            let trimmed = lc.text.trim_start();
            if let Some(token) = rejected_relation_token(trimmed) {
                record_hit(
                    file,
                    lc.no,
                    format!("rejected relation vocabulary `{token}`: {trimmed}"),
                    &mut counts,
                    &mut violations,
                );
            }
            if is_storage {
                if let Some(token) = storage_aggregation_token(trimmed) {
                    record_hit(
                        file,
                        lc.no,
                        format!("storage owns engagement aggregation token `{token}`: {trimmed}"),
                        &mut counts,
                        &mut violations,
                    );
                }
                if let Some(label) = storage_relation_kind_classifier(trimmed) {
                    record_hit(
                        file,
                        lc.no,
                        format!("storage classifies protocol engagement kind `{label}`: {trimmed}"),
                        &mut counts,
                        &mut violations,
                    );
                }
                if storage_nip10_marker_classifier(trimmed) {
                    record_hit(
                        file,
                        lc.no,
                        format!("storage owns NIP-10 reply/root marker policy: {trimmed}"),
                        &mut counts,
                        &mut violations,
                    );
                }
            }
            let Some(block_id) = lc.block else { continue };
            let Some(field) = field_ident(trimmed, lang) else {
                continue;
            };
            if let Some(noun) = ENGAGEMENT_NOUNS.iter().find(|n| **n == field) {
                per_block.entry(block_id).or_default().insert(*noun);
            }
        }
        for (block_id, nouns) in &per_block {
            if nouns.len() >= 2 {
                let block = scan.blocks.iter().find(|b| b.id == *block_id);
                let (line, name) = block
                    .map(|b| (b.first_line, b.name.as_str()))
                    .unwrap_or((0, "?"));
                record_hit(
                    file,
                    line,
                    format!("type `{name}` co-names engagement buckets {nouns:?}"),
                    &mut counts,
                    &mut violations,
                );
            }
        }
    }

    for baseline in RULE_B_BASELINE {
        let count = counts.get(baseline.path).copied().unwrap_or(0);
        if count > baseline.max_hits {
            violations.push(format!(
                "{}:1: Rule B baseline for {} grew from {} to {} hits ({})",
                baseline.path, baseline.issue, baseline.max_hits, count, baseline.reason
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "Rule B: concept-owned active reads are the architecture; reusable crates must \
         not expose global relation summaries, bucket APIs, rejected relation vocabulary, \
         or a central nmp-relations owner. Existing debt is capped to open issues #2508/#2512. \
         New violation(s) — fix, do NOT baseline:\n{}",
        violations.join("\n")
    );
}
