//! Unit tests for D26 — no ambient authority in protocol/command code.

use super::*;
use std::path::Path;

// ── scope ─────────────────────────────────────────────────────────────────────

#[test]
fn nip_crate_in_both_scopes() {
    let p = Path::new("crates/nmp-nip57/src/lnurl/mod.rs");
    assert!(app_host_in_scope(p));
    assert!(active_local_keys_in_scope(p));
}

#[test]
fn protocol_crate_in_both_scopes() {
    for c in [
        "nmp-marmot",
        "nmp-blossom",
        "nmp-router",
        "nmp-wot",
        "nmp-content",
    ] {
        let p = crate_src_path(c);
        assert!(app_host_in_scope(&p), "{c} must be in app_host scope");
        assert!(active_local_keys_in_scope(&p), "{c} must be in alk scope");
    }
}

fn crate_src_path(crate_name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("crates/{crate_name}/src/lib.rs"))
}

#[test]
fn core_command_module_in_app_host_scope_only() {
    // nmp-core protocol-command modules: AppHost banned, active_local_keys NOT
    // (the port + identity-state + plumbing legitimately live in nmp-core).
    for p in [
        "crates/nmp-core/src/substrate/protocol.rs",
        "crates/nmp-core/src/substrate/protocol/capabilities.rs",
        "crates/nmp-core/src/actor/commands/identity.rs",
    ] {
        let path = Path::new(p);
        assert!(app_host_in_scope(path), "{p} must be in app_host scope");
        assert!(
            !active_local_keys_in_scope(path),
            "{p} must NOT be in active_local_keys scope (legit port/plumbing)"
        );
    }
}

#[test]
fn app_host_definition_out_of_app_host_scope() {
    // The AppHost DEFINITION names AppHost legitimately — never in scope.
    let p = Path::new("crates/nmp-core/src/substrate/app_host/mod.rs");
    assert!(!app_host_in_scope(p));
    assert!(!active_local_keys_in_scope(p));
}

#[test]
fn composition_root_out_of_scope() {
    for p in [
        "apps/chirp/crates/nmp-app-chirp/src/ffi/register.rs",
        "crates/nmp-native-runtime/src/builder.rs",
        "crates/nmp-browser-runtime/src/builder.rs",
    ] {
        let path = Path::new(p);
        assert!(
            !app_host_in_scope(path),
            "{p} (composition root) must be out of scope"
        );
        assert!(
            !active_local_keys_in_scope(path),
            "{p} (composition root) must be out of scope"
        );
    }
}

#[test]
fn unrelated_core_files_out_of_scope() {
    // nmp-core files that are neither command modules nor the app_host def.
    for p in [
        "crates/nmp-core/src/kernel_reducer.rs",
        "crates/nmp-core/src/slots.rs",
        "crates/nmp-core/src/actor/dispatch.rs",
    ] {
        let path = Path::new(p);
        assert!(
            !app_host_in_scope(path),
            "{p} must be out of app_host scope"
        );
        assert!(
            !active_local_keys_in_scope(path),
            "{p} must be out of alk scope"
        );
    }
}

#[test]
fn lint_source_out_of_scope() {
    let p = Path::new("crates/nmp-testing/bin/doctrine-lint/rules/d26.rs");
    assert!(!app_host_in_scope(p));
    assert!(!active_local_keys_in_scope(p));
}

// ── AppHost token matching ──────────────────────────────────────────────────

fn run(line: &str, app_host: bool, alk: bool) -> Vec<(usize, String, String)> {
    check(line, app_host, alk, false, false)
}

#[test]
fn flags_app_host_impl_bound() {
    let hits = run("    pub fn register(host: &impl AppHost) {", true, false);
    assert_eq!(hits.len(), 1, "AppHost bound must fire");
    assert!(hits[0].1.contains("D26"));
    assert!(hits[0].1.contains("composition super-trait"));
}

#[test]
fn flags_app_host_use_import() {
    let hits = run("use crate::substrate::AppHost;", true, false);
    assert_eq!(hits.len(), 1, "importing AppHost must fire");
}

#[test]
fn flags_app_host_generic_bound() {
    let hits = run("fn wire<H: AppHost>(host: &H) {}", true, false);
    assert_eq!(hits.len(), 1);
}

#[test]
fn does_not_flag_app_host_substring() {
    // Boundary-anchored: longer identifiers containing the token never fire.
    assert!(run("    let x: AppHostImpl = make();", true, false).is_empty());
    assert!(run("    impl MyAppHost for T {}", true, false).is_empty());
    assert!(run("    host: &impl HostCapabilities,", true, false).is_empty());
}

#[test]
fn does_not_flag_app_host_in_trailing_comment() {
    // The "this crate does NOT take AppHost" annotation pattern.
    let line = "    fn register(h: &impl IngestParserRegistrar) {} // not AppHost";
    assert!(run(line, true, false).is_empty());
}

#[test]
fn app_host_not_flagged_when_out_of_app_host_scope() {
    assert!(run("fn register(host: &impl AppHost) {", false, false).is_empty());
}

// ── active_local_keys token matching ────────────────────────────────────────

#[test]
fn flags_active_local_keys_call() {
    let hits = run("        let keys = ctx.active_local_keys();", false, true);
    assert_eq!(hits.len(), 1, "ctx.active_local_keys() reach must fire");
    assert!(hits[0].1.contains("signer-session port"));
}

#[test]
fn flags_bareword_active_local_keys() {
    let hits = run("    let k = active_local_keys();", false, true);
    assert_eq!(hits.len(), 1);
}

#[test]
fn does_not_flag_active_local_keys_substring() {
    // A longer identifier ending in / containing the token must not fire.
    assert!(run("    let x = prev_active_local_keys_slot;", false, true).is_empty());
    assert!(run("    self.active_local_keys_cache.get();", false, true).is_empty());
}

#[test]
fn active_local_keys_not_flagged_when_out_of_scope() {
    assert!(run("let k = ctx.active_local_keys();", true, false).is_empty());
}

// ── shared suppression ──────────────────────────────────────────────────────

#[test]
fn comment_line_never_fires() {
    assert!(check(
        "//! takes a narrow trait, never AppHost",
        true,
        true,
        true,
        false
    )
    .is_empty());
    assert!(check(
        "/// active_local_keys is reached via the port",
        true,
        true,
        true,
        false
    )
    .is_empty());
}

#[test]
fn test_cfg_never_fires() {
    let line = "    fn active_local_keys(&self) -> Option<Keys> { None }";
    assert!(check(line, true, true, false, true).is_empty());
    assert!(check("    host: &impl AppHost,", true, true, false, true).is_empty());
}

#[test]
fn both_tokens_on_one_line_fire_sorted() {
    let line = "    fn x(h: &impl AppHost) { let k = ctx.active_local_keys(); }";
    let hits = run(line, true, true);
    assert_eq!(hits.len(), 2, "both bans fire");
    assert!(hits[0].0 < hits[1].0, "findings sorted by column");
}
