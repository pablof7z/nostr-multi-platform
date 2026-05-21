//! Doctrine-lint — grep-based static analyzer enforcing D0/D6/D7/D8/D9/D10/D15.
//!
//! See `walker.rs` for the `#[cfg(test)]` module tracker, `allow.rs` for the
//! per-line opt-out comment, and `rules/d{0,6,7,8,9,10,15}.rs` for individual
//! rule definitions. Brainstorm item #8 in
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
//!
//! # Workspace-wide D8 no-polling scan (every production crate)
//! cargo run -p nmp-testing --bin doctrine-lint -- --workspace-d8
//! ```
//!
//! ## `--workspace-d8` mode
//!
//! The hot-path-allocation and substrate-purity rules (D0/D6/D7 + the
//! hot-path half of D8) are deliberately `nmp-core`-scoped. The *no-polling*
//! half of D8 — `thread::sleep`, `tokio::time::sleep`, and
//! `tokio::time::sleep_until` are all busy-waits — is a universally
//! applicable correctness rule, so `--workspace-d8` runs **only** that check across
//! every `crates/*/src/` tree in the workspace. It skips `nmp-android-ffi`
//! (its own separate workspace) and `nmp-testing` (test-infrastructure
//! crate). `#[cfg(test)]` blocks and test-only files stay exempt, exactly as
//! in the `nmp-core` scan.
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

use rules::{d0, d10, d15, d6, d7, d8, d9};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let cfg = match parse_args(&args[1..]) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("doctrine-lint: {}", e);
            eprintln!();
            eprintln!(
                "usage: doctrine-lint [--crate <name>] [--path <dir>] [--allow-findings] \
                 [--d8-extra-scope <fragment>] [--d9-extra-scope <fragment>] \
                 [--d10-extra-scope <fragment>] [--d15-extra-scope <fragment>] \
                 [--workspace-d8 [--workspace-d8-root <dir>]]"
            );
            return ExitCode::from(2);
        }
    };

    let roots = match resolve_roots(&cfg) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("doctrine-lint: {}", e);
            return ExitCode::from(2);
        }
    };
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
            if let Err(e) = scan_one_file(
                path,
                &cfg.d8_extra_scopes,
                &cfg.d9_extra_scopes,
                &cfg.d10_extra_scopes,
                &cfg.d15_extra_scopes,
                cfg.workspace_d8,
                &mut all_findings,
            ) {
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
        let rules = if cfg.workspace_d8 {
            "D8 no-polling"
        } else {
            "D0/D6/D7/D8/D9/D10/D15"
        };
        eprintln!(
            "doctrine-lint: 0 findings across {} root(s) ({} clean).",
            roots.len(),
            rules
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

/// Scan one file, appending findings.
///
/// When `workspace_d8` is true the file belongs to a `--workspace-d8` scan:
/// only the D8 *no-polling* check runs. D0/D6/D7 and the hot-path half of D8
/// are `nmp-core`-scoped rules and would flood false positives across the
/// rest of the workspace, so they are skipped entirely in that mode.
fn scan_one_file(
    path: &Path,
    d8_extra_scopes: &[String],
    d9_extra_scopes: &[String],
    d10_extra_scopes: &[String],
    d15_extra_scopes: &[String],
    workspace_d8: bool,
    findings: &mut Vec<report::Finding>,
) -> std::io::Result<()> {
    let d0_exempt = d0::file_is_exempt(path);
    let d6_test_file = d6::file_is_test_only(path);
    let d7_in_scope = d7::file_in_scope(path);
    let d8_in_scope = d8::file_in_scope(path, d8_extra_scopes);
    let d9_in_scope = d9_file_in_scope(path, d9_extra_scopes);
    let d10_in_scope = d10_file_in_scope(path, d10_extra_scopes);
    let d15_in_scope = d15_file_in_scope(path, d15_extra_scopes);
    let mut d6_state = d6::State::default();
    let mut d8_tracker = d8::HotPathTracker::default();
    let mut d10_tracker = d10::PrivatePublishTracker::default();
    let mut d15_state = d15::State::default();

    walker::scan_file(path, |sl| {
        // D8 tracker must observe every line even when out-of-scope so its
        // brace counter stays correct relative to the file. But the actual
        // check only fires when in_scope.
        let in_marked_fn = d8_tracker.in_marked_fn();
        d8_tracker.observe_line(sl.text, false);
        // D10 tracker mirrors D8's contract: observe every line (so the
        // brace counter stays in sync) but only fire when in scope. The
        // marker-gated state is captured at line start, then advanced.
        let in_d10_marked_fn = d10_tracker.in_marked_fn();
        d10_tracker.observe_line(sl.text);

        // D0
        if !workspace_d8 && !d0_exempt {
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
        if !workspace_d8 && !d6_test_file {
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
        if !workspace_d8 && d7_in_scope {
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
        // This half of D8 is nmp-core-scoped — skipped in --workspace-d8.
        if !workspace_d8 && d8_in_scope {
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
        // D9 — protocol-crate action namespaces start with `nmp.`. Scope is
        // every `crates/nmp-*/src/` tree EXCEPT `nmp-testing` (its own
        // fixtures host intentional negative examples). Skipped in
        // --workspace-d8 (no-polling sweep only).
        if !workspace_d8 && d9_in_scope {
            for (col, msg, suggested) in d9::check(sl.text, sl.is_comment) {
                if allow::line_allows(sl.text, d9::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d9::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D10 — provenance: gift-wrap publish never escapes to public
        // relays. Scope is `crates/nmp-{core,nip17,marmot}/src/`; the
        // rule fires only inside functions opted-in via the
        // `// D10: private-kind publish` marker comment. Skipped in
        // --workspace-d8 (no-polling sweep only).
        if !workspace_d8 && d10_in_scope {
            for (col, msg, suggested) in d10::check(sl.text, sl.is_comment, in_d10_marked_fn) {
                if allow::line_allows(sl.text, d10::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d10::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D15 — host-supplied closure invocations MUST be wrapped in
        // `catch_unwind` / `guard_ffi_callback`. Scope is `nmp-core/src/`
        // (host-closure registration seams live in the substrate). The
        // check is stateful (brace-depth + guard stack), so the state
        // must observe every line of the in-scope file.
        if !workspace_d8 && d15_in_scope {
            for (col, msg, suggested) in
                d15::check(&mut d15_state, path, sl.text, sl.is_comment)
            {
                if allow::line_allows(sl.text, d15::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d15::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        } else if d15_in_scope {
            let _ = d15::check(&mut d15_state, path, sl.text, sl.is_comment);
        }
        // D8 — no polling (`thread::sleep`, `tokio::time::sleep`,
        // `tokio::time::sleep_until`). NOT path-scoped: the no-poll
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

/// True iff D9 should scan `path` — either the file is inside a protocol/
/// substrate crate (`d9::file_in_scope`), or the caller opted-in via
/// `--d9-extra-scope <fragment>` (the fixture smoke test uses this so a
/// staged fixture file under `target/<label>/` is reachable without faking a
/// `crates/nmp-*` layout).
fn d9_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d9::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D10 should scan `path` — either the file is inside one of the
/// D10-scoped trees (`crates/nmp-{core,nip17,marmot}/src/`), or the caller
/// opted-in via `--d10-extra-scope <fragment>` (the fixture smoke test
/// uses this so a staged fixture under `target/<label>/` is reachable
/// without faking a `crates/nmp-*` layout). Mirrors `d9_file_in_scope`.
fn d10_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d10::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D15 should scan `path` — either `nmp-core/src/` via
/// `d15::file_in_scope`, or the caller opted-in via `--d15-extra-scope`.
fn d15_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d15::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
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
    /// Extra path fragments treated as D9-in-scope. Same role as
    /// [`Self::d8_extra_scopes`]: lets the fixture smoke test point the
    /// rule at `bin/doctrine-lint/fixtures/d9/`, which otherwise falls
    /// outside the protocol-crate scope.
    d9_extra_scopes: Vec<String>,
    /// Extra path fragments treated as D10-in-scope. Same role as
    /// [`Self::d9_extra_scopes`]: lets the fixture smoke test point the
    /// rule at `bin/doctrine-lint/fixtures/d10/`, which otherwise falls
    /// outside the `nmp-{core,nip17,marmot}` scope.
    d10_extra_scopes: Vec<String>,
    /// Extra path fragments treated as D15-in-scope.
    d15_extra_scopes: Vec<String>,
    /// `--workspace-d8`: scan every production crate for D8 no-polling
    /// violations only. D0/D6/D7 (substrate-purity rules) stay nmp-core
    /// scoped — only the universally-applicable `thread::sleep` check runs.
    workspace_d8: bool,
    /// `--workspace-d8-root <dir>`: override the workspace root used by
    /// `--workspace-d8`. Defaults to the workspace root resolved from
    /// `CARGO_MANIFEST_DIR`. The smoke test points this at a temp tree so
    /// a positive fixture can be scanned without a real violation.
    workspace_d8_root: Option<PathBuf>,
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
                    args.get(i)
                        .ok_or_else(|| "--path requires a path".to_string())?,
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
            "--d9-extra-scope" => {
                i += 1;
                cfg.d9_extra_scopes.push(
                    args.get(i)
                        .ok_or_else(|| "--d9-extra-scope requires a path fragment".to_string())?
                        .clone(),
                );
            }
            "--d10-extra-scope" => {
                i += 1;
                cfg.d10_extra_scopes.push(
                    args.get(i)
                        .ok_or_else(|| "--d10-extra-scope requires a path fragment".to_string())?
                        .clone(),
                );
            }
            "--d15-extra-scope" => {
                i += 1;
                cfg.d15_extra_scopes.push(
                    args.get(i)
                        .ok_or_else(|| "--d15-extra-scope requires a path fragment".to_string())?
                        .clone(),
                );
            }
            "--workspace-d8" => {
                cfg.workspace_d8 = true;
            }
            "--workspace-d8-root" => {
                i += 1;
                cfg.workspace_d8_root =
                    Some(PathBuf::from(args.get(i).ok_or_else(|| {
                        "--workspace-d8-root requires a path".to_string()
                    })?));
            }
            "-h" | "--help" => {
                println!("usage: doctrine-lint [--crate <name>] [--path <dir>] [--allow-findings] [--d8-extra-scope <fragment>] [--d9-extra-scope <fragment>] [--d10-extra-scope <fragment>] [--d15-extra-scope <fragment>] [--workspace-d8 [--workspace-d8-root <dir>]]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {}", other)),
        }
        i += 1;
    }
    if cfg.workspace_d8 {
        // Workspace-d8 mode owns root resolution end-to-end — it must NOT
        // fall through to the `--crate nmp-core` default below, and mixing
        // it with `--crate` / `--path` would be ambiguous.
        if cfg.crate_name.is_some() || !cfg.explicit_paths.is_empty() {
            return Err("--workspace-d8 cannot be combined with --crate or --path".to_string());
        }
    } else {
        if cfg.workspace_d8_root.is_some() {
            return Err("--workspace-d8-root requires --workspace-d8".to_string());
        }
        if cfg.crate_name.is_none() && cfg.explicit_paths.is_empty() {
            // Default: scan nmp-core.
            cfg.crate_name = Some("nmp-core".to_string());
        }
    }
    Ok(cfg)
}

fn resolve_roots(cfg: &Config) -> Result<Vec<PathBuf>, String> {
    if cfg.workspace_d8 {
        let workspace_root = cfg
            .workspace_d8_root
            .clone()
            .unwrap_or_else(default_workspace_root);
        return workspace_crate_src_roots(&workspace_root);
    }

    let mut roots = Vec::new();
    if let Some(name) = &cfg.crate_name {
        // Best-effort: assume invocation from workspace root. CI invokes
        // exactly that way.
        roots.push(PathBuf::from(format!("crates/{}/src", name)));
    }
    for p in &cfg.explicit_paths {
        roots.push(p.clone());
    }
    Ok(roots)
}

/// Crates excluded from `--workspace-d8`:
/// - `nmp-android-ffi` — its own separate Cargo workspace, scanned by its
///   own gate; including it here double-counts and may break on its layout.
/// - `nmp-testing` — test-infrastructure crate; sleep in test harnesses and
///   benches is legitimate, mirroring the `#[cfg(test)]` exemption.
const WORKSPACE_D8_SKIP_CRATES: &[&str] = &["nmp-android-ffi", "nmp-testing"];

/// The workspace root, resolved from `CARGO_MANIFEST_DIR` (the `nmp-testing`
/// crate dir) by walking up two levels: `crates/nmp-testing` → `crates` →
/// workspace root. This makes `--workspace-d8` independent of the CWD.
fn default_workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or(manifest)
}

/// Enumerate every `<workspace_root>/crates/<name>/src/` directory, skipping
/// the crates in [`WORKSPACE_D8_SKIP_CRATES`]. Also enumerates app-layer
/// Rust crates under `apps/<app>/<crate>/src/` (one level deeper than
/// `crates/`). Returns a sorted, deterministic list. A crate with no `src/`
/// directory is silently skipped.
fn workspace_crate_src_roots(workspace_root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();

    // ── crates/<name>/src/ ────────────────────────────────────────────────────
    let crates_dir = workspace_root.join("crates");
    let entries = std::fs::read_dir(&crates_dir)
        .map_err(|e| format!("failed to read {}: {}", crates_dir.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read crates/ entry: {}", e))?;
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if WORKSPACE_D8_SKIP_CRATES.contains(&name.as_ref()) {
            continue;
        }
        let src = entry.path().join("src");
        if src.is_dir() {
            roots.push(src);
        }
    }

    // ── apps/<app>/<crate>/src/ ───────────────────────────────────────────────
    // App-layer Rust crates live one level deeper than `crates/` (the extra
    // nesting is the app name, e.g. `apps/chirp/nmp-app-chirp/src`). Walk two
    // levels: app-directory → crate-directory → src.
    let apps_dir = workspace_root.join("apps");
    if apps_dir.is_dir() {
        let app_entries = std::fs::read_dir(&apps_dir)
            .map_err(|e| format!("failed to read {}: {}", apps_dir.display(), e))?;
        for app_entry in app_entries {
            let app_entry = app_entry.map_err(|e| format!("failed to read apps/ entry: {}", e))?;
            if !app_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let crate_entries = std::fs::read_dir(app_entry.path())
                .map_err(|e| format!("failed to read {}: {}", app_entry.path().display(), e))?;
            for crate_entry in crate_entries {
                let crate_entry =
                    crate_entry.map_err(|e| format!("failed to read app crate entry: {}", e))?;
                if !crate_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    continue;
                }
                let src = crate_entry.path().join("src");
                if src.is_dir() {
                    roots.push(src);
                }
            }
        }
    }

    roots.sort();
    Ok(roots)
}
