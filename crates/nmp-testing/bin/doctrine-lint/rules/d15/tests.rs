use super::*;
use std::path::PathBuf;

fn check_one(line: &str) -> Vec<(usize, String, String)> {
    let mut state = State::default();
    check(&mut state, &PathBuf::from("crates/nmp-core/src/foo.rs"), line, false)
}

fn check_lines(lines: &[&str]) -> Vec<(usize, usize, String)> {
    let mut state = State::default();
    let path = PathBuf::from("crates/nmp-core/src/foo.rs");
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        for (col, msg, _sug) in check(&mut state, &path, line, false) {
            out.push((i + 1, col, msg));
        }
    }
    out
}

#[test]
fn flags_bare_observer_invocation_unguarded() {
    let hits = check_one("    observer(result);");
    assert_eq!(hits.len(), 1, "bare unguarded `observer(...)` must fire");
    assert!(hits[0].1.contains("D15"), "msg must name the rule");
    assert!(hits[0].1.contains("observer"), "msg must name the binding");
}

#[test]
fn allows_observer_invocation_inside_catch_unwind_same_line() {
    let hits =
        check_one("    let _ = catch_unwind(AssertUnwindSafe(|| observer(result)));");
    assert!(
        hits.is_empty(),
        "same-line `catch_unwind` must allow the invocation; got {hits:?}"
    );
}

#[test]
fn allows_observer_invocation_inside_catch_unwind_block() {
    let hits = check_lines(&[
        "    let result = catch_unwind(AssertUnwindSafe(|| {",
        "        observer(payload);",
        "        sibling(payload);",
        "    }));",
    ]);
    assert!(
        hits.is_empty(),
        "multi-line catch_unwind block must allow inner calls; got {hits:?}"
    );
}

#[test]
fn flags_observer_invocation_after_block_closes() {
    let hits = check_lines(&[
        "    let _ = catch_unwind(AssertUnwindSafe(|| {",
        "        observer(payload);",
        "    }));",
        "    observer(payload);", // <- this one is OUTSIDE the guard
    ]);
    assert_eq!(hits.len(), 1, "the post-block invocation must fire; got {hits:?}");
    assert_eq!(hits[0].0, 4, "line number must point at the unguarded call");
}

#[test]
fn allows_guard_ffi_callback_wrap() {
    let hits =
        check_one("    guard_ffi_callback(\"site\", || callback(ctx, payload));");
    assert!(
        hits.is_empty(),
        "guard_ffi_callback must be recognised as a guard; got {hits:?}"
    );
}

#[test]
fn flags_parens_wrapped_invocation_unguarded() {
    let hits = check_one("    (self.validate)(action_json);");
    assert_eq!(hits.len(), 1, "(self.validate)(...) must fire when unguarded");
}

#[test]
fn allows_parens_wrapped_invocation_in_catch_unwind() {
    let hits =
        check_one("    catch_unwind(AssertUnwindSafe(|| (self.validate)(action_json)));");
    assert!(hits.is_empty(), "guarded (self.validate)(...) must NOT fire");
}

#[test]
fn allows_doctrine_allow_d15_optout() {
    let hits = check_one(
        "    observer(result); // doctrine-allow: D15 — fixture observer for unit test",
    );
    assert!(
        hits.is_empty(),
        "doctrine-allow opt-out must suppress the finding; got {hits:?}"
    );
}

#[test]
fn does_not_flag_my_observer_token_boundary() {
    // `my_observer(` ends with `observer(` but the leading `_` is part of
    // the identifier — the bare-name rule must not fire on it.
    let hits = check_one("    my_observer.do_something();");
    assert!(hits.is_empty(), "non-token-boundary substring must not fire");
}

#[test]
fn does_not_flag_method_call_on_observer_name() {
    // `observer.foo()` is a method call on a binding named `observer`,
    // not an invocation of the closure itself. The pattern `observer(`
    // requires `observer` immediately followed by `(`.
    let hits = check_one("    observer.on_kernel_event(event);");
    // `observer.on_kernel_event(event)` does contain `event(` — but
    // `event` is not in INVOCATION_NAMES. The line must be clean.
    assert!(
        hits.is_empty(),
        "method calls on observer-named bindings must not fire; got {hits:?}"
    );
}

#[test]
fn flags_callback_invocation_unguarded() {
    // The C-ABI shape: `(registration.callback)(ctx, payload);` — must
    // be wrapped in `guard_ffi_callback`. Unguarded → D15 finding.
    let hits = check_one("    (registration.callback)(ctx, payload);");
    assert_eq!(hits.len(), 1);
    assert!(hits[0].1.contains("registration.callback"));
}

#[test]
fn ignores_comment_lines() {
    let mut state = State::default();
    let path = PathBuf::from("crates/nmp-core/src/foo.rs");
    // Even though the comment text contains `observer(`, comments are
    // exempt — D15 is about runtime behaviour, not docs.
    let hits = check(&mut state, &path, "    // observer(result) — example", true);
    assert!(hits.is_empty(), "comment lines must never fire; got {hits:?}");
}

#[test]
fn command_drain_site_is_allowlisted() {
    let path = PathBuf::from("crates/nmp-core/src/actor/mod.rs");
    let mut state = State::default();
    let hits =
        check(&mut state, &path, "    observer(payload);", false);
    assert!(
        hits.is_empty(),
        "actor/mod.rs command-drain allowlist must suppress the finding"
    );
}

#[test]
fn file_in_scope_includes_nmp_core() {
    assert!(file_in_scope(&PathBuf::from(
        "crates/nmp-core/src/kernel/action_registry.rs"
    )));
    assert!(file_in_scope(&PathBuf::from(
        "/abs/path/crates/nmp-core/src/lib.rs"
    )));
}

#[test]
fn file_in_scope_excludes_protocol_crates() {
    assert!(!file_in_scope(&PathBuf::from(
        "crates/nmp-nip29/src/action/content.rs"
    )));
    assert!(!file_in_scope(&PathBuf::from(
        "apps/chirp/nmp-app-chirp/src/lib.rs"
    )));
}

#[test]
fn file_in_scope_excludes_doctrine_lint_bin() {
    // The rule's own source contains invocation tokens as identifiers;
    // scanning it would create self-referential false positives.
    assert!(!file_in_scope(&PathBuf::from(
        "crates/nmp-testing/bin/doctrine-lint/rules/d15.rs"
    )));
}

#[test]
fn guard_token_inside_string_does_not_open_guard_scope() {
    // A guard token embedded in a string literal MUST NOT register a
    // guard scope — `contains_outside_strings` mirrors the brace
    // counter's string-skipping rules. A subsequent unguarded
    // `observer(...)` therefore fires.
    let hits = check_lines(&[
        "    let s = \"catch_unwind(\";",
        "    observer(result);",
    ]);
    assert_eq!(
        hits.len(),
        1,
        "the post-string `observer(...)` must still fire — string-embedded \
         guard tokens do not count; got {hits:?}"
    );
    assert_eq!(hits[0].0, 2, "the finding must point at the call site line");
}
