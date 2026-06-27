//! D18 - native shell doctrine.
//!
//! Swift/Kotlin/Java are rendering and capability layers. This rule keeps the
//! mechanically detectable high-risk violations out of native shells without
//! trying to parse UI rendering code:
//! - polling shaped as sleep/delay calls inside explicit loops;
//! - native timers that periodically query framework state;
//! - native construction of raw publish envelopes (`PublishRaw`,
//!   `PublishProfile`);
//! - app-facing **namespace/body** action dispatch that leaks the ADR-0064
//!   byte transport vocabulary above generated or bridge-internal code.
//!   Specifically: any dispatch token (see `TRANSPORT_DISPATCH_TOKENS`) that
//!   appears on the same line as a `"nmp.` namespace string literal.  The
//!   pure-bytes doorway `appHandle?.dispatchActionBytes(bytes)` — with no
//!   namespace literal — is the sanctioned ADR-0064 endpoint and is NOT
//!   flagged by this rule.  Hand-written wrappers must be named without the
//!   transport vocabulary (e.g. `dispatchBytes`) so they are not confused with
//!   transport-shaped calls;
//! - lifecycle debt markers that justify leaks instead of modelling ownership.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::allow;
use crate::braces::count_braces_ignoring_strings;
use crate::report::Finding;

pub const ID: &str = "D18";

const ALLOWLIST: &str = include_str!("../native-allowlist.txt");

const SLEEP_TOKENS: &[&str] = &[
    "Task.sleep(",
    "Thread.sleep(",
    "delay(",
    "usleep(",
    "DispatchQueue.main.asyncAfter(",
];

const TIMER_TOKENS: &[&str] = &[".scheduledTimer("];
const PUBLISH_POLICY_TOKENS: &[&str] = &["PublishRaw", "PublishProfile"];
const TRANSPORT_DISPATCH_TOKENS: &[&str] = &[
    "fun dispatchAction(",
    "fun dispatchActionBytes(",
    "func dispatchAction(namespace:",
    "func dispatchRawAction(",
    "func dispatchRawActionBytes(",
    "dispatchActionBytes(",
    "dispatchRawActionBytes(",
    "dispatchRawAction(namespace:",
    "dispatchAction(namespace:",
];
const LEAK_DEBT_MARKERS: &[&str] = &[
    "small bounded leak",
    "bounded leak",
    "intentional leak",
    "intentionally leak",
    "leak intentionally",
    "temporary leak",
];

pub fn collect_native_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk(root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(
            name.as_ref(),
            ".git" | "target" | "build" | ".gradle" | "node_modules" | "fixtures"
        ) {
            continue;
        }
        if file_type.is_dir() {
            walk(&path, out)?;
        } else if is_native_file(&path) && !is_generated_or_test_path(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn is_native_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()),
        Some("swift" | "kt" | "kts" | "java")
    )
}

fn is_generated_or_test_path(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.contains("/Bridge/Generated/")
        || s.contains("/Generated/")
        || s.ends_with(".generated.swift")
        || s.contains("/Tests/")
        || s.contains("/UITests/")
        || s.contains("/src/test/")
        || s.contains("/src/androidTest/")
        || s.ends_with("Test.kt")
        || s.ends_with("Test.java")
        || s.ends_with("Tests.swift")
}

pub fn scan_file(path: &Path, findings: &mut Vec<Finding>) -> io::Result<()> {
    let body = fs::read_to_string(path)?;
    let mut in_block_comment = false;
    let mut loops = LoopTracker::default();

    for (idx, raw_line) in body.lines().enumerate() {
        let line_no = idx + 1;
        let starts_in_block_comment = in_block_comment;
        let trimmed = raw_line.trim_start();
        let is_comment = starts_in_block_comment || trimmed.starts_with("//");
        let in_loop = loops.in_loop() || loops.line_opens_loop(raw_line, starts_in_block_comment);

        for hit in check_line(path, raw_line, is_comment, in_loop) {
            if allow::line_allows(raw_line, ID) || is_allowlisted(path, raw_line) {
                continue;
            }
            findings.push(Finding {
                rule: ID,
                path: path.to_path_buf(),
                line: line_no,
                col: hit.col,
                message: hit.message,
                suggested: hit.suggested,
            });
        }

        loops.observe_line(raw_line, starts_in_block_comment);
        update_block_comment(raw_line, &mut in_block_comment);
    }
    Ok(())
}

struct Hit {
    col: usize,
    message: String,
    suggested: String,
}

fn check_line(path: &Path, line: &str, is_comment: bool, in_loop: bool) -> Vec<Hit> {
    let mut hits = Vec::new();
    let path_s = path.to_string_lossy().replace('\\', "/");

    if in_loop && !is_comment {
        for token in SLEEP_TOKENS {
            push_token_hits(&mut hits, line, token, "`sleep`/`delay` inside a native loop violates D18 - no polling; use the pushed kernel update stream or an OS callback");
        }
    }

    if !is_comment {
        for token in TIMER_TOKENS {
            push_token_hits(&mut hits, line, token, "native scheduled timers violate D18 when used as framework polling; consume pushed snapshots or OS callbacks");
        }
        for token in PUBLISH_POLICY_TOKENS {
            push_token_hits(&mut hits, line, token, "native publish-envelope construction violates D18 - Rust owns event kind, tag, target, and publish policy");
        }
        // Only flag transport-dispatch tokens when a namespace string literal
        // is present on the same line.  The pure-bytes doorway call
        // `appHandle?.dispatchActionBytes(bytes)` (no "nmp." literal) is
        // the sanctioned ADR-0064 endpoint and must NOT be flagged.
        if line.contains("\"nmp.") {
            for token in TRANSPORT_DISPATCH_TOKENS {
                push_token_hits(
                    &mut hits,
                    line,
                    token,
                    "transport-shaped action dispatch violates D18 - app-facing native code must expose typed intent methods, not namespace/body dispatch",
                );
            }
        }
        if line.contains("dispatchAction(") && line.contains("\"nmp.") {
            push_token_hits(
                &mut hits,
                line,
                "dispatchAction(",
                "with a literal action namespace violates D18 - wrap the write in a typed native method or generated builder",
            );
        }
    }

    let lower = line.to_ascii_lowercase();
    for marker in LEAK_DEBT_MARKERS {
        if let Some(col) = lower.find(marker) {
            hits.push(Hit {
                col: col + 1,
                message: format!(
                    "`{}` in native lifecycle code violates D18 - model ownership instead of documenting a leak",
                    marker
                ),
                suggested: "release/unregister through the native lifecycle or track the staged fix in a GitHub issue labeled status:staged".to_string(),
            });
        }
    }

    if path_s.contains("/android/") && in_loop && !is_comment && line.contains(".nextUpdate(") {
        hits.push(Hit {
            col: line.find(".nextUpdate(").unwrap_or(0) + 1,
            message: "`nextUpdate` inside a native loop violates D18 - Android must use push callbacks, not polling drains".to_string(),
            suggested: "match the iOS update-callback model so native receives snapshots instead of polling".to_string(),
        });
    }

    hits
}

fn push_token_hits(hits: &mut Vec<Hit>, line: &str, token: &str, message: &str) {
    let mut start = 0;
    while let Some(rel) = line[start..].find(token) {
        let col = start + rel;
        hits.push(Hit {
            col: col + 1,
            message: format!("`{}` {}", token.trim_end_matches('('), message),
            suggested: "move policy/state ownership to Rust; keep native to rendering or raw capability execution".to_string(),
        });
        start = col + token.len();
    }
}

#[derive(Default)]
struct LoopTracker {
    cur_depth: i32,
    loop_depths: Vec<i32>,
    pending_loop: bool,
}

impl LoopTracker {
    fn in_loop(&self) -> bool {
        !self.loop_depths.is_empty()
    }

    fn line_opens_loop(&self, line: &str, starts_in_block_comment: bool) -> bool {
        !starts_in_block_comment && is_loop_opener(line)
    }

    fn observe_line(&mut self, line: &str, starts_in_block_comment: bool) {
        if starts_in_block_comment {
            return;
        }
        let opens_loop = is_loop_opener(line);
        let (opens, closes) = count_braces_ignoring_strings(line);
        if (self.pending_loop || opens_loop) && opens > 0 {
            self.loop_depths.push(self.cur_depth);
            self.pending_loop = false;
        } else if opens_loop {
            self.pending_loop = true;
        } else if !line.trim().is_empty() && !line.trim_start().starts_with('@') {
            self.pending_loop = false;
        }

        self.cur_depth += opens as i32;
        self.cur_depth -= closes as i32;
        while let Some(&top) = self.loop_depths.last() {
            if self.cur_depth <= top {
                self.loop_depths.pop();
            } else {
                break;
            }
        }
    }
}

fn is_loop_opener(line: &str) -> bool {
    let code = line.split("//").next().unwrap_or(line).trim_start();
    code.starts_with("while ")
        || code.starts_with("while(")
        || code.starts_with("while (")
        || code.starts_with("for ")
        || code.starts_with("for(")
        || code.starts_with("for (")
        || code.starts_with("repeat {")
}

fn update_block_comment(line: &str, state: &mut bool) {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if *state {
            if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                *state = false;
                i += 2;
                continue;
            }
        } else if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            *state = true;
            i += 2;
            continue;
        }
        i += 1;
    }
}

fn is_allowlisted(path: &Path, line: &str) -> bool {
    let path_s = path.to_string_lossy().replace('\\', "/");
    for raw in ALLOWLIST.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut fields = raw.split('\t');
        let Some(allowed_path) = fields.next() else {
            continue;
        };
        let Some(needle) = fields.next() else {
            continue;
        };
        if path_s.ends_with(allowed_path) && line.contains(needle) {
            return true;
        }
    }
    false
}
