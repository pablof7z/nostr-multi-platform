//! Doctrine-lint — grep-based static analyzer enforcing D0/D6/D7/D8/D9/D10/D11/D12/D13/D14/D15/D16/D17/D18/D19/D20/D21/D23/D24/D25/D26/D27.
//!
//! See `walker.rs` for the `#[cfg(test)]` module tracker, `allow.rs` for the
//! per-line opt-out comment, and `rules/{d0,d6,d7,d8,d9,d10,d11,d12,d13,d14,d15,d16,d17,d18,d19,d20,d21,d23,d24,d25,d26,d27}.rs` for
//! individual rule definitions. Brainstorm item #8 in
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
//!
//! # Workspace-wide D18 native shell scan (Swift/Kotlin/Java)
//! cargo run -p nmp-testing --bin doctrine-lint -- --workspace-native
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
mod cli;
mod event_flow_gates;
mod header_scan_a6;
mod report;
mod rules;
mod scope;
mod walker;

use std::env;
use std::path::Path;
use std::process::ExitCode;

use cli::{parse_args, resolve_roots};
use rules::{
    d0, d10, d11, d12, d13, d14, d15, d16, d17, d18, d19, d20, d21, d26, d27, d6, d7, d8, d9,
    no_raw_tap_reintroduction,
};
use scope::{
    d10_file_in_scope, d12_file_in_scope, d13_file_extra_in_scope,
    d14_file_in_scope, d15_file_in_scope, d16_file_in_scope, d17_file_in_scope, d19_file_in_scope,
    d20_file_in_scope, d21_file_in_scope, d26_active_local_keys_in_scope, d26_app_host_in_scope,
    d27_file_in_scope, d9_file_in_scope, is_doctrine_lint_source,
};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let cfg = match parse_args(&args[1..]) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("doctrine-lint: {}", e);
            eprintln!();
            eprintln!("{}", cli::USAGE);
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

    if cfg.workspace_native {
        for root in &roots {
            let files = match d18::collect_native_files(root) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("doctrine-lint: failed to walk {}: {}", root.display(), e);
                    return ExitCode::from(2);
                }
            };
            for path in &files {
                if let Err(e) = d18::scan_file(path, &mut all_findings) {
                    eprintln!("doctrine-lint: failed to read {}: {}", path.display(), e);
                    return ExitCode::from(2);
                }
            }
        }
        return report::finish(
            roots.len(),
            "D18 native doctrine",
            cfg.allow_findings,
            all_findings,
        );
    }

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
                &cfg.d12_extra_scopes,
                &cfg.d13_extra_scopes,
                &cfg.d14_extra_scopes,
                &cfg.d15_extra_scopes,
                &cfg.d16_extra_scopes,
                &cfg.d17_extra_scopes,
                &cfg.d19_extra_scopes,
                &cfg.d20_extra_scopes,
                &cfg.d21_extra_scopes,
                &cfg.d23_extra_scopes,
                &cfg.d24_extra_scopes,
                &cfg.d25_extra_scopes,
                &cfg.d26_extra_scopes,
                &cfg.d27_extra_scopes,
                cfg.workspace_d8,
                &mut all_findings,
            ) {
                eprintln!("doctrine-lint: failed to read {}: {}", path.display(), e);
                return ExitCode::from(2);
            }
        }

        // A6 (banned schema-less snapshot lane) scans `.rs` + `.h` in `header_scan_a6.rs`.
        if !header_scan_a6::scan_root_for_a6(root, &cfg, &mut all_findings) {
            return ExitCode::from(2);
        }
    }

    let rules = if cfg.workspace_d8 {
        "D8 no-polling"
    } else {
        "A6/D0/D6/D7/D8/D9/D10/D11/D12/D13/D14/D15/D16/D17/D19/D20/D21/D23/D24/D25/D26/D27/no_raw_tap"
    };
    report::finish(roots.len(), rules, cfg.allow_findings, all_findings)
}


/// Scan one file, appending findings.
///
/// When `workspace_d8` is true the file belongs to a `--workspace-d8` scan:
/// only the D8 *no-polling* check runs. D0/D6/D7 and the hot-path half of D8
/// are `nmp-core`-scoped rules and would flood false positives across the
/// rest of the workspace, so they are skipped entirely in that mode.
// One `extra_scopes` slice per doctrine rule — bundling them into a struct
// would obscure the 1:1 rule correspondence and make call-sites harder to read.
#[allow(clippy::too_many_arguments)]
fn scan_one_file(
    path: &Path,
    d8_extra_scopes: &[String],
    d9_extra_scopes: &[String],
    d10_extra_scopes: &[String],
    d12_extra_scopes: &[String],
    d13_extra_scopes: &[String],
    d14_extra_scopes: &[String],
    d15_extra_scopes: &[String],
    d16_extra_scopes: &[String],
    d17_extra_scopes: &[String],
    d19_extra_scopes: &[String],
    d20_extra_scopes: &[String],
    d21_extra_scopes: &[String],
    d23_extra_scopes: &[String],
    d24_extra_scopes: &[String],
    d25_extra_scopes: &[String],
    d26_extra_scopes: &[String],
    d27_extra_scopes: &[String],
    workspace_d8: bool,
    findings: &mut Vec<report::Finding>,
) -> std::io::Result<()> {
    let d0_exempt = d0::file_is_exempt(path);
    let d6_test_file = d6::file_is_test_only(path);
    let no_raw_tap_in_scope = no_raw_tap_reintroduction::file_in_scope(path);
    let d7_in_scope = d7::file_in_scope(path);
    let d8_in_scope = d8::file_in_scope(path, d8_extra_scopes);
    let d9_in_scope = d9_file_in_scope(path, d9_extra_scopes);
    let d10_in_scope = d10_file_in_scope(path, d10_extra_scopes);
    let d12_in_scope = d12_file_in_scope(path, d12_extra_scopes);
    // D13 Part A scope: default files + marker-driven opt-in + extra
    // scopes (the fixture smoke test uses the last). The marker check
    // requires the file body, so resolve it once up-front by reading the
    // file — small price, same shape the other rules pay via observed
    // line scans (the walker reads the file anyway; this just peeks first
    // to set scope). On read error, fall back to "not in scope" so the
    // outer walker emits its own better-formatted IO error below.
    let d13_part_a_in_scope = {
        let default = d13::file_in_part_a_default(path);
        let extra = d13_file_extra_in_scope(path, d13_extra_scopes);
        // The marker-based opt-in reads the file body and fires when it
        // contains `PART_A_MARKER`. Exempt doctrine-lint's own rule source
        // (which contains the marker string in doc comments) to prevent
        // meta-false-positives on broad `--path crates/` sweeps.
        let marker = !is_doctrine_lint_source(path)
            && std::fs::read_to_string(path)
                .map(|s| s.contains(d13::PART_A_MARKER))
                .unwrap_or(false);
        default || extra || marker
    };
    let d13_part_b_in_scope = d13::file_in_part_b_scope(path);
    let d14_in_scope = d14_file_in_scope(path, d14_extra_scopes);
    let d15_in_scope = d15_file_in_scope(path, d15_extra_scopes);
    let d16_in_scope = d16_file_in_scope(path, d16_extra_scopes);
    let d17_in_scope = d17_file_in_scope(path, d17_extra_scopes);
    // D19 — display-formatting banned from kernel projection builders.
    // Scope is `kernel/update/`, `kernel/types.rs`, `kernel/publish_outbox.rs`.
    let d19_in_scope = d19_file_in_scope(path, d19_extra_scopes);
    // D20 — no raw `std::time::Instant`/`SystemTime` on the wasm-compiled path.
    // Scope is the wasm-reachable crates (minus the two time shims and the
    // native-only actor/relay_worker/lmdb subtrees).
    let d20_in_scope = d20_file_in_scope(path, d20_extra_scopes);
    // D21 — no ambient authority (K2 / ADR-0052 §D6 regression gate).
    // Scope is the K2 blast-radius crates (where the five deleted process-
    // globals + two read-once-config residuals lived).
    let d21_in_scope = d21_file_in_scope(path, d21_extra_scopes);
    // D26 — no ambient authority in protocol/command code (Workstream D item 7;
    // K2 + D6 lock-in). Two tokens with distinct scopes: `AppHost` (protocol-
    // command surface incl. nmp-core command modules, minus the AppHost def +
    // composition root) and `active_local_keys` (protocol-command impl crates
    // only; nmp-core hosts the legit capability port).
    let d26_app_host_scope = d26_app_host_in_scope(path, d26_extra_scopes);
    let d26_alk_scope = d26_active_local_keys_in_scope(path, d26_extra_scopes);
    // D27 — banned display helpers in projection/snapshot/FFI serialization.
    // ADR-0032-deferred lint: catches pubkey-formatters, timestamp-formatters,
    // and precomputed *_label/*_display String fields in protocol-crate code
    // (nmp-core projection paths, nmp-nip*, nmp-marmot).
    let d27_in_scope = d27_file_in_scope(path, d27_extra_scopes);
    // D23/D24/D25 — event-flow spine locks (wiring + state in event_flow_gates).
    let ef_scope =
        event_flow_gates::FileScope::resolve(path, d23_extra_scopes, d24_extra_scopes, d25_extra_scopes);
    let mut ef_state = event_flow_gates::ScanState::default();
    let mut d6_state = d6::State::default();
    let mut d8_tracker = d8::HotPathTracker::default();
    let mut d10_tracker = d10::PrivatePublishTracker::default();
    let mut d11_tracker = d11::FnTracker::default();
    // D14 needs a running enclosing-struct identifier on every line, so it
    // carries its own tracker. Updated unconditionally (in lockstep with the
    // walker) so the in-scope check doesn't desync the brace counter.
    let mut d14_tracker = d14::StructTracker::default();
    let mut d15_state = d15::State::default();
    // D12 is a per-FILE scan rather than a per-line one (the rule needs to
    // know whether the WHOLE file ever calls `record_action_stage`), so we
    // collect a parallel `is_comment` mask during the walk and run the
    // per-file scan after.
    let mut d12_line_is_comment: Vec<bool> = Vec::new();

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
        let in_nmp_app_extern_fn = d11_tracker.in_nmp_app_extern_fn();
        d11_tracker.observe_line(sl.text, false);
        // D12 — collect the per-line `is_comment` mask so the per-file
        // scan downstream can skip doc-comments that happen to name the
        // marker function. Cheap (one bool per line); only used when
        // d12_in_scope.
        if d12_in_scope {
            d12_line_is_comment.push(sl.is_comment);
        }
        // D14 — observe every line for the same reason as the D8 tracker:
        // the brace counter must stay aligned with the file. The check
        // fires only when the file is in scope (kernel/actor/FFI substrate).
        d14_tracker.observe_line(sl.text, sl.is_comment);

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
        //
        // Escape hatch: D10 uses its OWN tightened parser
        // [`d10::line_allows_d10`] that REQUIRES a non-whitespace reason
        // after the separator. The generic `allow::line_allows` (which
        // accepts a bare `// doctrine-allow: D10`) is intentionally NOT
        // used here — every D10 escape must carry a written justification
        // a reviewer can audit. Other rules keep the lenient parser until
        // they opt in to their own per-rule variant.
        if !workspace_d8 && d10_in_scope {
            for (col, msg, suggested) in d10::check(sl.text, sl.is_comment, in_d10_marked_fn) {
                if d10::line_allows_d10(sl.text) {
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
        // D11 — one door per publish capability. Every user/app-authored
        // publish-engine event goes through `nmp_app_dispatch_action`;
        // bespoke event-producing `nmp_app_*` FFI must stay deleted.
        // Exempt doctrine-lint's own rule source (contains banned patterns
        // as string constants → meta-false-positives on broad sweeps).
        if !workspace_d8 && !is_doctrine_lint_source(path) {
            for (col, msg, suggested) in d11::check(sl.text, sl.is_comment, in_nmp_app_extern_fn) {
                if allow::line_allows(sl.text, d11::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d11::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D13 — DM-path raw-key isolation (ADR-0026). Part A fires inside
        // marked DM / zap / NIP-44 files; Part B fires on any read of
        // `mls_local_nsec` outside the marmot crate. Both halves are
        // workspace-wide-relevant correctness rules (a leaked raw nsec is
        // a leak everywhere), so they run regardless of `--workspace-d8`.
        if d13_part_a_in_scope {
            for (col, msg, suggested) in d13::check_part_a(sl.text, sl.is_comment, sl.in_test_cfg) {
                if allow::line_allows(sl.text, d13::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d13::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        if d13_part_b_in_scope {
            for (col, msg, suggested) in d13::check_part_b(sl.text, sl.is_comment) {
                if allow::line_allows(sl.text, d13::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d13::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D14 — typed snapshot-projection slots on `NmpApp` / `Kernel` /
        // `Actor*` structs. Path-scoped to `crates/nmp-core/src/` because
        // the rule disciplines the kernel/actor/FFI substrate, not user
        // code or app-layer crates. Skipped in --workspace-d8 (no-polling
        // sweep only).
        //
        // `#[cfg(test)] mod tests` blocks within scoped files are exempt:
        // the walker's `in_test_cfg` flag covers inline test modules, and
        // a fully test-only file gets the `d6_test_file` exemption (the
        // same `mod tests;` declared in a parent module).
        if !workspace_d8 && d14_in_scope && !d6_test_file && !sl.in_test_cfg {
            for (col, msg, suggested) in
                d14::check(sl.text, sl.is_comment, d14_tracker.current_struct())
            {
                if allow::line_allows(sl.text, d14::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d14::ID,
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
        // must observe every line of the in-scope file — even comment
        // lines — so brace counting stays accurate even when invocation
        // detection is suppressed. Skipped in --workspace-d8 (no-polling
        // sweep only).
        if !workspace_d8 && d15_in_scope {
            for (col, msg, suggested) in d15::check(&mut d15_state, path, sl.text, sl.is_comment) {
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
            // Even when D15 reporting is suppressed (e.g. --workspace-d8)
            // we still observe lines so the state stays in sync within a
            // file — important if a future driver pass interleaves modes.
            let _ = d15::check(&mut d15_state, path, sl.text, sl.is_comment);
        }
        // D16 — snapshot-projection keys in `apps/chirp/` must use the `nmp.`
        // prefix. Bare `"nip17.*"` / `"nip29.*"` literals are banned. Scope is
        // `apps/chirp/` Rust source. The explicitly whitelisted paths
        // (`nmp-nip29/src/interest.rs`, `nmp-nip17/src/inbox.rs`) carry stable
        // hash seeds that share the bare-prefix shape but are NOT projection
        // keys — they bypass the scope check via `d16::file_is_allowlisted`.
        // Skipped in --workspace-d8 (no-polling sweep only).
        if !workspace_d8 && d16_in_scope && !d16::file_is_allowlisted(path) {
            for (col, msg, suggested) in d16::check(sl.text, sl.is_comment) {
                if allow::line_allows(sl.text, d16::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d16::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D17 — social-timeline kind policy ({1,6}) must not be hardcoded in
        // nmp-core substrate (V-68 regression guard). Scope is
        // `crates/nmp-core/src/` excluding test code. The two-layer test
        // exemption mirrors D14: `d6_test_file` covers files whose
        // `#[cfg(test)]` gate lives in the parent module, and `sl.in_test_cfg`
        // covers inline `#[cfg(test)] mod tests` blocks. Skipped in
        // --workspace-d8 (no-polling sweep only).
        if !workspace_d8 && d17_in_scope && !d6_test_file && !sl.in_test_cfg {
            for (col, msg, suggested) in d17::check(sl.text, sl.is_comment) {
                if allow::line_allows(sl.text, d17::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d17::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D19 — display formatting banned from kernel projection builders.
        // ADR-0032 (V-115): `crate::display::` and `format_timestamp` must
        // not appear in `kernel/update/`, `kernel/types.rs`, or
        // `kernel/publish_outbox.rs`. Test-only files and #[cfg(test)] bodies
        // are exempt. Skipped in --workspace-d8 (no-polling sweep only).
        if !workspace_d8 && d19_in_scope && !d6_test_file {
            for (col, msg, suggested) in d19::check(sl.text, sl.is_comment, sl.in_test_cfg) {
                if allow::line_allows(sl.text, d19::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d19::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D20 — no raw `std::time::Instant`/`SystemTime` on the wasm-compiled
        // path (#1173, #1161). `std::time::*::now()` PANICS on wasm32; the
        // wasm-reachable crates must import from the `crate::time` web-time
        // shim. Scope is the wasm-reachable crates minus the two shims and the
        // native-only actor/relay_worker/lmdb subtrees. Test-only files
        // (`d6_test_file`) and #[cfg(test)] bodies (`sl.in_test_cfg`) are
        // exempt — tests never run on wasm32. Skipped in --workspace-d8.
        if !workspace_d8 && d20_in_scope && !d6_test_file {
            for (col, msg, suggested) in d20::check(sl.text, sl.is_comment, sl.in_test_cfg) {
                if allow::line_allows(sl.text, d20::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d20::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D21 — no ambient authority (K2 / ADR-0052 §D6 regression gate).
        // Bans module-/block-level `static`/`OnceLock`/`Lazy`/`lazy_static!`
        // holding non-const, interior-mutable, process-wide authority (handles,
        // runtimes, senders, hooks) — the shape of the five globals K2 deleted.
        // Type-scoped: `OnceLock`/`Lazy` of plain read-once config (`bool`,
        // `PathBuf`, `Regex`, …) is NOT authority, so the two benign residuals
        // never trip. Scope is the K2 blast-radius crates. Test-only files
        // (`d6_test_file`) and #[cfg(test)] bodies (`sl.in_test_cfg`) are exempt.
        // Skipped in --workspace-d8 (no-polling sweep only). Like D10, D21 uses
        // its OWN tightened parser [`d21::line_allows_d21`] that REQUIRES a
        // written reason — a bare `// doctrine-allow: D21` does NOT silence it.
        if !workspace_d8 && d21_in_scope && !d6_test_file {
            for (col, msg, suggested) in d21::check(sl.text, sl.is_comment, sl.in_test_cfg) {
                if d21::line_allows_d21(sl.text) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d21::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D26 — no ambient authority in protocol/command code (Workstream D
        // item 7). Bans `AppHost` (the broad super-trait — narrow protocol
        // modules must take the specific registrar/capability traits) and a
        // protocol command reaching `active_local_keys` (raw signing keys —
        // must sign via the signer-session port). Comments + #[cfg(test)] +
        // test-only files exempt; skipped in --workspace-d8. Reason-REQUIRED
        // `// doctrine-allow: D26 — reason` opt-out (the D10/D21/F idiom).
        if !workspace_d8 && (d26_app_host_scope || d26_alk_scope) && !d6_test_file {
            for (col, msg, suggested) in d26::check(
                sl.text,
                d26_app_host_scope,
                d26_alk_scope,
                sl.is_comment,
                sl.in_test_cfg,
            ) {
                if allow::line_allows_with_reason(sl.text, d26::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d26::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D27 — banned display helpers in projection / snapshot / FFI paths.
        // ADR-0032-deferred lint (see issue #1679). Catches pubkey-formatters
        // (`short_npub`, `to_npub`, etc.) and precomputed `*_label`/`*_display`
        // String struct fields in nmp-core projection paths, nmp-nip*, and
        // nmp-marmot. Test-only files (`d6_test_file`) and `#[cfg(test)]` bodies
        // (`sl.in_test_cfg`) are exempt. Skipped in --workspace-d8 (no-polling
        // sweep only — this is a protocol-layer structural correctness rule).
        if !workspace_d8 && d27_in_scope && !d6_test_file && !is_doctrine_lint_source(path) {
            for (col, msg, suggested) in d27::check(sl.text, sl.is_comment, sl.in_test_cfg) {
                if allow::line_allows(sl.text, d27::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: d27::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
        }
        // D23/D24/D25 — event-flow spine locks. Stateful D23 split-chain
        // tracking + the per-line D24/D25 checks live in event_flow_gates;
        // test-only files + #[cfg(test)] bodies exempt; skipped in --workspace-d8.
        event_flow_gates::scan_line(
            &ef_scope,
            &mut ef_state,
            path,
            &sl,
            workspace_d8,
            d6_test_file,
            findings,
        );
        // no_raw_tap — bans re-introduction of the deleted raw event tap. Workspace-wide;
        // test-only files / #[cfg(test)] bodies / --workspace-d8 sweeps are exempt.
        if !workspace_d8 && no_raw_tap_in_scope && !d6_test_file && !sl.in_test_cfg
            && !is_doctrine_lint_source(path)
        {
            for (col, msg, suggested) in no_raw_tap_reintroduction::check(
                sl.text,
                sl.is_comment,
                sl.in_test_cfg,
                no_raw_tap_reintroduction::in_sink_module(path),
            ) {
                if allow::line_allows_with_reason(sl.text, no_raw_tap_reintroduction::ID) {
                    continue;
                }
                findings.push(report::Finding {
                    rule: no_raw_tap_reintroduction::ID,
                    path: path.to_path_buf(),
                    line: sl.line_no,
                    col,
                    message: msg,
                    suggested,
                });
            }
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

    // D12 — per-file scan. A file declaring `fn is_async_completing() -> bool
    // { true }` must also call `record_action_stage` (or its kernel wrappers)
    // somewhere in the same file. Skipped in `--workspace-d8` (substrate-purity
    // rules are nmp-core scoped). Reading the file body again is the cost of
    // the per-file shape — `--workspace-d8` mode never touches this path.
    if !workspace_d8 && d12_in_scope {
        let body = std::fs::read_to_string(path)?;
        for hit in d12::scan_file(&body, &d12_line_is_comment) {
            // Honour `// doctrine-allow: D12 — reason` on the marker line.
            let marker_line = body.lines().nth(hit.line.saturating_sub(1)).unwrap_or("");
            if allow::line_allows(marker_line, d12::ID) {
                continue;
            }
            findings.push(report::Finding {
                rule: d12::ID,
                path: path.to_path_buf(),
                line: hit.line,
                col: hit.col,
                message: hit.message,
                suggested: hit.suggested,
            });
        }
    }

    Ok(())
}

// File-scope resolution helpers (`dN_file_in_scope`, `is_doctrine_lint_source`)
// live in `scope.rs` and are imported above.
