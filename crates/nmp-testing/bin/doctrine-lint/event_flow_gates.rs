//! Event-flow spine gate wiring (D23/D24/D25), extracted from `main.rs` so the
//! driver does not grow past its file-size baseline (split-don't-grow).
//!
//! These three gates lock the just-landed event-flow architecture: D23 (single
//! accepted-event store-insert chokepoint), D24 (single post-store observer
//! fan-out seam), D25 (single REQ-build door / acquisition one-door). Each is
//! scoped to `crates/nmp-core/src/` minus its sole legal call site(s), exempts
//! comments + `#[cfg(test)]` bodies + test-only files, and uses the
//! REASON-REQUIRED `// doctrine-allow:` parser ([`allow::line_allows_with_reason`],
//! the D10/D21 tightened idiom) so a reasonless allow cannot silence a finding.
//!
//! D23 is stateful (rustfmt-split `.store` / `.insert(` chain detection); D24
//! and D25 are per-line (the method-name token + `(` is atomic, so a split
//! chained call is caught with no state). This module owns the D23 [`ScanState`]
//! and the per-line dispatch the driver calls once per source line.

use std::path::Path;

use crate::allow;
use crate::report;
use crate::rules::{d23, d24, d25};
use crate::scope::{d23_file_in_scope, d24_file_in_scope, d25_file_in_scope};
use crate::walker::ScannedLine;

/// Per-file scope decision for the three event-flow gates, resolved once per
/// file (combining each rule's static scope with its `--dNN-extra-scope`
/// opt-ins, used by the fixture smoke tests).
pub(crate) struct FileScope {
    d23: bool,
    d24: bool,
    d25: bool,
}

impl FileScope {
    pub(crate) fn resolve(
        path: &Path,
        d23_extra: &[String],
        d24_extra: &[String],
        d25_extra: &[String],
    ) -> Self {
        FileScope {
            d23: d23_file_in_scope(path, d23_extra),
            d24: d24_file_in_scope(path, d24_extra),
            d25: d25_file_in_scope(path, d25_extra),
        }
    }
}

/// Cross-line scan state for the three gates (D23 receiver-split chain; D24/D25
/// method/paren split). Reset per file.
#[derive(Default)]
pub(crate) struct ScanState {
    d23: d23::State,
    d24: d24::State,
    d25: d25::State,
}

/// Scan one source line for D23/D24/D25, appending findings. Called once per
/// line from the driver's walker closure. `workspace_d8` (no-polling sweep) and
/// `d6_test_file` (test-only file) suppress all three gates wholesale.
pub(crate) fn scan_line(
    scope: &FileScope,
    state: &mut ScanState,
    path: &Path,
    sl: &ScannedLine,
    workspace_d8: bool,
    d6_test_file: bool,
    findings: &mut Vec<report::Finding>,
) {
    if workspace_d8 || d6_test_file {
        return;
    }
    if scope.d23 {
        for (col, msg, suggested) in
            d23::check(&mut state.d23, sl.text, sl.is_comment, sl.in_test_cfg)
        {
            if allow::line_allows_with_reason(sl.text, d23::ID) {
                continue;
            }
            findings.push(finding(d23::ID, path, sl.line_no, col, msg, suggested));
        }
    }
    if scope.d24 {
        for (col, msg, suggested) in
            d24::check(&mut state.d24, sl.text, sl.is_comment, sl.in_test_cfg)
        {
            if allow::line_allows_with_reason(sl.text, d24::ID) {
                continue;
            }
            findings.push(finding(d24::ID, path, sl.line_no, col, msg, suggested));
        }
    }
    if scope.d25 {
        for (col, msg, suggested) in
            d25::check(&mut state.d25, sl.text, sl.is_comment, sl.in_test_cfg)
        {
            if allow::line_allows_with_reason(sl.text, d25::ID) {
                continue;
            }
            findings.push(finding(d25::ID, path, sl.line_no, col, msg, suggested));
        }
    }
}

fn finding(
    rule: &'static str,
    path: &Path,
    line: usize,
    col: usize,
    message: String,
    suggested: String,
) -> report::Finding {
    report::Finding {
        rule,
        path: path.to_path_buf(),
        line,
        col,
        message,
        suggested,
    }
}
