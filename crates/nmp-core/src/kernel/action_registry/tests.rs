use super::*;
use crate::actor::{ActorCommand, PublishCommand};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

fn ctx() -> ActionContext {
    ActionContext::default()
}

/// A `SignedEvent` with non-empty `id`/`sig` — enough to pass
/// `PublishModule::start`'s "requires a signed event" gate. The content
/// is irrelevant: `start` never inspects `unsigned`.
fn fixture_signed_event() -> SignedEvent {
    SignedEvent {
        id: "a".repeat(64),
        sig: "b".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: "c".repeat(64),
            kind: 1,
            tags: Vec::new(),
            content: "test".to_string(),
            created_at: 1_700_000_000,
        },
    }
}

#[test]
fn default_registry_has_publish_module() {
    let registry = default_registry();
    assert!(registry.contains("nmp.publish"));
    assert!(!registry.contains("nmp.nope"));
}

// V-38: the `nmp.wallet.pay_invoice` registration test moved to `nmp-nip47`
// (the crate that now owns `WalletPayInvoiceModule`). `default_registry`
// post-V-38 no longer registers it — host apps register the module
// themselves from `nmp-nip47`.

#[test]
fn start_publish_raw_action_returns_correlation_id() {
    // `PublishAction::PublishRaw` exercises the registry → adapter →
    // module::start path; the registry mints a fresh 32-hex `correlation_id`.
    let registry = default_registry();
    let action_json = r#"{"PublishRaw":{"kind":1,"tags":[],"content":"hello","target":"Auto"}}"#;
    let id = registry
        .start(&mut ctx(), 1_700_000_000_000, "nmp.publish", action_json)
        .expect("publish raw action should be accepted");
    assert_eq!(id.len(), 32, "correlation id should be 32 hex chars");
    assert!(
        id.chars().all(|c| c.is_ascii_hexdigit()),
        "correlation id should be hex: {id}"
    );
}

#[test]
fn start_cancel_action_is_not_a_dispatch_variant() {
    // S7 (#1754): cancel is the kernel's cancel-by-`correlation_id` doorway
    // (`nmp_app_cancel_action`), NEVER `dispatch_action`. The bespoke
    // `PublishAction::Cancel` variant is DELETED, so a `Cancel` JSON cannot even
    // deserialize — the action seam carries nothing for cancel, by construction.
    let registry = default_registry();
    let action_json = r#"{"Cancel":{"handle":"smoke-test"}}"#;
    let err = registry
        .start(&mut ctx(), 1_700_000_000_000, "nmp.publish", action_json)
        .expect_err("cancel must not be dispatchable via dispatch_action");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("unknown variant `Cancel`"),
            "rejection should be an unknown-variant decode error (Cancel deleted): {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}

#[test]
fn start_publish_action_returns_minted_correlation_id_not_event_id() {
    // Regression for #1748 Fix 1: the `correlation_id` is the operation's
    // identity, NEVER the event id. The registry mints a fresh 32-hex
    // correlation_id and does NOT substitute the event's 64-hex `id`.
    let registry = default_registry();
    let event = fixture_signed_event();
    let event_id = event.id.clone();
    let action = crate::publish::PublishAction::Publish {
        handle: "h1".to_string(),
        event,
        target: crate::publish::PublishTarget::Auto,
    };
    let action_json = serde_json::to_string(&action).unwrap();
    let id = registry
        .start(&mut ctx(), 1_700_000_000_000, "nmp.publish", &action_json)
        .expect("publish action with id+sig should be accepted");
    assert_ne!(
        id, event_id,
        "the correlation_id must NOT be the event id — identity is not output data"
    );
    assert_eq!(
        id.len(),
        32,
        "minted correlation_id is 32-hex, not the 64-hex event id"
    );
    assert!(
        id.chars().all(|c| c.is_ascii_hexdigit()),
        "minted correlation_id should be hex: {id}"
    );
}

#[test]
fn unknown_namespace_is_rejected() {
    let registry = default_registry();
    let err = registry
        .start(&mut ctx(), 1_700_000_000_000, "nmp.does-not-exist", "{}")
        .expect_err("unknown namespace must be rejected");
    match err {
        ActionRejection::Invalid(msg) => {
            assert!(msg.contains("unknown action namespace"), "got: {msg}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn malformed_json_is_rejected_as_invalid() {
    let registry = default_registry();
    let err = registry
        .start(
            &mut ctx(),
            1_700_000_000_000,
            "nmp.publish",
            "{not valid json",
        )
        .expect_err("malformed JSON must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "expected Invalid, got {err:?}"
    );
}

#[test]
fn json_not_matching_action_shape_is_rejected() {
    // Valid JSON, wrong shape for `PublishAction` — serde's externally
    // tagged enum expects `{"<Variant>": {...}}`, so a flat
    // `{"t":"PublishRaw"}` matches no variant and is rejected.
    let registry = default_registry();
    let err = registry
        .start(
            &mut ctx(),
            1_700_000_000_000,
            "nmp.publish",
            r#"{"t":"PublishRaw"}"#,
        )
        .expect_err("wrong-shape JSON must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

/// THE FIX: the `nmp.publish` executor threads the registry-minted
/// `correlation_id` onto `ActorCommand::PublishRawEvent`. The actor signs the
/// event, so its id is unknown at dispatch time — without this, the
/// publish engine would report the event id and the host's spinner (keyed
/// on the dispatch return value) could never be cleared. This exercises
/// the real `default_registry()` executor closure end-to-end via
/// `execute()`, capturing the `ActorCommand` it sends.
#[test]
fn publish_raw_executor_threads_correlation_id_onto_actor_command() {
    use crate::actor::ActorCommand;
    use std::cell::RefCell;

    let registry = default_registry();
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());

    let minted_correlation_id = "fe".repeat(16);
    let action_json = r#"{"PublishRaw":{"kind":1,"tags":[],"content":"hello","target":{"Explicit":{"relays":["wss://relay.example"],"route_class":"manual_override"}}}}"#;
    registry
        .execute(
            &ctx(),
            "nmp.publish",
            action_json,
            &minted_correlation_id,
            &|cmd| {
                captured.borrow_mut().push(cmd);
            },
        )
        .expect("publish-raw execution should succeed");

    let cmds = captured.into_inner();
    assert_eq!(
        cmds.len(),
        1,
        "executor must emit exactly one ActorCommand; got {cmds:?}"
    );
    match cmds.into_iter().next().unwrap() {
        ActorCommand::Publish(PublishCommand::RawEvent {
            kind,
            content,
            target,
            correlation_id,
            ..
        }) => {
            assert_eq!(kind, 1);
            assert_eq!(content, "hello");
            assert_eq!(
                target,
                crate::publish::PublishTarget::Explicit {
                    relays: vec!["wss://relay.example".to_string()],
                    route_class: crate::publish::PublishRouteClass::ManualOverride,
                },
                "the executor must preserve the validated publish target"
            );
            assert_eq!(
                correlation_id,
                Some(minted_correlation_id),
                "the executor must thread the minted correlation_id onto the command"
            );
        }
        other => panic!("expected ActorCommand::PublishRawEvent, got {other:?}"),
    }
}

#[test]
fn publish_raw_executor_rejects_anonymous_explicit_target() {
    let registry = default_registry();
    let action_json = r#"{"PublishRaw":{"kind":1,"tags":[],"content":"hello","target":{"Explicit":{"relays":["wss://relay.example"]}}}}"#;
    let err = registry
        .execute(&ctx(), "nmp.publish", action_json, "cid", &|_| {})
        .expect_err("anonymous explicit relay target must fail decode/validation");
    assert!(
        err.message.contains("route_class") || err.message.contains("missing field"),
        "rejection must mention the missing route class; got: {err:?}"
    );
}

/// Regression for #1748 Fix 1: the pre-signed `Publish` executor threads the
/// registry-minted `correlation_id` onto `ActorCommand::PublishSignedEvent` —
/// NOT the event id (output data in `raw`). The deleted `preferred_action_id`
/// substituted `event.id`, so the host could not match the terminal to its
/// spinner.
#[test]
fn publish_signed_executor_sends_publish_signed_event_command() {
    use crate::actor::ActorCommand;
    use std::cell::RefCell;

    let registry = default_registry();
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());

    let action = crate::publish::PublishAction::Publish {
        handle: "h-presigned".to_string(),
        event: fixture_signed_event(),
        target: crate::publish::PublishTarget::Auto,
    };
    let action_json = serde_json::to_string(&action).unwrap();
    let minted_correlation_id = "ae".repeat(16);
    registry
        .execute(
            &ctx(),
            "nmp.publish",
            &action_json,
            &minted_correlation_id,
            &|cmd| {
                captured.borrow_mut().push(cmd);
            },
        )
        .expect("publish execution should succeed");

    let cmds = captured.into_inner();
    assert_eq!(
        cmds.len(),
        1,
        "executor must emit exactly one ActorCommand; got {cmds:?}"
    );
    match cmds.into_iter().next().unwrap() {
        ActorCommand::Publish(PublishCommand::SignedEvent {
            target,
            correlation_id,
            raw,
        }) => {
            assert_eq!(target, crate::publish::PublishTarget::Auto);
            assert_ne!(
                correlation_id.as_deref(),
                Some(raw.id.as_str()),
                "the correlation_id must NOT be the event id — identity is not output data (#1748)"
            );
            assert_eq!(
                correlation_id,
                Some(minted_correlation_id),
                "the executor must thread the minted correlation_id onto the command"
            );
        }
        other => panic!("a pre-signed Publish must route to PublishSignedEvent, got {other:?}"),
    }
}

#[test]
fn start_publish_profile_action_with_string_fields_is_accepted() {
    // `PublishAction::PublishProfile` with a flat string-valued `fields`
    // map passes `PublishModule::start`'s validation gate — the
    // `ActionModule`-native path for kind:0 metadata publish. The
    // one-door-per-capability rule deleted the prior
    // `nmp_app_publish_unsigned_event` FFI symbol; this `nmp.publish`
    // dispatch is the sole entrypoint for it.
    let registry = default_registry();
    let action_json = r#"{"PublishProfile":{"fields":{"name":"Alice","about":"hello"}}}"#;
    let id = registry
        .start(&mut ctx(), 1_700_000_000_000, "nmp.publish", action_json)
        .expect("publish-profile action with string fields should be accepted");
    assert_eq!(id.len(), 32, "correlation id should be 32 hex chars");
    assert!(
        id.chars().all(|c| c.is_ascii_hexdigit()),
        "correlation id should be hex: {id}"
    );
}

#[test]
fn start_publish_profile_action_with_non_string_field_is_rejected() {
    // A kind:0 `content` is a flat JSON object of string values — a
    // numeric (or any non-string) field is rejected at `start`.
    let registry = default_registry();
    let action_json = r#"{"PublishProfile":{"fields":{"name":"Alice","age":42}}}"#;
    let err = registry
        .start(&mut ctx(), 1_700_000_000_000, "nmp.publish", action_json)
        .expect_err("non-string profile field must be rejected");
    match err {
        ActionRejection::Invalid(msg) => {
            assert!(msg.contains("must be a string value"), "got: {msg}");
        }
        other => panic!("expected Invalid, got {other:?}"),
    }
}

/// The `nmp.publish` executor threads the registry-minted `correlation_id`
/// onto `ActorCommand::PublishProfile`. The actor signs the event, so its
/// id is unknown at dispatch time — without this the publish engine could
/// not report the host's correlation_id in `action_results`. Exercises
/// the real `default_registry()` executor closure via `execute()`.
#[test]
fn publish_profile_executor_threads_correlation_id_onto_actor_command() {
    use crate::actor::ActorCommand;
    use std::cell::RefCell;

    let registry = default_registry();
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());

    let minted_correlation_id = "ab".repeat(16);
    let action_json =
        r#"{"PublishProfile":{"fields":{"name":"Alice","picture":"https://x/y.png"}}}"#;
    registry
        .execute(
            &ctx(),
            "nmp.publish",
            action_json,
            &minted_correlation_id,
            &|cmd| {
                captured.borrow_mut().push(cmd);
            },
        )
        .expect("publish-profile execution should succeed");

    let cmds = captured.into_inner();
    assert_eq!(
        cmds.len(),
        1,
        "executor must emit exactly one ActorCommand; got {cmds:?}"
    );
    match cmds.into_iter().next().unwrap() {
        ActorCommand::Publish(PublishCommand::Profile {
            fields,
            correlation_id,
        }) => {
            assert_eq!(
                fields.get("name").and_then(|v| v.as_str()),
                Some("Alice"),
                "the profile fields must be carried through verbatim"
            );
            assert_eq!(
                fields.get("picture").and_then(|v| v.as_str()),
                Some("https://x/y.png")
            );
            assert_eq!(
                correlation_id,
                Some(minted_correlation_id),
                "the executor must thread the minted correlation_id onto the command"
            );
        }
        other => panic!("expected ActorCommand::PublishProfile, got {other:?}"),
    }
}
