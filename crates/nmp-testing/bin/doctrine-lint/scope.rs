//! File-scope resolution for the per-rule scanners.
//!
//! Each `dN_file_in_scope` helper answers "should rule DN scan this file?" by
//! combining the rule's own static scope predicate (`dN::file_in_scope`) with
//! the caller's `--dN-extra-scope <fragment>` opt-ins. The extra-scope hook is
//! used by the fixture smoke test to stage a positive fixture under `target/`
//! (outside any real `crates/nmp-*/src/` layout) and still reach the rule.
//!
//! These functions live in their own module (rather than `main.rs`) so the
//! binary entrypoint stays focused on the scan loop. Extracted to keep
//! `main.rs` within the file-size hard cap.

use std::path::Path;

use crate::rules::{
    a6, action_namespace, browser_runtime_boundary, d10, d12, d14, d15, d17, d19, d20, d21, d23,
    d24, d25, d26, d27, d6, d9, wasm_abi_only,
};

/// True iff the action-namespace prefix rule should scan `path`.
pub(crate) fn action_namespace_file_in_scope(path: &Path) -> bool {
    action_namespace::file_in_scope(path)
}

/// True iff D6 should scan `path` — either the file is inside D6's explicit
/// enforced-crate set (`d6::file_in_scope`; see that module's "Scope" doc for
/// why D6 is bounded rather than workspace-wide), or the caller opted-in via
/// `--d6-extra-scope <fragment>` (the fixture smoke test uses this so a
/// staged fixture under `target/<label>/` is reachable without faking a
/// `crates/nmp-core/src/` layout). Mirrors `d9_file_in_scope`.
pub(crate) fn d6_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d6::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D9 should scan `path` — either the file is inside a kernel time
/// policy path (`d9::file_in_scope`), or the caller opted-in via
/// `--d9-extra-scope <fragment>` (the fixture smoke test uses this so a
/// staged fixture file under `target/<label>/` is reachable without faking a
/// `crates/nmp-core` layout).
pub(crate) fn d9_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
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
pub(crate) fn d10_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d10::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D12 should scan `path` — either the file is inside a protocol/
/// substrate or app-layer crate (`d12::file_in_scope`), or the caller
/// opted-in via `--d12-extra-scope <fragment>`. Mirrors `d9_file_in_scope`
/// exactly; the smoke test uses the extra-scope flag to point the rule at
/// a fixture staged under `target/<label>/`.
pub(crate) fn d12_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d12::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D14 should scan `path` — either the file is inside
/// `crates/nmp-core/src/` (the substrate scope), or the caller opted-in via
/// `--d14-extra-scope <fragment>` (the fixture smoke test uses this so a
/// staged fixture file under `target/<label>/` is reachable without faking a
/// `crates/nmp-core/src/` layout).
pub(crate) fn d14_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d14::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D15 should scan `path` — either `nmp-core/src/` via
/// `d15::file_in_scope`, or the caller opted-in via `--d15-extra-scope`
/// (used by the fixture smoke test to stage a positive fixture under
/// `target/` outside the nmp-core path tree).
pub(crate) fn d15_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d15::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D17 should scan `path` — either the file is inside
/// `crates/nmp-core/src/` via `d17::file_in_scope`, or the caller opted-in
/// via `--d17-extra-scope` (used by the fixture smoke test to stage a
/// positive fixture under `target/` outside the nmp-core path tree). Mirrors
/// `d14_file_in_scope` exactly.
pub(crate) fn d17_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d17::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff `--d13-extra-scope` opts `path` into D13 Part A scope. Mirrors
/// `--d9-extra-scope` etc: the fixture smoke test stages a positive D13
/// fixture under `target/<label>/` (outside `crates/nmp-core/src/actor/
/// commands/dm.rs`) and uses this hook to reach it without forging a
/// fake `crates/` layout.
pub(crate) fn d13_file_extra_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D19 should scan `path` — either the file matches the kernel
/// projection-builder paths via `d19::file_in_scope`, or the caller opted-in
/// via `--d19-extra-scope <fragment>` (the fixture smoke test stages a
/// positive fixture under `target/` outside the nmp-core kernel/ path tree).
/// Mirrors `d17_file_in_scope`.
pub(crate) fn d19_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d19::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D20 should scan `path` — either the file is inside a wasm-reachable
/// crate's `src/` tree via `d20::file_in_scope` (which already exempts the two
/// time shims and the native-only `actor/**`, `relay_worker/**`,
/// `nmp-store/src/lmdb/**` subtrees), or the caller opted-in via
/// `--d20-extra-scope <fragment>` (the fixture smoke test stages a positive
/// fixture under `target/` outside any `crates/nmp-*/src/` tree). Mirrors
/// `d19_file_in_scope`.
pub(crate) fn d20_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d20::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D21 should scan `path` — either the file is inside a K2
/// blast-radius crate's `src/` tree via `d21::file_in_scope` (the crates that
/// hosted the five deleted process-global singletons plus the two read-once-
/// config residuals), or the caller opted-in via `--d21-extra-scope <fragment>`
/// (the fixture smoke test stages a positive fixture under `target/` outside any
/// `crates/nmp-*/src/` tree). Mirrors `d20_file_in_scope`.
pub(crate) fn d21_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d21::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D23 should scan `path` — either the file is inside
/// `crates/nmp-core/src/` (minus the accepted-event chokepoint file
/// `kernel/ingest/mod.rs`) via `d23::file_in_scope`, or the caller opted-in via
/// `--d23-extra-scope <fragment>` (the fixture smoke test stages a positive
/// fixture under `target/` outside the nmp-core path tree). Mirrors
/// `d21_file_in_scope`.
pub(crate) fn d23_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d23::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D24 should scan `path` — either the file is inside
/// `crates/nmp-core/src/` (minus the post-store fan-out seam files
/// `kernel/ingest/mod.rs`, `kernel/event_observer.rs`, and the
/// `kernel/cache_serve/` dir) via `d24::file_in_scope`, or the caller opted-in
/// via `--d24-extra-scope <fragment>`. Mirrors `d23_file_in_scope`.
pub(crate) fn d24_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d24::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D25 should scan `path` — either the file is inside
/// `crates/nmp-core/src/` (minus the REQ-build owners `kernel/requests/` and
/// `kernel/replay.rs`) via `d25::file_in_scope`, or the caller opted-in via
/// `--d25-extra-scope <fragment>`. Mirrors `d23_file_in_scope`.
pub(crate) fn d25_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d25::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff the D26 `AppHost` ban should scan `path` — either the file is in the
/// protocol-command surface (`d26::app_host_in_scope`: reusable protocol crates +
/// `nmp-core` protocol-command modules, minus the `AppHost` definition and
/// composition root), or the caller opted-in via `--d26-extra-scope <fragment>`
/// (the fixture smoke test stages a positive fixture under `target/`). Mirrors
/// `d21_file_in_scope`.
pub(crate) fn d26_app_host_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d26::app_host_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff the D26 `active_local_keys` ban should scan `path` — either the file
/// is in the protocol-command IMPLEMENTATION crates (`d26::active_local_keys_in_scope`;
/// `nmp-core` is excluded — it hosts the legitimate capability port), or the
/// caller opted-in via `--d26-extra-scope <fragment>`. Shares the one
/// `--d26-extra-scope` flag with the `AppHost` half so a staged fixture exercises
/// BOTH bans.
pub(crate) fn d26_active_local_keys_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d26::active_local_keys_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff A6 should scan `path` — either the file is in the A6 workspace
/// scope via `a6::file_in_scope`, or the caller opted-in via
/// `--a6-extra-scope <fragment>` (the fixture smoke test uses this so a
/// staged fixture file under `target/` is reachable without faking a
/// `crates/` layout). Mirrors `d17_file_in_scope`.
pub(crate) fn a6_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if a6::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff D27 should scan `path` — either the file is in a protocol/
/// projection-builder crate (`d27::file_in_scope`), or the caller opted-in via
/// `--d27-extra-scope <fragment>` (the fixture smoke test stages a positive
/// fixture under `target/` outside any real `crates/nmp-*/src/` layout).
/// Mirrors `d19_file_in_scope`.
pub(crate) fn d27_file_in_scope(path: &Path, extra_scopes: &[String]) -> bool {
    if d27::file_in_scope(path) {
        return true;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    extra_scopes.iter().any(|frag| s.contains(frag.as_str()))
}

/// True iff `path` is part of doctrine-lint's own source tree but NOT a
/// fixture file (fixtures are intentional negative test cases that must
/// remain scannable). Rule files in `bin/doctrine-lint/rules/` and the tool's
/// `tests.rs` contain banned tokens as string constants; scanning them on
/// `--path crates/` produces meta-false-positives that obscure real findings.
pub(crate) fn is_doctrine_lint_source(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    (s.contains("/doctrine-lint/") || s.starts_with("doctrine-lint/")) && !s.contains("/fixtures/")
}

/// True when `path` is inside the nmp-testing harness binaries (i.e. under
/// `crates/nmp-testing/bin/` or `nmp-testing/bin/` in a fake workspace) but
/// NOT inside the doctrine-lint tool itself (whose source contains intentional
/// positive-fixture strings that must never be reported as real findings).
///
/// Used by the D8 test-exemption logic: `d6::file_is_test_only` marks all
/// nmp-testing paths as test-infra (so D6 `.expect()` rules don't fire), but
/// D8 explicitly wants to scan the harness binaries when `--workspace-d8` is
/// active. This helper identifies exactly those paths.
pub(crate) fn is_nmp_testing_harness_bin(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    (s.contains("/nmp-testing/bin/") || s.contains("nmp-testing/bin/"))
        && !s.contains("/doctrine-lint/")
}

/// True iff WASM_ABI_ONLY should scan `path` — either the file is inside
/// actual ABI modules (wasm-bindgen exports, future nmp-wasm paths), or the
/// caller opted-in via `--path` to a fixture under `fixtures/wasm_abi_only/`.
/// The fixture smoke test uses this hook to stage test files outside the real
/// crates/ layout while still reaching the rule.
pub(crate) fn wasm_abi_only_file_in_scope(path: &Path) -> bool {
    if wasm_abi_only::file_is_in_scope(path) {
        return true;
    }
    // Fixture path opt-in for smoke tests.
    let s = path.to_string_lossy().replace('\\', "/");
    s.contains("/fixtures/wasm_abi_only/") || s.contains("fixtures/wasm_abi_only/")
}

/// True iff BROWSER_RUNTIME_BOUNDARY should scan `path` — either the file is
/// in a browser-runtime transport adapter or web package path, or the caller
/// opted-in via `--path` to a fixture under `fixtures/browser_runtime_boundary/`.
/// The fixture smoke test uses this hook to stage test files outside the real
/// crates/ layout while still reaching the rule.
pub(crate) fn browser_runtime_boundary_file_in_scope(path: &Path) -> bool {
    if browser_runtime_boundary::file_is_in_scope(path) {
        return true;
    }
    // Fixture path opt-in for smoke tests.
    let s = path.to_string_lossy().replace('\\', "/");
    s.contains("/fixtures/browser_runtime_boundary/") || s.contains("fixtures/browser_runtime_boundary/")
}
