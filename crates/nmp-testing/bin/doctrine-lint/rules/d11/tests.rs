use super::*;

fn run_tracker(lines: &[&str]) -> Vec<(usize, String, String)> {
    let mut tracker = FnTracker::default();
    let mut hits = Vec::new();
    for line in lines {
        let in_export = tracker.in_uniffi_export_scope();
        tracker.observe_line(line, false);
        // Run the per-line check AFTER updating in_export for the body
        // (the open-brace line itself is the signature, but the variant
        // is on a body line so the post-observe-line transition does
        // not matter for these fixtures). Mirror the driver's order:
        // it captures `in_marked_fn` BEFORE `observe_line`, but the
        // tracker's `observe_line` flips the flag on `{`, so the
        // signature line itself sees `false` — fine, the offending
        // constructions live on body lines.
        for hit in check(line, false, in_export) {
            hits.push(hit);
        }
    }
    hits
}

#[test]
fn flags_publishsignedevent_in_uniffi_export_impl() {
    let lines = [
        "#[uniffi::export]",
        "impl LegacyDoor {",
        "    pub fn legacy_publish_door(&self, event_json: String) {",
        "        let raw = todo!();",
        "        send(ActorCommand::Publish(PublishCommand::SignedEvent { raw, relays: Vec::new(), correlation_id: None }));",
        "    }",
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
fn flags_publishunsignedevent_in_uniffi_export_fn() {
    let lines = [
        "#[uniffi::export]",
        "pub fn smuggle_unsigned(unsigned_json: String) {",
        "    send(ActorCommand::PublishUnsignedEvent(unsigned));",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].1.contains("PublishUnsignedEvent"));
}

#[test]
fn does_not_fire_in_non_exported_helper() {
    // The `kernel::action_registry` executor builds a
    // `PublishSignedEvent` from validated dispatch JSON. That is the
    // GOOD path (Theme A's "dispatch_action seam"); the body is a
    // regular Rust fn, not `#[uniffi::export]`-attributed. D11 must not fire.
    let lines = [
        "pub(crate) fn execute(action: PublishAction) {",
        "    send(ActorCommand::Publish(PublishCommand::SignedEvent { raw, relays, correlation_id }));",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert!(
        hits.is_empty(),
        "non-exported helpers must not trip D11; got {:?}",
        hits
    );
}

#[test]
fn does_not_fire_for_plain_impl_without_uniffi_export() {
    // A regular impl block with no `#[uniffi::export]` attribute is out of
    // D11's scope — D11 is the door for the UniFFI publish surface, not
    // every impl in the workspace.
    let lines = [
        "impl SomeType {",
        "    pub fn hypothetical(&self) {",
        "        let _ = ActorCommand::Publish(PublishCommand::SignedEvent { raw, relays, correlation_id });",
        "    }",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert!(hits.is_empty());
}

#[test]
fn handles_nested_braces_in_body() {
    // A struct-literal `{ ... }` inside the body of a banned exported
    // method must not prematurely pop the tracker stack.
    let lines = [
        "#[uniffi::export]",
        "impl LegacyDoor {",
        "    pub fn bad(&self) {",
        "        let payload = SomeStruct { a: 1, b: 2 };",
        "        send(ActorCommand::Publish(PublishCommand::SignedEvent { raw, relays, correlation_id }));",
        "    }",
        "}",
        "// outside the impl — must NOT fire here",
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
fn flags_bare_publishsignedevent_split_construction() {
    // Regression guard for the split-construction bypass: a developer
    // assigns `PublishCommand::SignedEvent { .. }` to a local on line A,
    // then passes the local to `ActorCommand::Publish(cmd)` on line B.
    // Neither line contains the full inline pattern
    // `ActorCommand::Publish(PublishCommand::SignedEvent`; D11 must still
    // fire on line A via the bare `PublishCommand::SignedEvent` entry.
    let lines = [
        "#[uniffi::export]",
        "impl LegacyDoor {",
        "    pub fn something(&self) {",
        "        let cmd = PublishCommand::SignedEvent { event, relays: Vec::new(), correlation_id: None };",
        "        send(ActorCommand::Publish(cmd));",
        "    }",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert!(
        !hits.is_empty(),
        "bare PublishCommand::SignedEvent in an exported body must trip D11; got no hits"
    );
    assert!(
        hits.iter()
            .any(|(_, msg, _)| msg.contains("PublishSignedEvent")),
        "at least one hit must name ActorCommand::PublishSignedEvent; got {:?}",
        hits
    );
}

#[test]
fn flags_bare_publishunsignedevent_split_construction() {
    // Same loophole for the unsigned variant.
    let lines = [
        "#[uniffi::export]",
        "impl LegacyDoor {",
        "    pub fn other(&self) {",
        "        let cmd = PublishCommand::UnsignedEvent { event };",
        "        send(ActorCommand::Publish(cmd));",
        "    }",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert!(
        !hits.is_empty(),
        "bare PublishCommand::UnsignedEvent in an exported body must trip D11; got no hits"
    );
    assert!(
        hits.iter()
            .any(|(_, msg, _)| msg.contains("PublishUnsignedEvent")),
        "at least one hit must name ActorCommand::PublishUnsignedEvent; got {:?}",
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
fn opens_export_scope_detects_impl_and_fn_openers() {
    assert!(opens_export_scope_with_brace("impl NmpApp {"));
    assert!(opens_export_scope_with_brace(
        "    pub fn dispatch_action(&self, envelope: Vec<u8>) -> DispatchOutcome {"
    ));
    assert!(opens_export_scope_with_brace("pub fn free_fn(x: u32) {"));
    // No brace on this line — signature continues on the next line.
    assert!(!opens_export_scope_with_brace(
        "pub fn wrapped_signature(app: *mut NmpApp,"
    ));
    // Neither an impl nor an fn opener.
    assert!(!opens_export_scope_with_brace("pub struct Foo {"));
}

#[test]
fn tracker_pending_export_survives_doc_comment_and_second_attribute() {
    // The real `NmpApp::new` shape: `#[uniffi::export]` decorates the impl,
    // then a doc comment and a second attribute (`#[uniffi::constructor]`)
    // sit between the attribute and the method's own opener line.
    let lines = [
        "#[uniffi::export]",
        "impl NmpApp {",
        "    /// Construct a new `NmpApp`.",
        "    #[uniffi::constructor]",
        "    pub fn new() -> Arc<Self> {",
        "        send(ActorCommand::Publish(PublishCommand::SignedEvent { raw, relays, correlation_id }));",
        "    }",
        "}",
    ];
    let hits = run_tracker(&lines);
    assert_eq!(
        hits.len(),
        1,
        "impl-level export must cover nested fns; got {:?}",
        hits
    );
}
