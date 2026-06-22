//! #1723 / ADR-0053 — CLI handlers for the two Rust-const generators driven by
//! the neutral projection contract: `nmp gen builtin-keys` (the kernel built-in
//! projection key set) and `nmp gen builtin-deps` (the kernel built-in
//! projection-revision dependency table). Split out of `cli.rs` to keep that
//! file under the 500-LOC hard ceiling (AGENTS.md). The dispatcher in `main.rs`
//! routes `gen builtin-keys` / `gen builtin-deps` here.

use std::path::PathBuf;

/// `nmp gen builtin-keys [--out <path>] [--check]`.
///
/// Generates `KERNEL_BUILTIN_PROJECTION_KEYS` — the Tier-2 kernel-owned built-in
/// projection key const `nmp-core` `include!`s. Driven entirely by the neutral
/// projection contract (`projection_contract::kernel_builtin_projection_keys`);
/// takes no schema stdin.
///
/// `--out` defaults to
/// `crates/nmp-core/src/kernel/update/builtin_projection_keys.generated.rs`.
/// This default is intentional: the output belongs to the framework crate
/// `nmp-core`, not to any app.
///
/// `--check` diffs against the file on disk and exits non-zero on drift. The CI
/// gate at `.github/workflows/codegen-drift.yml` uses this mode.
pub fn run_gen_builtin_keys(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut out =
        PathBuf::from("crates/nmp-core/src/kernel/update/builtin_projection_keys.generated.rs");
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                out = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a path".to_string())?;
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{help}")),
        }
        index += 1;
    }

    if check {
        let outcome = nmp_codegen::check_builtin_keys(&out).map_err(|e| e.to_string())?;
        if outcome.up_to_date {
            println!("nmp gen builtin-keys --check: ok ({})", out.display());
            Ok(())
        } else {
            let where_diff = outcome
                .first_diff_line
                .map(|n| format!(" (first differing line {n})"))
                .unwrap_or_else(|| " (file missing)".to_string());
            Err(format!(
                "builtin-keys codegen stale at {}{where_diff}.\n\
                 Regenerate with:\n  \
                 cargo run -p nmp-codegen -- gen builtin-keys",
                out.display()
            ))
        }
    } else {
        nmp_codegen::generate_builtin_keys(&out).map_err(|e| e.to_string())?;
        println!("wrote {}", out.display());
        Ok(())
    }
}

/// `nmp gen builtin-deps [--out <path>] [--check]`.
///
/// #1723 — generates `BUILTIN_PROJECTION_DEPENDENCIES`, the per-key
/// source-version dependency table `nmp-core`'s `kernel/projection_rev/mod.rs`
/// `include!`s. Driven entirely by the projection contract
/// (`projection_contract::kernel_builtin_dependencies`); takes no schema stdin.
///
/// `--out` defaults to
/// `crates/nmp-core/src/kernel/projection_rev/builtin_projection_deps.generated.rs`.
///
/// `--check` diffs against the file on disk and exits non-zero on drift. The CI
/// gate at `.github/workflows/codegen-drift.yml` uses this mode.
pub fn run_gen_builtin_deps(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut out = PathBuf::from(
        "crates/nmp-core/src/kernel/projection_rev/builtin_projection_deps.generated.rs",
    );
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                out = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a path".to_string())?;
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{help}")),
        }
        index += 1;
    }

    if check {
        let outcome = nmp_codegen::check_builtin_deps(&out).map_err(|e| e.to_string())?;
        if outcome.up_to_date {
            println!("nmp gen builtin-deps --check: ok ({})", out.display());
            Ok(())
        } else {
            let where_diff = outcome
                .first_diff_line
                .map(|n| format!(" (first differing line {n})"))
                .unwrap_or_else(|| " (file missing)".to_string());
            Err(format!(
                "builtin-deps codegen stale at {}{where_diff}.\n\
                 Regenerate with:\n  \
                 cargo run -p nmp-codegen -- gen builtin-deps",
                out.display()
            ))
        }
    } else {
        nmp_codegen::generate_builtin_deps(&out).map_err(|e| e.to_string())?;
        println!("wrote {}", out.display());
        Ok(())
    }
}

/// `nmp gen presence-keys [--out <path>] [--check]`.
///
/// #1723 — generates `DRAIN_PROJECTION_KEYS` + `CONDITIONAL_PRESENCE_KEYS`, the
/// kernel's presence-classification key sets `nmp-core`'s
/// `kernel/projection_rev/mod.rs` `include!`s, from the contract's
/// `presence_policy` column. Takes no schema stdin.
///
/// `--out` defaults to
/// `crates/nmp-core/src/kernel/projection_rev/presence_keys.generated.rs`.
///
/// `--check` diffs against the file on disk and exits non-zero on drift.
pub fn run_gen_presence_keys(args: Vec<String>, help: &str) -> Result<(), String> {
    let mut out = PathBuf::from(
        "crates/nmp-core/src/kernel/projection_rev/presence_keys.generated.rs",
    );
    let mut check = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                index += 1;
                out = args
                    .get(index)
                    .map(PathBuf::from)
                    .ok_or_else(|| "--out requires a path".to_string())?;
            }
            "--check" => check = true,
            other => return Err(format!("unknown argument {other}\n{help}")),
        }
        index += 1;
    }

    if check {
        let outcome = nmp_codegen::check_presence_keys(&out).map_err(|e| e.to_string())?;
        if outcome.up_to_date {
            println!("nmp gen presence-keys --check: ok ({})", out.display());
            Ok(())
        } else {
            let where_diff = outcome
                .first_diff_line
                .map(|n| format!(" (first differing line {n})"))
                .unwrap_or_else(|| " (file missing)".to_string());
            Err(format!(
                "presence-keys codegen stale at {}{where_diff}.\n\
                 Regenerate with:\n  \
                 cargo run -p nmp-codegen -- gen presence-keys",
                out.display()
            ))
        }
    } else {
        nmp_codegen::generate_presence_keys(&out).map_err(|e| e.to_string())?;
        println!("wrote {}", out.display());
        Ok(())
    }
}
