use std::fs;
use std::path::{Path, PathBuf};

/// Workspace root (parent of `crates/`). `CARGO_MANIFEST_DIR` is
/// `crates/nmp-testing`; two `parent()` hops reach the repo root.
pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

pub(crate) fn crates_dir() -> PathBuf {
    workspace_root().join("crates")
}

/// All `crates/nmp-nip*` crate directory names, sorted. New NIP crates are
/// auto-covered by the rules that scan the family.
pub(crate) fn nmp_nip_crates() -> Vec<String> {
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
pub(crate) fn rel(path: &Path) -> String {
    path.strip_prefix(workspace_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Recursively collect files with one of `exts` under `dir`, skipping `tests/`
/// directories, `*fixtures*` paths, and machine-generated files
/// (`*/generated/*`, `*.generated.rs`). The carve-outs match the audit's
/// "authored declarations only" intent.
pub(crate) fn collect_files(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
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
pub(crate) fn is_comment(trimmed: &str) -> bool {
    trimmed.starts_with("//")
}

/// Read a file to a string (lossless on UTF-8; panics loudly on IO error).
pub(crate) fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

/// A single scanned line plus its enclosing-type context.
pub(crate) struct LineCtx {
    /// 1-based line number.
    pub(crate) no: usize,
    /// The raw line text.
    pub(crate) text: String,
    /// Names of the enclosing `struct`/`enum`/`table` definition blocks
    /// (outermost first), as of *before* this line opened/closed any brace.
    /// Non-definition braces (fn/impl/struct-literal bodies) appear as empty
    /// strings.
    pub(crate) def_stack: Vec<String>,
    /// Id of the innermost *named* definition block enclosing this line, or
    /// `None` if the innermost brace is a non-definition body. Distinguishes a
    /// field DEFINITION (inside a struct/table) from a struct LITERAL init
    /// (inside a fn body).
    pub(crate) block: Option<usize>,
}

/// A named definition block discovered while scanning.
pub(crate) struct Block {
    pub(crate) id: usize,
    pub(crate) first_line: usize,
    pub(crate) name: String,
}

pub(crate) struct Scan {
    pub(crate) lines: Vec<LineCtx>,
    pub(crate) blocks: Vec<Block>,
}

/// If `trimmed` is a `struct`/`enum`/`table` declaration header, return the
/// declared type name. Handles an optional `pub` / `pub(...)` prefix. Used to
/// track enclosing-type context; not itself a violation check.
pub(crate) fn decl_name(trimmed: &str) -> Option<String> {
    if is_comment(trimmed) {
        return None;
    }
    let mut s = trimmed;
    if let Some(rest) = s.strip_prefix("pub") {
        let rest = rest.trim_start();
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
pub(crate) fn scan_blocks(content: &str) -> Scan {
    let mut name_stack: Vec<String> = Vec::new();
    let mut id_stack: Vec<Option<usize>> = Vec::new();
    let mut pending: Option<String> = None;
    let mut next_id = 0usize;
    let mut blocks: Vec<Block> = Vec::new();
    let mut lines: Vec<LineCtx> = Vec::new();

    for (i, raw) in content.lines().enumerate() {
        let trimmed = raw.trim_start();
        let snapshot_names = name_stack.clone();
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
pub(crate) fn field_ident(trimmed: &str, lang: Lang) -> Option<String> {
    if is_comment(trimmed) {
        return None;
    }
    let body = match lang {
        Lang::Rust => trimmed.strip_prefix("pub ")?.trim_start(),
        Lang::Fbs => trimmed,
    };
    let mut chars = body.char_indices();
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
    if after.starts_with(':') {
        return None;
    }
    match lang {
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
pub(crate) enum Lang {
    Rust,
    Fbs,
}

pub(crate) fn lang_of(path: &Path) -> Lang {
    if path.extension().and_then(|e| e.to_str()) == Some("fbs") {
        Lang::Fbs
    } else {
        Lang::Rust
    }
}
