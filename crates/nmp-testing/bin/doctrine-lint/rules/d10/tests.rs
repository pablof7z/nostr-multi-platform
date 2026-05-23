use super::*;
use std::path::PathBuf;

// ── file_in_scope ────────────────────────────────────────────────────

#[test]
fn file_in_scope_includes_target_crates() {
    assert!(file_in_scope(&PathBuf::from(
        "crates/nmp-core/src/actor/commands/dm.rs"
    )));
    assert!(file_in_scope(&PathBuf::from("crates/nmp-nip17/src/action.rs")));
    assert!(file_in_scope(&PathBuf::from(
        "crates/nmp-marmot/src/projection/publish.rs"
    )));
    assert!(file_in_scope(&PathBuf::from(
        "/abs/path/crates/nmp-core/src/lib.rs"
    )));
}

#[test]
fn file_in_scope_excludes_other_crates() {
    // D10 is private-kind-publish-oriented; other crates have no
    // kind:1059 publishers and stay silent.
    assert!(!file_in_scope(&PathBuf::from("crates/nmp-nip29/src/lib.rs")));
    assert!(!file_in_scope(&PathBuf::from(
        "apps/chirp/nmp-app-chirp/src/ffi.rs"
    )));
    assert!(!file_in_scope(&PathBuf::from("crates/nmp-testing/src/lib.rs")));
}

// ── PrivatePublishTracker ────────────────────────────────────────────

fn tracker_states(src: &str) -> Vec<bool> {
    // Mirror the driver's call order: observe a line, then the state
    // reported for THAT line is the state AT START of the line. Since
    // the tracker advances the brace counter inside `observe_line`,
    // we record `in_marked_fn` BEFORE the call (state at line start).
    let mut tracker = PrivatePublishTracker::default();
    let mut states = Vec::new();
    for line in src.lines() {
        states.push(tracker.in_marked_fn());
        tracker.observe_line(line);
    }
    states
}

#[test]
fn unmarked_fn_is_never_in_scope() {
    let src = "fn plain() {\n    let x = 1;\n    println!(\"{}\", x);\n}\n";
    let s = tracker_states(src);
    assert!(s.iter().all(|b| !*b), "no marker → never in scope: {:?}", s);
}

#[test]
fn marked_fn_body_is_in_scope() {
    // The marker sits inside the fn body; subsequent body lines must
    // report `in_marked_fn == true`. The `fn ... {` line itself opens
    // the scope, so its state at line start is still false.
    let src = "\
fn send() {
    // D10: private-kind publish
    let pin = recipient_dm_relays();
    publish(&envelope, &pin);
}
fn after() {
    let _ = 1;
}
";
    let s = tracker_states(src);
    // Line 1 `fn send() {` — depth 0 at start, no marker yet → false.
    assert!(!s[0]);
    // Line 2 — marker line. State BEFORE observing is still false
    // (marker not yet seen at this point); the assertion that matters
    // is the lines AFTER the marker.
    assert!(!s[1]);
    // Lines 3, 4 — inside marked fn body. True.
    assert!(s[2], "body after marker must be in scope: {:?}", s);
    assert!(s[3], "second body line must still be in scope: {:?}", s);
    // Line 5 `}` — state at start is still inside (we haven't yet
    // processed the closing brace).
    assert!(s[4]);
    // Line 6 `fn after() {` — after the marked fn closed.
    assert!(!s[5]);
    assert!(!s[6]);
}

#[test]
fn second_unmarked_fn_remains_out_of_scope() {
    let src = "\
fn send() {
    // D10: private-kind publish
    let pin = recipient_dm_relays();
}
fn normal() {
    let _ = PublishTarget::Auto;
}
";
    let s = tracker_states(src);
    // The Auto literal in `normal()` must NOT report in_marked_fn.
    assert!(!s[5], "unmarked fn body must not be in scope: {:?}", s);
}

// ── check ────────────────────────────────────────────────────────────

#[test]
fn check_silent_when_out_of_marked_fn() {
    // Outside a marked fn body, even the most blatant Auto literal is
    // ignored — that's the whole "opt-in marker" design.
    assert!(check("    target: PublishTarget::Auto,", false, false).is_empty());
    assert!(check("    kernel.publish_signed(&signed, &[])", false, false).is_empty());
}

#[test]
fn check_flags_publish_target_auto_inside_marked_fn() {
    let hits = check("        target: PublishTarget::Auto,", false, true);
    assert_eq!(hits.len(), 1, "exactly one D10 finding expected");
    assert!(
        hits[0].1.contains("PublishTarget::Auto"),
        "message must name the offending token: {}",
        hits[0].1
    );
    assert!(
        hits[0].1.contains("D10"),
        "message must mention D10: {}",
        hits[0].1
    );
}

#[test]
fn check_flags_publish_signed_auto_variant_inside_marked_fn() {
    // The `publish_signed(` form (NOT `_to`) is Auto-routing. The
    // `_to` sibling pins explicit relays and is NOT flagged.
    let hits = check("    kernel.publish_signed(&signed, &[])", false, true);
    assert_eq!(hits.len(), 1, "publish_signed( must fire D10: {:?}", hits);
}

#[test]
fn check_does_not_flag_publish_signed_to_variant() {
    // `publish_signed_to` is the Explicit-pin variant — never D10.
    let hits = check(
        "    kernel.publish_signed_to(&signed, &[], target)",
        false,
        true,
    );
    assert!(
        hits.is_empty(),
        "publish_signed_to (Explicit variant) must NOT fire D10: {:?}",
        hits
    );
}

#[test]
fn check_flags_publish_signed_with_correlation_auto_variant() {
    let hits = check(
        "        kernel.publish_signed_with_correlation(&signed, &[], None)",
        false,
        true,
    );
    assert_eq!(
        hits.len(),
        1,
        "publish_signed_with_correlation( must fire D10: {:?}",
        hits
    );
}

#[test]
fn check_does_not_flag_publish_signed_to_with_correlation() {
    // The `_to_with_correlation` variant carries an explicit
    // `PublishTarget` argument — it's the Explicit-pin variant and is
    // NOT a D10 violation.
    let hits = check(
        "    kernel.publish_signed_to_with_correlation(&signed, &[], target, None)",
        false,
        true,
    );
    assert!(
        hits.is_empty(),
        "publish_signed_to_with_correlation (Explicit variant) must NOT fire D10: {:?}",
        hits
    );
}

#[test]
fn check_flags_publish_unsigned_event_auto_variant() {
    // The Auto-routing actor command — the Explicit sibling is
    // `publish_unsigned_event_to_relays`.
    let hits = check(
        "    commands::publish_unsigned_event(identity, kernel, unsigned, ps);",
        false,
        true,
    );
    assert_eq!(
        hits.len(),
        1,
        "publish_unsigned_event( must fire D10: {:?}",
        hits
    );
}

#[test]
fn check_does_not_flag_publish_unsigned_event_to_relays() {
    let hits = check(
        "    commands::publish_unsigned_event_to_relays(id, kernel, ev, relays, ps);",
        false,
        true,
    );
    assert!(
        hits.is_empty(),
        "publish_unsigned_event_to_relays must NOT fire D10: {:?}",
        hits
    );
}

#[test]
fn check_ignores_comment_lines() {
    // The comment must NEVER fire — comments document banned tokens.
    let hits = check(
        "    /// `PublishTarget::Auto` resolves via the outbox.",
        true,
        true,
    );
    assert!(hits.is_empty(), "comment lines must never fire D10");
}

#[test]
fn check_reports_column_at_token_start() {
    // The column points at the offending substring so a developer can
    // jump straight to it.
    let line = "    target: PublishTarget::Auto,";
    let hits = check(line, false, true);
    assert_eq!(hits.len(), 1);
    let expected_col = line.find("PublishTarget::Auto").unwrap() + 1;
    assert_eq!(
        hits[0].0, expected_col,
        "column must point at the start of the offending token"
    );
}

#[test]
fn check_emits_one_finding_per_banned_token_on_a_line() {
    // A pathological "two violations on one line" must produce two
    // separate findings — each is its own structural offence.
    let line = "publish_signed(&ev, &[]); /* and */ publish_unsigned_event(id, kernel, ue, ps);";
    let hits = check(line, false, true);
    assert_eq!(
        hits.len(),
        2,
        "each banned token must produce its own finding: {:?}",
        hits
    );
}

#[test]
fn check_silent_inside_unmarked_fn_even_with_auto() {
    // The marker-gate is what scopes D10; without it the rule is
    // dormant, no matter how many Auto-routing seams a line has.
    let line = "publish_signed(&ev, &[]); /* and */ publish_unsigned_event(id, kernel, ue, ps);";
    let hits = check(line, false, false);
    assert!(
        hits.is_empty(),
        "unmarked-fn lines must NEVER produce D10 findings: {:?}",
        hits
    );
}

// ── banned-list: publish_signed_event ────────────────────────────────

#[test]
fn check_flags_publish_signed_event_inside_marked_fn() {
    // `commands::publish::publish_signed_event` maps `relays.is_empty()`
    // → `PublishTarget::Auto`. Inside a marked kind:1059 publisher that
    // mapping is a D10 leak by construction — the call must be either
    // guarded upstream (and annotated `doctrine-allow: D10 — …`) or
    // refactored to a non-Auto entry point.
    let hits = check(
        "    outbound.extend(super::publish::publish_signed_event(kernel, raw, &relays, None));",
        false,
        true,
    );
    assert_eq!(
        hits.len(),
        1,
        "publish_signed_event( inside a marked fn must fire D10: {:?}",
        hits
    );
    assert!(
        hits[0].1.contains("publish_signed_event"),
        "the finding message must name the offending token: {}",
        hits[0].1
    );
}

#[test]
fn check_does_not_flag_publish_signed_event_outside_marked_fn() {
    // The `commands::publish::publish_signed_event` call in
    // `actor::dispatch::PublishSignedEvent` is the generic dispatch arm,
    // NOT inside a marked private-kind publisher. It must stay silent —
    // the marker is the opt-in.
    let hits = check(
        "    commands::publish_signed_event(ctx.kernel, raw, &relays, correlation_id);",
        false,
        false,
    );
    assert!(
        hits.is_empty(),
        "publish_signed_event in an unmarked dispatch arm must NOT fire D10: {:?}",
        hits
    );
}

// ── line_allows_d10 (tightened escape hatch) ─────────────────────────

#[test]
fn line_allows_d10_requires_em_dash_reason() {
    let line = "    foo(); // doctrine-allow: D10 — kind:1059 empty-relay guarded above";
    assert!(
        line_allows_d10(line),
        "an em-dash separator with a non-empty reason must silence D10"
    );
}

#[test]
fn line_allows_d10_accepts_ascii_separator() {
    let line = "    foo(); // doctrine-allow: D10 - guarded above";
    assert!(
        line_allows_d10(line),
        "the ASCII ` - ` fallback separator must also silence D10"
    );
}

#[test]
fn line_allows_d10_rejects_bare_annotation() {
    // The whole point of the tightened parser: a bare
    // `// doctrine-allow: D10` (no separator, no reason) must NOT
    // silence the rule. Authors must justify the escape.
    let line = "    foo(); // doctrine-allow: D10";
    assert!(
        !line_allows_d10(line),
        "a bare D10 annotation with no reason must NOT silence the rule"
    );
}

#[test]
fn line_allows_d10_rejects_empty_reason_after_separator() {
    // A separator with only whitespace after it does not count as a
    // written reason.
    let line = "    foo(); // doctrine-allow: D10 —    ";
    assert!(
        !line_allows_d10(line),
        "whitespace-only after the separator does not count as a reason"
    );
    let line_ascii = "    foo(); // doctrine-allow: D10 -    ";
    assert!(
        !line_allows_d10(line_ascii),
        "whitespace-only after the ASCII separator must also fail"
    );
}

#[test]
fn line_allows_d10_rejects_no_annotation() {
    // No annotation at all → not silenced.
    assert!(!line_allows_d10("    foo();"));
}

#[test]
fn line_allows_d10_works_inside_multi_rule_annotation() {
    // The reason lives once at the end of the multi-rule comma list;
    // D10 must recognize itself as one of the listed ids and accept
    // the shared reason.
    let line = "    foo(); // doctrine-allow: D6,D10 — shared reason";
    assert!(
        line_allows_d10(line),
        "D10 must be recognized inside a multi-rule annotation"
    );
    let line_other_only = "    foo(); // doctrine-allow: D6,D7 — shared reason";
    assert!(
        !line_allows_d10(line_other_only),
        "D10 absent from the id list must NOT be silenced"
    );
}

#[test]
fn line_allows_d10_rejects_when_other_rule_has_reason_but_d10_not_listed() {
    // Sanity: an annotation that explicitly excludes D10 cannot
    // accidentally pick up the silencing via the reason text.
    let line = "    foo(); // doctrine-allow: D8 — sleep is legitimate in this bench";
    assert!(!line_allows_d10(line));
}
