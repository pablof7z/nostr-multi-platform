use super::*;
use crate::substrate::UnsignedEvent;

fn ctx() -> ActionContext {
    ActionContext::default()
}

fn signed_event() -> SignedEvent {
    SignedEvent {
        id: "a".repeat(64),
        sig: "b".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: "c".repeat(64),
            kind: 1,
            tags: Vec::new(),
            content: "hello".to_string(),
            created_at: 1_700_000_000,
        },
    }
}

#[test]
fn explicit_publish_target_requires_non_empty_relays() {
    let action = PublishAction::PublishRaw {
        kind: 1,
        tags: Vec::new(),
        content: "hello".to_string(),
        target: PublishTarget::Explicit { relays: Vec::new() },
        signer_pubkey: None,
    };
    let err = PublishModule.start(&mut ctx(), action)
        .expect_err("empty explicit target must fail closed");
    assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("at least one relay")));
}

#[test]
fn explicit_publish_target_rejects_malformed_relay_url() {
    let action = PublishAction::Publish {
        handle: "h".to_string(),
        event: signed_event(),
        target: PublishTarget::Explicit {
            relays: vec!["https://relay.example".to_string()],
        },
    };
    let err = PublishModule.start(&mut ctx(), action)
        .expect_err("malformed explicit relay must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("ws:// or wss://")));
}

#[test]
fn explicit_publish_target_accepts_valid_relay_url() {
    let action = PublishAction::PublishRaw {
        kind: 1,
        tags: Vec::new(),
        content: "hello".to_string(),
        target: PublishTarget::Explicit {
            relays: vec!["wss://relay.example".to_string()],
        },
        signer_pubkey: None,
    };
    PublishModule.start(&mut ctx(), action).expect("valid explicit target should pass validation");
}

#[test]
fn publish_raw_rejects_kind_0_to_protect_profile_path() {
    // kind:0 has dedicated `PublishProfile` handling (field validation +
    // string-typed-content guarantee). Routing it through `PublishRaw`
    // would bypass that, so the guard fails closed at `start`.
    let action = PublishAction::PublishRaw {
        kind: 0,
        tags: Vec::new(),
        content: "{}".to_string(),
        target: PublishTarget::Auto,
        signer_pubkey: None,
    };
    let err = PublishModule.start(&mut ctx(), action).expect_err("PublishRaw must reject kind:0");
    assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("PublishProfile")));
}

#[test]
fn publish_raw_rejects_kind_3_pending_dedicated_path() {
    // kind:3 (contact list) needs a follow-list-merge step; PublishRaw
    // would publish the raw payload verbatim, silently overwriting the
    // user's existing follow set. Fail closed until a dedicated variant
    // (or contacts-aware PublishRaw branch) lands.
    let action = PublishAction::PublishRaw {
        kind: 3,
        tags: Vec::new(),
        content: String::new(),
        target: PublishTarget::Auto,
        signer_pubkey: None,
    };
    let err = PublishModule.start(&mut ctx(), action).expect_err("PublishRaw must reject kind:3");
    assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("kind:3")));
}

#[test]
fn publish_raw_accepts_arbitrary_event_kind_with_auto_target() {
    // A kind:30023 (long-form article) is the canonical second-app
    // motivation. `Auto` target must pass validation — `#[serde(default)]`
    // + `Default::default() == Auto` is the host-omits-the-field path,
    // so it has to be a valid input.
    let action = PublishAction::PublishRaw {
        kind: 30023,
        tags: vec![vec!["d".to_string(), "my-article".to_string()]],
        content: "# Hello, second app".to_string(),
        target: PublishTarget::Auto,
        signer_pubkey: None,
    };
    PublishModule.start(&mut ctx(), action)
        .expect("valid PublishRaw with Auto target should pass validation");
}

#[test]
fn publish_raw_propagates_explicit_target_validation_failure() {
    // The kind guard runs first, but past it the existing
    // `validate_publish_target` must still apply — an explicit empty
    // relay set fails closed exactly as for `Publish`.
    let action = PublishAction::PublishRaw {
        kind: 30023,
        tags: Vec::new(),
        content: "body".to_string(),
        target: PublishTarget::Explicit { relays: Vec::new() },
        signer_pubkey: None,
    };
    let err = PublishModule.start(&mut ctx(), action)
        .expect_err("empty explicit target must fail closed for PublishRaw too");
    assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("at least one relay")));
}

#[test]
fn publish_target_default_is_auto_for_serde_omitted_field() {
    // `#[serde(default)] target: PublishTarget` on PublishRaw relies
    // on Default returning Auto. Lock that in so a future contributor
    // can't quietly flip it to Explicit and silently widen routing.
    assert_eq!(PublishTarget::default(), PublishTarget::Auto);
}

fn run_execute(action: PublishAction) -> Result<Vec<ActorCommand>, String> {
    use std::cell::RefCell;
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    PublishModule.execute(action, "test-cid", &|cmd| {
        captured.borrow_mut().push(cmd);
    })?;
    Ok(captured.into_inner())
}

#[test]
fn execute_publish_profile_emits_publish_profile_command() {
    let mut fields = serde_json::Map::new();
    fields.insert(
        "display_name".to_string(),
        serde_json::Value::String("Alice".to_string()),
    );
    let action = PublishAction::PublishProfile { fields };
    let cmds = run_execute(action).expect("execute must succeed");
    assert_eq!(cmds.len(), 1, "must emit exactly one command");
    match cmds.into_iter().next().unwrap() {
        ActorCommand::PublishProfile {
            fields,
            correlation_id,
        } => {
            assert_eq!(
                fields.get("display_name").and_then(|v| v.as_str()),
                Some("Alice"),
            );
            assert_eq!(correlation_id.as_deref(), Some("test-cid"));
        }
        other => panic!("expected PublishProfile, got {other:?}"),
    }
}

#[test]
fn execute_publish_raw_emits_publish_raw_event_command() {
    let action = PublishAction::PublishRaw {
        kind: 30023,
        tags: vec![vec!["d".to_string(), "slug".to_string()]],
        content: "body".to_string(),
        target: PublishTarget::Auto,
        signer_pubkey: None,
    };
    let cmds = run_execute(action).expect("execute must succeed");
    assert_eq!(cmds.len(), 1, "must emit exactly one command");
    match cmds.into_iter().next().unwrap() {
        ActorCommand::PublishRawEvent {
            kind,
            content,
            target,
            signer_pubkey,
            correlation_id,
            ..
        } => {
            assert_eq!(kind, 30023);
            assert_eq!(content, "body");
            assert_eq!(target, PublishTarget::Auto);
            assert_eq!(
                signer_pubkey, None,
                "no selector supplied → active account (None)"
            );
            assert_eq!(correlation_id.as_deref(), Some("test-cid"));
        }
        other => panic!("expected PublishRawEvent, got {other:?}"),
    }
}

#[test]
fn execute_publish_raw_threads_signer_pubkey_onto_actor_command() {
    // The signer selector must survive `execute`: a `PublishRaw` carrying
    // `signer_pubkey: Some(agent_pk)` lands on the `ActorCommand::PublishRawEvent`
    // the actor dispatches, so the agent / per-podcast key signs instead of the
    // active account (app-signer-slot.md §"Publishing with an agent key").
    let agent_pk = "f".repeat(64);
    let action = PublishAction::PublishRaw {
        kind: 30023,
        tags: Vec::new(),
        content: "agent-authored".to_string(),
        target: PublishTarget::Auto,
        signer_pubkey: Some(agent_pk.clone()),
    };
    let cmds = run_execute(action).expect("execute must succeed");
    assert_eq!(cmds.len(), 1, "must emit exactly one command");
    match cmds.into_iter().next().unwrap() {
        ActorCommand::PublishRawEvent { signer_pubkey, .. } => {
            assert_eq!(
                signer_pubkey,
                Some(agent_pk),
                "execute must thread the signer selector onto the actor command"
            );
        }
        other => panic!("expected PublishRawEvent, got {other:?}"),
    }
}

#[test]
fn publish_raw_serde_default_signer_pubkey_is_none_when_field_omitted() {
    // Backward-compat: dispatch JSON authored before the selector existed
    // omits `signer_pubkey`; `#[serde(default)]` must deserialize it to `None`
    // (active account) rather than failing the decode.
    let json = r#"{"PublishRaw":{"kind":1,"tags":[],"content":"hi","target":"Auto"}}"#;
    let action: PublishAction =
        serde_json::from_str(json).expect("legacy PublishRaw JSON must deserialize");
    match action {
        PublishAction::PublishRaw { signer_pubkey, .. } => {
            assert_eq!(
                signer_pubkey, None,
                "an omitted signer_pubkey must default to None (active account)"
            );
        }
        other => panic!("expected PublishRaw, got {other:?}"),
    }
}

#[test]
fn publish_raw_serde_round_trips_explicit_signer_pubkey() {
    // The selector must also survive the wire when a host *does* supply it, so
    // a Swift / Kotlin shell can address an agent key by hex pubkey.
    let agent_pk = "a".repeat(64);
    let json = format!(
        r#"{{"PublishRaw":{{"kind":1,"tags":[],"content":"hi","target":"Auto","signer_pubkey":"{agent_pk}"}}}}"#
    );
    let action: PublishAction =
        serde_json::from_str(&json).expect("PublishRaw JSON with signer_pubkey must deserialize");
    match action {
        PublishAction::PublishRaw { signer_pubkey, .. } => {
            assert_eq!(signer_pubkey, Some(agent_pk));
        }
        other => panic!("expected PublishRaw, got {other:?}"),
    }
}

#[test]
fn execute_publish_signed_event_emits_publish_signed_event_command() {
    let action = PublishAction::Publish {
        handle: "h".to_string(),
        event: signed_event(),
        target: PublishTarget::Auto,
    };
    let cmds = run_execute(action).expect("execute must succeed");
    assert_eq!(cmds.len(), 1, "must emit exactly one command");
    match cmds.into_iter().next().unwrap() {
        ActorCommand::PublishSignedEvent {
            raw,
            target,
            correlation_id,
        } => {
            assert_eq!(raw.kind, 1);
            assert_eq!(target, PublishTarget::Auto);
            assert_eq!(correlation_id.as_deref(), Some("test-cid"));
        }
        other => panic!("expected PublishSignedEvent, got {other:?}"),
    }
}
