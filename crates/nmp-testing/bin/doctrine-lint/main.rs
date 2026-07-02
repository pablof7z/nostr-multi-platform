//! Doctrine-lint — grep-based static analyzer enforcing D0/D6/D7/D8/D9/D10/D11/D12/D13/D14/D15/D17/D18/D19/D20/D21/D23/D24/D25/D26/D27 plus action_namespace/nip29_kind_blind/product_raw_read.
//!
//! D16 (bare `nip17.`/`nip29.` snapshot-projection keys under `apps/chirp/`)
//! was deleted along with the rule machinery when Chirp was extracted to its
//! own repository — `apps/chirp/` no longer exists in this monorepo, so the
//! rule's `file_in_scope` predicate could never match a real path again. See
//! `tests_d16_workspace.rs::apps_chirp_directory_does_not_exist` for the
//! tombstone gate that replaced it.
//!
//! See `walker.rs` for the `#[cfg(test)]` module tracker, `allow.rs` for the
//! per-line opt-out comment, and `rules/{d0,d6,d7,d8,d9,d10,d11,d12,d13,d14,d15,d17,d18,d19,d20,d21,d23,d24,d25,d26,d27,action_namespace,nip29_kind_blind,product_raw_read}.rs` for
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
//! every `crates/*/src/` tree in the workspace, plus `crates/nmp-testing/bin/`
//! (the perf/harness binaries, excluding doctrine-lint itself whose fixtures
//! contain intentional positive examples). It skips only `nmp-android-ffi`
//! (its own separate workspace). `#[cfg(test)]` blocks and test-only files stay
//! exempt, exactly as in the `nmp-core` scan.
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
mod scan;
mod scope;
mod walker;

use std::env;
use std::process::ExitCode;

use cli::{parse_args, resolve_roots};
use rules::d18;

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
            if let Err(e) = scan::scan_one_file(path, &cfg, &mut all_findings) {
                eprintln!("doctrine-lint: failed to read {}: {}", path.display(), e);
                return ExitCode::from(2);
            }
        }

        // A6 (banned schema-less snapshot lane) scans `.rs` + `.h` in
        // `header_scan_a6.rs`.
        if !header_scan_a6::scan_root_for_a6(root, &cfg, &mut all_findings) {
            return ExitCode::from(2);
        }
    }

    let rules = if cfg.workspace_d8 {
        "D8 no-polling"
    } else {
        "A6/D0/D6/D7/D8/D9/D10/D11/D12/D13/D14/D15/D17/D19/D20/D21/D23/D24/D25/D26/D27/action_namespace/nip29_kind_blind/no_raw_tap/product_raw_read/deleted_defaults"
    };
    report::finish(roots.len(), rules, cfg.allow_findings, all_findings)
}
