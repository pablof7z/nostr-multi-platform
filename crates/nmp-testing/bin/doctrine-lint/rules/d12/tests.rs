use super::*;

/// Helper: scan a string source as if every line were non-comment.
fn scan(src: &str) -> Vec<AsyncMarkerHit> {
    let n = src.lines().count();
    let flags = vec![false; n];
    scan_file(src, &flags)
}

#[test]
fn flags_async_marker_without_recording_call() {
    let src = "\
struct M;
impl ActionModule for M {
    fn is_async_completing() -> bool { true }
}
";
    let hits = scan(src);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].message.contains("D12"));
    assert!(hits[0].message.contains("is_async_completing"));
    assert_eq!(
        hits[0].line, 3,
        "finding points at the marker declaration line"
    );
}

#[test]
fn passes_when_recording_call_is_present() {
    let src = "\
fn drive(k: &mut Kernel) {
    k.record_action_stage(\"x\", ActionStage::Publishing, None);
}
impl ActionModule for M {
    fn is_async_completing() -> bool { true }
}
";
    let hits = scan(src);
    assert!(
        hits.is_empty(),
        "a sibling `record_action_stage` call satisfies the rule"
    );
}

#[test]
fn passes_when_record_action_failure_is_present() {
    // `record_action_failure` fans into the stage mirror — it's a
    // legitimate recording call.
    let src = "\
impl ActionModule for M {
    fn is_async_completing() -> bool { true }
}
fn fail(k: &mut Kernel) {
    k.record_action_failure(id, msg);
}
";
    let hits = scan(src);
    assert!(hits.is_empty());
}

#[test]
fn default_false_marker_is_ignored() {
    // The trait's default returns `false`. A module that doesn't override
    // it (or that explicitly writes `false`) is synchronous-by-declaration
    // — D12 does not fire.
    let src = "\
impl ActionModule for M {
    fn is_async_completing() -> bool { false }
}
";
    let hits = scan(src);
    assert!(
        hits.is_empty(),
        "a `false` marker is synchronous; no recording required"
    );
}

#[test]
fn comment_lines_are_skipped_by_the_caller() {
    // The walker masks comment lines via the parallel `line_is_comment`
    // vec. A doc-comment naming the function must not fire the rule.
    let src = "\
/// Modules with `fn is_async_completing() -> bool { true }` ...
";
    let flags = vec![true];
    let hits = scan_file(src, &flags);
    assert!(hits.is_empty());
}

#[test]
fn method_call_without_fn_is_not_a_declaration() {
    // A call site `M::is_async_completing()` is not a declaration — no
    // `fn` keyword on the same line.
    let src = "\
fn observe(_: &M) {
    let _ = M::is_async_completing();
}
";
    let hits = scan(src);
    assert!(hits.is_empty());
}

#[test]
fn contains_word_true_rejects_truncated_identifiers() {
    // The `true` in `truely` is not a literal; the boundary check filters it.
    assert!(contains_word_true("    fn f() -> bool { true }"));
    assert!(!contains_word_true(
        "    fn f() -> Truely { return Truely; }"
    ));
    assert!(!contains_word_true("    let x = intrue;"));
}

#[test]
fn file_in_scope_includes_protocol_and_app_crates() {
    assert!(file_in_scope(&Path::new(
        "crates/nmp-nip29/src/action/content.rs"
    )));
    assert!(file_in_scope(&Path::new(
        "apps/chirp/nmp-app-chirp/src/lib.rs"
    )));
    assert!(file_in_scope(&Path::new("crates/nmp-core/src/publish.rs")));
}

/// PR-G2 — codex MEDIUM "D12 multi-line bypass" finding.
///
/// A declaration whose body spans multiple lines used to slip through
/// the same-line `is_async_completing` + `true` heuristic. The
/// `PublishModule` declaration is the canonical real-world case that
/// motivated the fix; this exercises the same shape against the
/// rule's grep-level scan.
#[test]
fn flags_multi_line_async_marker_without_recording_call() {
    let src = "\
struct M;
impl ActionModule for M {
    fn is_async_completing() -> bool {
        true
    }
}
";
    let hits = scan(src);
    assert_eq!(
        hits.len(),
        1,
        "the multi-line body returning `true` must be flagged exactly once"
    );
    assert_eq!(
        hits[0].line, 3,
        "the finding must anchor on the declaration line, not the body line"
    );
}

/// A multi-line body returning `false` is NOT async-completing and
/// must NOT fire the rule — synchronous-by-declaration is the
/// default. This is the symmetric negative to
/// `flags_multi_line_async_marker_without_recording_call`.
#[test]
fn passes_multi_line_false_marker() {
    let src = "\
struct M;
impl ActionModule for M {
    fn is_async_completing() -> bool {
        false
    }
}
";
    let hits = scan(src);
    assert!(
        hits.is_empty(),
        "a multi-line `false` body must NOT fire the rule; got {hits:?}"
    );
}

/// A multi-line body with a recording-call sibling in the same file
/// passes the rule. Exercises the multi-line scanner working WITH the
/// recording-call short-circuit.
#[test]
fn passes_multi_line_async_marker_when_recording_call_is_present() {
    let src = "\
struct M;
impl ActionModule for M {
    fn is_async_completing() -> bool {
        true
    }
}
fn drive(k: &mut Kernel) {
    k.record_action_stage(\"x\", ActionStage::Publishing, None);
}
";
    let hits = scan(src);
    assert!(
        hits.is_empty(),
        "a sibling `record_action_stage` call satisfies the rule \
         even with a multi-line declaration; got {hits:?}"
    );
}

/// The trait-method DECLARATION case: a `fn is_async_completing() -> bool;`
/// inside a `trait` block has no body. The scanner must not be confused
/// into reporting a body-less declaration as async-completing.
#[test]
fn trait_method_declaration_without_body_does_not_fire() {
    let src = "\
trait ActionModule {
    fn is_async_completing() -> bool;
}
";
    let hits = scan(src);
    assert!(
        hits.is_empty(),
        "a body-less trait declaration must not fire the rule; got {hits:?}"
    );
}

/// A `true` literal in a trailing line comment must NOT make a
/// `false`-returning multi-line body look async-completing. The
/// scanner strips line comments before checking for `true`.
#[test]
fn trailing_comment_with_true_does_not_satisfy_marker() {
    let src = "\
impl ActionModule for M {
    fn is_async_completing() -> bool {
        false // never true here
    }
}
";
    let hits = scan(src);
    assert!(
        hits.is_empty(),
        "a `true` inside a trailing line comment must not satisfy the rule; got {hits:?}"
    );
}

#[test]
fn file_in_scope_excludes_nmp_testing() {
    assert!(!file_in_scope(&Path::new(
        "crates/nmp-testing/bin/doctrine-lint/fixtures/d12/neg.rs"
    )));
    assert!(!file_in_scope(&Path::new("crates/nmp-testing/src/lib.rs")));
}
