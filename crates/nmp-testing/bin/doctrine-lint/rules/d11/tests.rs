use super::*;

fn run_tracker(lines: &[&str]) -> Vec<(usize, String, String)> {
    let mut tracker = FnTracker::default();
    let mut hits = Vec::new();
    for line in lines {
        let in_extern = tracker.in_nmp_app_extern_fn();
        tracker.observe_line(line, false);
        // Run the per-line check AFTER updating in_extern for the body
        // (the open-brace line itself is the signature, but the variant
        // is on a body line so the post-observe-line transition does
        // not matter for these fixtures). Mirror the driver's order:
        // it captures `in_marked_fn` BEFORE `observe_line`, but the
        // tracker's `observe_line` flips the flag on `{`, so the
        // signature line itself sees `false` — fine, the offending
        // constructions live on body lines.
        for hit in check(line, false, in_extern) {
            hits.push(hit);
        }
    }
    hits
}

#[test]
fn flags_publishsignedevent_in_new_nmp_app_extern_fn() {
    let lines = [
        "#[no_mangle]",
        "pub extern \"C\" fn nmp_app_legacy_publish_door(app: *mut NmpApp) {",
        "    let raw = todo!();",
        "    app.send_cmd(ActorCommand::PublishSignedEvent { raw, relays: Vec::new(), correlation_id: None });",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one D11 finding; got {:?}",
        hits
    );
    assert!(
        hits[0].1.contains("ActorCommand::PublishSignedEvent"),
        "message must name the banned variant; got: {}",
        hits[0].1
    );
    assert!(
        hits[0].1.contains("D11"),
        "rule id must appear in the message; got: {}",
        hits[0].1
    );
}

#[test]
fn flags_publish_specific_symbol_even_without_actor_command() {
    let hits = check(
        "pub extern \"C\" fn nmp_app_publish_signed_event(_app: *mut NmpApp) {}",
        false,
        false,
    );
    assert_eq!(hits.len(), 1, "publish-specific symbol must trip D11");
    assert!(hits[0].1.contains("nmp_app_publish_signed_event"));
}

#[test]
fn flags_publishunsignedevent_in_new_nmp_app_extern_fn() {
    let lines = [
        "#[no_mangle]",
        "pub extern \"C\" fn nmp_app_smuggle_unsigned(app: *mut NmpApp) {",
        "    app.send_cmd(ActorCommand::PublishUnsignedEvent(unsigned));",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].1.contains("PublishUnsignedEvent"));
}

#[test]
fn whitelists_retry_publish_body() {
    // A construction of `ActorCommand::PublishSignedEvent` inside
    // `nmp_app_retry_publish` is the whitelisted escape hatch. In
    // practice the body uses `RetryPublish`, but the whitelist is the
    // contract: D11 must not fire.
    let lines = [
        "#[no_mangle]",
        "pub extern \"C\" fn nmp_app_retry_publish(app: *mut NmpApp, handle: *const c_char) {",
        "    app.send_cmd(ActorCommand::PublishSignedEvent { /* impossible today, exempted */ });",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert!(
        hits.is_empty(),
        "whitelist must suppress D11 inside nmp_app_retry_publish; got {:?}",
        hits
    );
}

#[test]
fn whitelists_cancel_publish_body() {
    let lines = [
        "pub extern \"C\" fn nmp_app_cancel_publish(app: *mut NmpApp, handle: *const c_char) {",
        "    app.send_cmd(ActorCommand::PublishUnsignedEvent(_));",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert!(hits.is_empty());
}

#[test]
fn does_not_fire_in_non_ffi_helper() {
    // The `kernel::action_registry` executor builds a
    // `PublishSignedEvent` from validated dispatch JSON. That is the
    // GOOD path (Theme A's "dispatch_action seam"); the body is a
    // regular Rust fn, not `extern "C" fn nmp_app_*`. D11 must not fire.
    let lines = [
        "pub(crate) fn execute(action: PublishAction) {",
        "    send(ActorCommand::PublishSignedEvent { raw, relays, correlation_id });",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert!(
        hits.is_empty(),
        "non-FFI helpers must not trip D11; got {:?}",
        hits
    );
}

#[test]
fn does_not_fire_for_extern_fn_outside_nmp_app_prefix() {
    // A different FFI prefix (e.g. an `nmp_signer_broker_*` symbol) is
    // out of D11's scope — D11 is the door for the `nmp-core` `nmp_app_*`
    // surface, not every `extern "C"` symbol in the workspace.
    let lines = [
        "pub extern \"C\" fn nmp_signer_broker_init(app: *mut c_void) {",
        "    let _ = ActorCommand::PublishSignedEvent { /* hypothetical */ };",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert!(hits.is_empty());
}

#[test]
fn handles_nested_braces_in_body() {
    // A struct-literal `{ ... }` inside the body of a banned `nmp_app_*`
    // function must not prematurely pop the tracker stack.
    let lines = [
        "pub extern \"C\" fn nmp_app_bad(app: *mut NmpApp) {",
        "    let payload = SomeStruct { a: 1, b: 2 };",
        "    app.send_cmd(ActorCommand::PublishSignedEvent { raw, relays, correlation_id });",
        "}",
        "// outside the function — must NOT fire here",
        "pub fn unrelated() { let _ = ActorCommand::PublishSignedEvent; }",
    ];
    let hits = run_tracker(&lines);
    assert_eq!(
        hits.len(),
        1,
        "exactly one D11 hit (the body line) expected; got {:?}",
        hits
    );
    assert!(hits[0].1.contains("PublishSignedEvent"));
}

#[test]
fn ignores_comment_lines() {
    // A doc-comment showing the banned variant for illustration must
    // not fire. The driver routes `is_comment` to `check`; verify here
    // directly.
    let hits = check(
        "    /// Constructs `ActorCommand::PublishSignedEvent` — historical.",
        true,
        true,
    );
    assert!(
        hits.is_empty(),
        "comment lines must be exempt; got {:?}",
        hits
    );
}

#[test]
fn parse_verb_handles_paren_terminator() {
    assert_eq!(
        parse_nmp_app_verb("nmp_app_publish_signed_event(app: *mut NmpApp)"),
        Some("nmp_app_publish_signed_event".to_string())
    );
}

#[test]
fn parse_verb_handles_bracket_terminator() {
    // Generic params terminator (extremely rare for FFI but defensive).
    assert_eq!(
        parse_nmp_app_verb("nmp_app_foo<T>(...)"),
        Some("nmp_app_foo".to_string())
    );
}

#[test]
fn parse_verb_rejects_non_nmp_app_prefix() {
    assert_eq!(parse_nmp_app_verb("other_fn(...)"), None);
}

#[test]
fn finds_opener_with_inline_brace() {
    let line = "pub extern \"C\" fn nmp_app_foo(app: *mut NmpApp) {";
    let pos = find_nmp_app_extern_fn_opener_with_brace(line).expect("should detect opener");
    // The returned position points at the `n` of `nmp_app_foo`.
    assert_eq!(&line[pos..pos + 11], "nmp_app_foo");
}

#[test]
fn opener_requires_same_line_brace() {
    // Wrapped signature where `{` lives on the next line — the
    // `find_nmp_app_extern_fn_opener_with_brace` helper rejects it (no
    // `{` on this line). The wrapped-signature helper picks it up
    // instead.
    let line = "pub extern \"C\" fn nmp_app_foo(";
    assert!(find_nmp_app_extern_fn_opener_with_brace(line).is_none());
    assert_eq!(
        find_wrapped_nmp_app_extern_fn_opener(line),
        Some("nmp_app_foo".to_string())
    );
}

#[test]
fn wrapped_signature_promotes_on_brace_line() {
    // Multi-line FFI signature (the common shape for `nmp_app_*`
    // symbols with several `*const c_char` params, e.g.
    // `nmp_app_create_new_account` or `nmp_app_add_relay`). The body
    // must still be scanned: the verb is parked when the wrapped
    // opener is seen and promoted to a real stack frame on the line
    // that introduces `{`.
    let lines = [
        "#[no_mangle]",
        "pub extern \"C\" fn nmp_app_create_new_account(",
        "    app: *mut NmpApp,",
        "    profile_json: *const c_char,",
        ") {",
        "    app.send_cmd(ActorCommand::PublishSignedEvent { raw, relays, correlation_id });",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert_eq!(
        hits.len(),
        1,
        "wrapped FFI signature body must still trip D11; got {:?}",
        hits
    );
    assert!(hits[0].1.contains("PublishSignedEvent"));
}

#[test]
fn wrapped_whitelisted_signature_still_exempt() {
    // Whitelist must apply through the wrapped-signature path too.
    let lines = [
        "pub extern \"C\" fn nmp_app_cancel_publish(",
        "    app: *mut NmpApp,",
        "    handle: *const c_char,",
        ") {",
        "    let _ = ActorCommand::PublishUnsignedEvent(_);",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert!(
        hits.is_empty(),
        "wrapped whitelisted signature must still suppress D11; got {:?}",
        hits
    );
}
