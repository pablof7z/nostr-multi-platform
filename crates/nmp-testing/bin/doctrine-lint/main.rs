//! Doctrine-lint — grep-based static analyzer enforcing D0/D6/D7/D8.
//!
//! See `walker.rs` for the `#[cfg(test)]` module tracker, `allow.rs` for the
//! per-line opt-out comment, and `rules/d{0,6,7,8}.rs` for individual rule
//! definitions. Brainstorm item #8 in
//! `docs/perf/parallel-work-brainstorm-2026-05-18.md`.
//!
//! ## Invocation
//!
//! ```bash
//! # Default: scan nmp-core
//! cargo run -p nmp-testing --bin doctrine-lint -- --crate nmp-core
//!
//! # Scan a specific path
//! cargo run -p nmp-testing --bin doctrine-lint -- --path crates/nmp-core/src
//!
//! # Scan a fixture dir (smoke tests use this)
//! cargo run -p nmp-testing --bin doctrine-lint -- --path crates/nmp-testing/bin/doctrine-lint/fixtures/d0
//! ```
//!
//! ## Exit codes
//!
//! - `0` — no findings (or `--allow-findings` was passed)
//! - `1` — at least one finding emitted
//! - `2` — usage error / IO error
//!
//! ## Output shape
//!
//! Clippy-parseable lines:
//!
//! ```text
//! crates/nmp-core/src/foo.rs:42:5: error[D6]: `.unwrap()` violates D6 — ...
//!     suggested: use `?` to propagate `Result`, or `.unwrap_or(default)` for fallible defaults
//! ```

mod allow;
mod braces;
mod report;
mod rules;
mod walker;

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rules::{d0, d6, d7, d8};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let cfg = match parse_args(&args[1..]) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("doctrine-lint: {}", e);
            eprintln!();
            eprintln!("usage: doctrine-lint [--crate <name>] [--path <dir>] [--allow-findings]");
            return ExitCode::from(2);
        }
    };

    let roots = resolve_roots(&cfg);
    let mut all_findings: Vec<report::Finding> = Vec::new();

    for root in &roots {
        let files = match walker::collect_rs_files(root) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("doctrine-lint: failed to walk {}: {}", root.display(), e);
                return ExitCode::from(2);
            }
        };
        for path in &files {
            if let Err(e) = scan_one_file(path, &cfg.d8_extra_scopes, &mut all_findings) {
                eprintln!("doctrine-lint: failed to read {}: {}", path.display(), e);
                return ExitCode::from(2);
            }
        }
    }

    // Stable order: by file, then by line, then by column.
    all_findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
    });

    for f in &all_findings {
        println!("{}", f.render());
    }

    if all_findings.is_empty() {
        eprintln!(
            "doctrine-lint: 0 findings across {} root(s) (D0/D6/D7/D8 clean).",
            roots.len()
        );
        ExitCode::from(0)
    } else if cfg.allow_findings {
        eprintln!(
            "doctrine-lint: {} finding(s) (passing because --allow-findings).",
            all_findings.len()
        );
        ExitCode::from(0)
    } else {
        eprintln!("doctrine-lint: {} finding(s).", all_findings.len());
        ExitCode::from(1)
    }
}

fn scan_one_file(
    path: &Path,
    d8_extra_scopes: &[String],
    findings: &mut Vec<report::Finding>,
) -> std::io::Result<()> {
    let d0_exempt = d0::file_is_exempt(path);
    let d6_test_file = d6::file_is_test_only(path);
    let d7_in_scope = d7::file_in_scope(path);
    let d8_in_scope = d8::file_in_scope(path, d8_extra_scopes);
    let mut d6_state = d6::State::default();
    let mut d8_tracker = d8::HotPathTracker::default();

    walker::scan_file(path, |sl| {
        // D8 tracker must observe every line even when out-of-scope so its
        // brace counter stays correct relative to the file. But the actual
        // check only fires when in_scope.
        let in_marked_fn = d8_tracker.in_marked_fn();
        d8_tracker.observe_line(sl.text, false);

        // D0
        if !d0_exempt {
            for (col, msg, suggested) in d0::check(sl.text, sl.is_comment) {
                if allow::line_allows(sl.text, d0::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d0::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D6 — `d6_test_file` short-circuits files that are themselves
        // gated via `#[cfg(test)] mod tests;` in a parent (the file body
        // has no cfg(test) attribute, so the walker can't see it). The
        // state advances even for test-only files so prev_trail stays in
        // sync with the file (cheap, keeps the check uniform).
        let d6_hits = d6::check(&mut d6_state, sl.text, sl.is_comment, sl.in_test_cfg);
        if !d6_test_file {
            for (col, msg, suggested) in d6_hits {
                if allow::line_allows(sl.text, d6::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d6::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D7
        if d7_in_scope {
            for (col, msg, suggested) in d7::check(sl.text, sl.is_comment) {
                if allow::line_allows(sl.text, d7::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d7::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D8 — hot-path allocation (path-scoped to kernel/ingest/ + bench).
        if d8_in_scope {
            for (col, msg, suggested) in d8::check_in_scope(sl.text, sl.is_comment, in_marked_fn) {
                if allow::line_allows(sl.text, d8::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d8::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D8 — no polling (`thread::sleep`). NOT path-scoped: the no-poll
        // doctrine applies to all non-test code under `nmp-core`. Reuses
        // the D6 two-layer test exemption — `d6_test_file` covers files
        // whose `#[cfg(test)]` gate lives in the parent module, and
        // `sl.in_test_cfg` covers inline `#[cfg(test)] mod tests` blocks.
        if !d6_test_file {
            for (col, msg, suggested) in
                d8::check_no_polling(sl.text, sl.is_comment, sl.in_test_cfg)
            {
                if allow::line_allows(sl.text, d8::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d8::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
    })?;

    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// CLI
// ────────────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct Config {
    crate_name: Option<String>,
    explicit_paths: Vec<PathBuf>,
    allow_findings: bool,
    /// Extra path fragments treated as D8-in-scope. Used by the fixture
    /// smoke test to point the rule at `bin/doctrine-lint/fixtures/d8/`.
    d8_extra_scopes: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut cfg = Config::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--crate" => {
                i += 1;
                cfg.crate_name = Some(
                    args.get(i)
                        .ok_or_else(|| "--crate requires a name".to_string())?
                        .clone(),
                );
            }
            "--path" => {
                i += 1;
                cfg.explicit_paths.push(PathBuf::from(
                    args.get(i).ok_or_else(|| "--path requires a path".to_string())?,
                ));
            }
            "--allow-findings" => {
                cfg.allow_findings = true;
            }
            "--d8-extra-scope" => {
                i += 1;
                cfg.d8_extra_scopes.push(
                    args.get(i)
                        .ok_or_else(|| "--d8-extra-scope requires a path fragment".to_string())?
                        .clone(),
                );
            }
            "-h" | "--help" => {
                println!("usage: doctrine-lint [--crate <name>] [--path <dir>] [--allow-findings] [--d8-extra-scope <fragment>]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {}", other)),
        }
        i += 1;
    }
    if cfg.crate_name.is_none() && cfg.explicit_paths.is_empty() {
        // Default: scan nmp-core.
        cfg.crate_name = Some("nmp-core".to_string());
    }
    Ok(cfg)
}

fn resolve_roots(cfg: &Config) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(name) = &cfg.crate_name {
        // Best-effort: assume invocation from workspace root. CI invokes
        // exactly that way.
        roots.push(PathBuf::from(format!("crates/{}/src", name)));
    }
    for p in &cfg.explicit_paths {
        roots.push(p.clone());
    }
    roots
}
