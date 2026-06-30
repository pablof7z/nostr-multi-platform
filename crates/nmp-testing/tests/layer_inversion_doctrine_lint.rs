//! Layer-inversion doctrine gate — durable backstop for the 2026-06 crate-layer
//! audit (issues #2510, #2508, #2512, #2513, #2514, #2515).
//!
//! A "layer inversion" is a sub-L5 crate owning a concern that belongs to a
//! higher layer: render / feed-item shape, display enrichment, an app-named
//! noun, cross-protocol engagement aggregation, or a substrate naming a
//! protocol noun. `docs/architecture/crate-boundaries.md` (§2–§10a) is the
//! durable spec; this grep gate is the CI ratchet that prevents *new*
//! inversions from being introduced while the audited debt is paid down.
//!
//! Four independent rules, each scoped to the layer it protects:
//!
//! * **Rule A — display-enrichment-in-primitive.** L1/L4 protocol primitives
//!   (`nmp-nip01`, `nmp-content`, every L4 `nmp-nipNN`, `nmp-feed`,
//!   `nmp-threading`) must not carry kind:0 display strings or rendered
//!   previews as struct/table *fields*. The kind:0 `Profile*` vocabulary is
//!   carved out (it is the legitimate owner of display data).
//! * **Rule B — cross-protocol aggregation in single-protocol/storage
//!   substrate.** `nmp-store`, `nmp-nostr-lmdb`, and every single-protocol
//!   `nmp-nipNN` (except `nmp-nip01`, whose `NoteRelationCounts` vocabulary is
//!   carved out by crate-boundaries.md §8) must not co-name multiple
//!   engagement nouns in one type, name `TargetInteractionCounts`, or classify
//!   on the zap kind literal `9735`. `nmp-relations` is the designated owner
//!   and is skipped.
//! * **Rule C — kind-blind transport (`nmp-nip29`).** NIP-29 is kind-blind
//!   `h`-tag transport: it owns ONE generic publish-into-group verb plus pure
//!   envelope/admin ops. Kind-specific verbs (`react`/`repost`/`share`) and
//!   kind constants must not live here.
//! * **Rule D — substrate protocol-noun (`nmp-core`).** The substrate kernel
//!   must not own NIP-19 entity codecs (`nip19`, `Nip19Entity`,
//!   `Nprofile`/`Nevent`/`Naddr` types). NIP-21 `NostrUri` and `parse_nip10`
//!   were judged legitimate generic substrate codecs by the audit and are NOT
//!   banned.
//!
//! # Baseline ratchet
//!
//! The 15 audited violations still exist on `master`, so each rule carries an
//! explicit BASELINE ALLOWLIST of the known-violation files, each annotated
//! with its owning issue. A rule fails only on occurrences NOT in its baseline.
//! Baseline entries are tracked debt; the owning fix PR removes its line when it
//! lands. Do NOT add new entries — a new violation must be fixed, not
//! baselined.
//!
//! # Running
//!
//! ```bash
//! cargo test -p nmp-testing --test layer_inversion_doctrine_lint
//! ```

use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Workspace root (parent of `crates/`). `CARGO_MANIFEST_DIR` is
/// `crates/nmp-testing`; two `parent()` hops reach the repo root.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn crates_dir() -> PathBuf {
    workspace_root().join("crates")
}

/// All `crates/nmp-nip*` crate directory names, sorted. New NIP crates are
/// auto-covered by the rules that scan the family.
fn nmp_nip_crates() -> Vec<String> {
    let mut out = Vec::new();
    for entry in fs::read_dir(crates_dir()).expect("read crates dir") {
        let path = entry.expect("dir entry").path();
        if !path.is_dir() {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("nmp-nip") {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    out
}

/// Path relative to the workspace root, forward-slash normalised, for stable
/// display and baseline matching.
fn rel(path: &Path) -> String {
    path.strip_prefix(workspace_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Recursively collect files with one of `exts` under `dir`, skipping `tests/`
/// directories, `*fixtures*` paths, and machine-generated files
/// (`*/generated/*`, `*.generated.rs`). The carve-outs match the audit's
/// "authored declarations only" intent.
fn collect_files(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}")) {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "tests" || name == "generated" {
                continue;
            }
            collect_files(&path, exts, out);
        } else {
            let r = rel(&path);
            if r.contains("fixtures") || r.ends_with(".generated.rs") {
                continue;
            }
            if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| exts.contains(&e))
            {
                out.push(path);
            }
        }
    }
}

/// `true` if the trimmed line is a comment (`//`, `///`, `//!` in Rust; `//`
/// in `.fbs`).
fn is_comment(trimmed: &str) -> bool {
    trimmed.starts_with("//")
}

/// Read a file to a string (lossless on UTF-8; panics loudly on IO error).
fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

/// A single scanned line plus its enclosing-type context.
struct LineCtx {
    /// 1-based line number.
    no: usize,
    /// The raw line text.
    text: String,
    /// Names of the enclosing `struct`/`enum`/`table` definition blocks
    /// (outermost first), as of *before* this line opened/closed any brace.
    /// Non-definition braces (fn/impl/struct-literal bodies) appear as empty
    /// strings.
    def_stack: Vec<String>,
    /// Id of the innermost *named* definition block enclosing this line, or
    /// `None` if the innermost brace is a non-definition body. Distinguishes a
    /// field DEFINITION (inside a struct/table) from a struct LITERAL init
    /// (inside a fn body).
    block: Option<usize>,
}

/// A named definition block discovered while scanning.
struct Block {
    id: usize,
    first_line: usize,
    name: String,
}

struct Scan {
    lines: Vec<LineCtx>,
    blocks: Vec<Block>,
}

/// If `trimmed` is a `struct`/`enum`/`table` declaration header, return the
/// declared type name. Handles an optional `pub` / `pub(...)` prefix. Used to
/// track enclosing-type context; not itself a violation check.
fn decl_name(trimmed: &str) -> Option<String> {
    if is_comment(trimmed) {
        return None;
    }
    let mut s = trimmed;
    // Strip a leading visibility modifier.
    if let Some(rest) = s.strip_prefix("pub") {
        let rest = rest.trim_start();
        // `pub(crate)` / `pub(super)` etc.
        if let Some(after) = rest.strip_prefix('(') {
            if let Some(idx) = after.find(')') {
                s = after[idx + 1..].trim_start();
            } else {
                s = rest;
            }
        } else {
            s = rest;
        }
    }
    for kw in ["struct ", "enum ", "table "] {
        if let Some(rest) = s.strip_prefix(kw) {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Scan a file's text, tracking enclosing `struct`/`enum`/`table` definition
/// blocks via a brace-depth stack. Robust for the one-declaration-per-line,
/// one-brace-per-line formatting that Rust and FlatBuffers schemas use.
fn scan_blocks(content: &str) -> Scan {
    let mut name_stack: Vec<String> = Vec::new();
    let mut id_stack: Vec<Option<usize>> = Vec::new();
    let mut pending: Option<String> = None;
    let mut next_id = 0usize;
    let mut blocks: Vec<Block> = Vec::new();
    let mut lines: Vec<LineCtx> = Vec::new();

    for (i, raw) in content.lines().enumerate() {
        let trimmed = raw.trim_start();
        // Snapshot context BEFORE this line mutates the stacks.
        let snapshot_names = name_stack.clone();
        // Innermost *named* definition block enclosing this line (skips
        // non-definition `{}` bodies, which are stored as `None`).
        let snapshot_block = id_stack.iter().rev().find_map(|b| *b);

        if pending.is_none() {
            pending = decl_name(trimmed);
        }

        let mut opened_brace = false;
        for ch in raw.chars() {
            match ch {
                '{' => {
                    opened_brace = true;
                    if let Some(name) = pending.take() {
                        let id = next_id;
                        next_id += 1;
                        blocks.push(Block {
                            id,
                            first_line: i + 1,
                            name: name.clone(),
                        });
                        name_stack.push(name);
                        id_stack.push(Some(id));
                    } else {
                        name_stack.push(String::new());
                        id_stack.push(None);
                    }
                }
                '}' => {
                    name_stack.pop();
                    id_stack.pop();
                }
                _ => {}
            }
        }
        // A unit/tuple struct (`struct Foo;`) or a forward decl never opens a
        // body; drop the pending name so it does not bind to a later brace.
        if pending.is_some() && !opened_brace && raw.contains(';') {
            pending = None;
        }

        lines.push(LineCtx {
            no: i + 1,
            text: raw.to_string(),
            def_stack: snapshot_names,
            block: snapshot_block,
        });
    }

    Scan { lines, blocks }
}

/// Leading identifier of a `name : Type` field declaration, if `trimmed` looks
/// like one. `kind` selects the syntax: `Lang::Fbs` accepts a bare `name:Type`
/// field; `Lang::Rust` requires a `pub <ident>:` field. Returns the field name.
fn field_ident(trimmed: &str, lang: Lang) -> Option<String> {
    if is_comment(trimmed) {
        return None;
    }
    let body = match lang {
        Lang::Rust => trimmed.strip_prefix("pub ")?.trim_start(),
        Lang::Fbs => trimmed,
    };
    let mut chars = body.char_indices();
    // First char of an identifier.
    let first = chars.next()?;
    if !(first.1.is_ascii_alphabetic() || first.1 == '_') {
        return None;
    }
    let mut end = body.len();
    for (idx, c) in body.char_indices().skip(1) {
        if c.is_ascii_alphanumeric() || c == '_' {
            continue;
        }
        end = idx;
        break;
    }
    let name = &body[..end];
    let rest = body[end..].trim_start();
    let after = rest.strip_prefix(':')?;
    // `::` is a path separator, not a field colon.
    if after.starts_with(':') {
        return None;
    }
    match lang {
        // A field type follows immediately for `.fbs` (`name:Type`).
        Lang::Fbs => {
            let t = after.trim_start();
            let c = t.chars().next()?;
            if c.is_ascii_alphabetic() || c == '[' {
                Some(name.to_string())
            } else {
                None
            }
        }
        Lang::Rust => Some(name.to_string()),
    }
}

#[derive(Clone, Copy)]
enum Lang {
    Rust,
    Fbs,
}

fn lang_of(path: &Path) -> Lang {
    if path.extension().and_then(|e| e.to_str()) == Some("fbs") {
        Lang::Fbs
    } else {
        Lang::Rust
    }
}

// ---------------------------------------------------------------------------
// Rule A — display-enrichment-in-primitive
// ---------------------------------------------------------------------------

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
fn has_formatted_field(line: &str) -> bool {
    let mut idx = 0;
    while let Some(pos) = line[idx..].find("formatted_") {
        let start = idx + pos;
        let after = &line[start + "formatted_".len()..];
        if after.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
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
    dirs.extend(nmp_nip_crates()); // includes nmp-nip01

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
            // Profile carve-out: the kind:0 ProfileProjection vocabulary owns
            // display data legitimately.
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

// ---------------------------------------------------------------------------
// Rule B — cross-protocol aggregation in single-protocol / storage substrate
// ---------------------------------------------------------------------------

/// Baseline (tracked debt). The owning fix PR removes its line when it lands.
/// Do NOT add new entries.
const RULE_B_BASELINE: &[&str] = &[
    // #2512 — TargetInteractionCounts aggregate + zap classify in nmp-store.
    "crates/nmp-store/src/types/outcomes.rs", // pub struct TargetInteractionCounts { replies, reactions, reposts, zaps }
    "crates/nmp-store/src/events.rs",         // interaction_counts trait method
    "crates/nmp-store/src/interaction.rs",    // classify(9735 => Zap)
    "crates/nmp-store/src/lmdb/interaction_counters.rs", // TargetInteractionCounts construction
];

const ENGAGEMENT_NOUNS: &[&str] = &["replies", "reactions", "reposts", "zaps", "comments"];

/// `true` if `line` is a match/classify arm keyed on the zap kind literal
/// `9735` (e.g. `9735 => ...`). Distinguishes cross-protocol classification
/// from a protocol crate's own kind constant.
fn classifies_on_zap_kind(trimmed: &str) -> bool {
    if is_comment(trimmed) {
        return false;
    }
    if let Some(pos) = trimmed.find("9735") {
        // Boundary before.
        let before_ok = trimmed[..pos]
            .chars()
            .last()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
        let after = trimmed[pos + 4..].trim_start();
        let after_ok = after.starts_with("=>") || after.starts_with('|');
        // Guard against `19735`, `97350`, etc.
        let next_is_digit = trimmed[pos + 4..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit());
        before_ok && after_ok && !next_is_digit
    } else {
        false
    }
}

#[test]
fn rule_b_no_cross_protocol_aggregation_in_substrate() {
    // Storage substrate (the `9735` classify check applies here only).
    let storage = ["nmp-store", "nmp-nostr-lmdb"];
    // Single-protocol crates: every nmp-nipNN except nmp-nip01 (NoteRelationCounts
    // vocabulary carved out by crate-boundaries.md §8). nmp-relations is the
    // designated owner and is never scanned.
    let nips: Vec<String> = nmp_nip_crates()
        .into_iter()
        .filter(|c| c != "nmp-nip01")
        .collect();

    let mut storage_files = Vec::new();
    for d in &storage {
        collect_files(&crates_dir().join(d).join("src"), &["rs"], &mut storage_files);
    }
    let mut all_files = storage_files.clone();
    for d in &nips {
        let cd = crates_dir().join(d);
        collect_files(&cd.join("src"), &["rs"], &mut all_files);
        collect_files(&cd.join("schema"), &["fbs"], &mut all_files);
    }
    assert!(
        !all_files.is_empty(),
        "Rule B scanned zero files — gate would be vacuous"
    );

    let mut violations = Vec::new();

    for file in &all_files {
        let lang = lang_of(file);
        let content = read(file);
        let scan = scan_blocks(&content);
        let baselined = RULE_B_BASELINE.contains(&rel(file).as_str());
        let is_storage = storage_files.iter().any(|f| f == file);

        // (b1) A type DEFINITION named `*InteractionCounts`.
        for block in &scan.blocks {
            if block.name.contains("InteractionCounts") && !baselined {
                violations.push(format!(
                    "{}:{}: Rule B (cross-protocol-aggregation) — aggregate type `{}`",
                    rel(file),
                    block.first_line,
                    block.name
                ));
            }
        }

        // (b2) A single definition block co-naming >= 2 distinct engagement nouns.
        let mut per_block: std::collections::BTreeMap<usize, std::collections::BTreeSet<&str>> =
            std::collections::BTreeMap::new();
        for lc in &scan.lines {
            let Some(block_id) = lc.block else { continue };
            let trimmed = lc.text.trim_start();
            let Some(field) = field_ident(trimmed, lang) else {
                continue;
            };
            if let Some(noun) = ENGAGEMENT_NOUNS.iter().find(|n| **n == field) {
                per_block.entry(block_id).or_default().insert(*noun);
            }
        }
        for (block_id, nouns) in &per_block {
            if nouns.len() >= 2 && !baselined {
                let block = scan.blocks.iter().find(|b| b.id == *block_id);
                let (line, name) = block
                    .map(|b| (b.first_line, b.name.as_str()))
                    .unwrap_or((0, "?"));
                violations.push(format!(
                    "{}:{}: Rule B (cross-protocol-aggregation) — type `{}` co-names engagement nouns {:?}",
                    rel(file),
                    line,
                    name,
                    nouns
                ));
            }
        }

        // (b3) Storage substrate classifying on the zap kind literal `9735`.
        if is_storage {
            for lc in &scan.lines {
                let trimmed = lc.text.trim_start();
                if classifies_on_zap_kind(trimmed) && !baselined {
                    violations.push(format!(
                        "{}:{}: Rule B (cross-protocol-aggregation) — storage classifies on zap kind 9735: {}",
                        rel(file),
                        lc.no,
                        trimmed
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Rule B: storage / single-protocol substrate must not aggregate cross-protocol \
         engagement (crate-boundaries.md §8; nmp-relations is the owner). New violation(s) \
         — fix, do NOT baseline:\n{}",
        violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Rule C — kind-blind transport (nmp-nip29)
// ---------------------------------------------------------------------------

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
const RULE_C_NS_ALLOWLIST: &[&str] = &[
    // The single generic publish-into-group verb.
    "publish_group_event",
    // Pure envelope / admin action ops.
    "put_user",
    "create_invite",
    "create_public_group",
    "discover",
    "edit_metadata",
    "join",
    "leave",
    "set_parent",
    // Projection / cache / wire snapshot keys.
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
fn nip29_namespaces(line: &str) -> Vec<String> {
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

    // src/**: banned namespace verbs + REACTION_KIND/REPOST_KIND constants.
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

    // schema/**: react/repost/share .fbs filenames are kind-specific verbs.
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

// ---------------------------------------------------------------------------
// Rule D — substrate protocol-noun (nmp-core)
// ---------------------------------------------------------------------------

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
fn is_nip19_entity_ident(ident: &str) -> bool {
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
            // `pub mod nip19` — the entity-codec module.
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
            // `pub enum`/`pub struct` entity surfaces.
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

// ---------------------------------------------------------------------------
// Matcher sanity (non-vacuous, guards the helpers themselves)
// ---------------------------------------------------------------------------

#[test]
fn matchers_are_correct() {
    // field_ident
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
    assert!(field_ident("use crate::Foo::bar;", Lang::Rust).is_none()); // not a pub field
    assert!(field_ident("Self::Variant => x,", Lang::Fbs).is_none()); // path `::` is not a field colon

    // decl_name
    assert_eq!(
        decl_name("pub struct TargetInteractionCounts {").as_deref(),
        Some("TargetInteractionCounts")
    );
    assert_eq!(decl_name("table RootCard {").as_deref(), Some("RootCard"));
    assert_eq!(
        decl_name("pub(crate) enum Foo {").as_deref(),
        Some("Foo")
    );
    assert!(decl_name("// struct Foo {").is_none());

    // zap-kind classifier
    assert!(classifies_on_zap_kind("9735 => first_e_tag(tags),"));
    assert!(classifies_on_zap_kind("9735 | 7 => x,"));
    assert!(!classifies_on_zap_kind("const ZAP: u32 = 9735;"));
    assert!(!classifies_on_zap_kind("// kind:9735 is a zap"));
    assert!(!classifies_on_zap_kind("19735 => x,"));

    // namespace extraction + allowlist
    assert_eq!(
        nip29_namespaces(r#"const NS: &str = "nmp.nip29.react_in_group";"#),
        vec!["react_in_group".to_string()]
    );
    assert!(!RULE_C_NS_ALLOWLIST.contains(&"react_in_group"));
    assert!(RULE_C_NS_ALLOWLIST.contains(&"publish_group_event"));

    // nip19 entity idents
    assert!(is_nip19_entity_ident("Nip19Entity"));
    assert!(is_nip19_entity_ident("NprofileData"));
    assert!(is_nip19_entity_ident("NeventData"));
    assert!(is_nip19_entity_ident("NaddrData"));
    assert!(!is_nip19_entity_ident("NostrUri"));
    assert!(!is_nip19_entity_ident("Nip21Error"));

    // formatted_* field
    assert!(has_formatted_field("pub formatted_amount: String,"));
    assert!(!has_formatted_field("pub amount: String,"));

    // block scanner: struct def counts noun fields; struct literal does not.
    let src = "pub struct Agg {\n    pub replies: u64,\n    pub zaps: u64,\n}\nfn f() {\n    let x = Agg { replies: 1, zaps: 2 };\n}\n";
    let scan = scan_blocks(src);
    let agg = scan.blocks.iter().find(|b| b.name == "Agg").expect("Agg block");
    let mut nouns = std::collections::BTreeSet::new();
    for lc in &scan.lines {
        if lc.block == Some(agg.id) {
            if let Some(f) = field_ident(lc.text.trim_start(), Lang::Rust) {
                if ENGAGEMENT_NOUNS.contains(&f.as_str()) {
                    nouns.insert(f);
                }
            }
        }
    }
    assert_eq!(nouns.len(), 2, "struct def must count both noun fields");
    // The struct-literal init line has block == None (inside fn body), so it is
    // never counted as a co-naming definition.
    let lit_line = scan
        .lines
        .iter()
        .find(|lc| lc.text.contains("Agg { replies"))
        .expect("literal line");
    assert_eq!(lit_line.block, None);
}
