//! A6 scan machinery (schema-less JSON snapshot-projection lane banned).
//!
//! A6 bans the schema-less JSON snapshot-projection lane. The banned C-ABI
//! symbol `nmp_app_register_snapshot_projection` can reappear in Rust sources
//! *and* in C/Obj-C header prototypes (e.g. `apps/chirp/ios/Chirp/Bridge/NmpCore.h`),
//! which a `.rs`-only walker would silently miss. This module owns the entire
//! A6 sweep — both the `.rs` walk and the `.h` header walk — so `main.rs` only
//! carries a single call site ([`scan_root_for_a6`]).
//!
//! Split out of `main.rs` to keep that file within its file-size hard cap.

use std::path::Path;

use crate::allow;
use crate::cli::Config;
use crate::report;
use crate::rules::{a6, d6};
use crate::scope::a6_file_in_scope;
use crate::walker::{self, ScannedLine};

/// Run the full A6 sweep for one scan `root`: `.rs` sources first, then `.h`
/// headers. A6 is workspace-wide (crates/ + apps/, opt-in via
/// `--a6-extra-scope`); it is *not* part of the `--workspace-d8` no-polling
/// sweep, so that mode skips A6 entirely. Returns `false` after printing a
/// diagnostic on a walk/read failure, so the caller can exit with code 2.
pub(crate) fn scan_root_for_a6(
    root: &Path,
    cfg: &Config,
    all_findings: &mut Vec<report::Finding>,
) -> bool {
    if cfg.workspace_d8 {
        return true;
    }
    let scopes = &cfg.a6_extra_scopes;
    scan_rs_for_a6(root, scopes, all_findings) && scan_headers_for_a6(root, scopes, all_findings)
}

/// Walk the Rust sources (`.rs`) under `root` for A6. Test-only files and
/// `#[cfg(test)]` bodies are exempt, matching the rest of the doctrine sweep.
fn scan_rs_for_a6(
    root: &Path,
    a6_extra_scopes: &[String],
    all_findings: &mut Vec<report::Finding>,
) -> bool {
    let rs_files = match walker::collect_rs_files(root) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("doctrine-lint: failed to walk {}: {}", root.display(), e);
            return false;
        }
    };
    for path in &rs_files {
        if !a6_file_in_scope(path, a6_extra_scopes) || d6::file_is_test_only(path) {
            continue;
        }
        let res = walker::scan_file(path, |sl| {
            if !sl.in_test_cfg {
                push_a6_line_findings(sl, path, all_findings);
            }
        });
        if let Err(e) = res {
            eprintln!("doctrine-lint: failed to read {}: {}", path.display(), e);
            return false;
        }
    }
    true
}

/// Walk the C/Obj-C header files (`.h`) under `root` and apply the A6 check.
/// Out-of-scope headers (per [`a6_file_in_scope`]) are skipped.
fn scan_headers_for_a6(
    root: &Path,
    a6_extra_scopes: &[String],
    all_findings: &mut Vec<report::Finding>,
) -> bool {
    let h_files = match walker::collect_h_files(root) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "doctrine-lint: failed to walk headers {}: {}",
                root.display(),
                e
            );
            return false;
        }
    };
    for path in &h_files {
        if !a6_file_in_scope(path, a6_extra_scopes) {
            continue;
        }
        if let Err(e) = scan_one_h_file_a6(path, all_findings) {
            eprintln!("doctrine-lint: failed to read {}: {}", path.display(), e);
            return false;
        }
    }
    true
}

/// Scan one C/Obj-C header file (`.h`) for A6 violations only.
///
/// Header files have no `#[cfg(test)]` modules, no Rust brace tracking, and no
/// `d6_test_file` concept — every non-comment line is a live production
/// declaration. [`walker::scan_h_file`] sets `in_test_cfg = false` on every
/// line, so [`push_a6_line_findings`] runs unconditionally per line.
fn scan_one_h_file_a6(path: &Path, findings: &mut Vec<report::Finding>) -> std::io::Result<()> {
    walker::scan_h_file(path, |sl| push_a6_line_findings(sl, path, findings))
}

/// Emit any A6 findings for a single scanned line. Honours the per-line allow
/// comment. Shared by the `.rs` and `.h` walks; callers gate scope/exemptions.
fn push_a6_line_findings(sl: &ScannedLine, path: &Path, findings: &mut Vec<report::Finding>) {
    for (col, msg, suggested) in a6::check(sl.text, sl.is_comment, sl.in_test_cfg) {
        if allow::line_allows(sl.text, a6::ID) {
            continue;
        }
        findings.push(report::Finding {
            rule: a6::ID,
            path: path.to_path_buf(),
            line: sl.line_no,
            col,
            message: msg,
            suggested,
        });
    }
}
