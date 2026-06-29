use super::*;
use crate::actor::PublishCommand;
use nmp_signer_iface::UnsignedEvent;

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
        target: PublishTarget::manual_override(Vec::new()),
        signer: Default::default(),
    };
    let err = PublishModule
        .start(&mut ctx(), action)
        .expect_err("empty explicit target must fail closed");
    assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("at least one relay")));
}

#[test]
fn explicit_publish_target_rejects_malformed_relay_url() {
    let action = PublishAction::PublishRaw {
        kind: 1,
        tags: Vec::new(),
        content: "hello".to_string(),
        target: PublishTarget::manual_override(vec!["https://relay.example".to_string()]),
        signer: Default::default(),
    };
    let err = PublishModule
        .start(&mut ctx(), action)
        .expect_err("malformed explicit relay must be rejected");
    assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("ws:// or wss://")));
}

#[test]
fn explicit_publish_target_accepts_valid_relay_url() {
    let action = PublishAction::PublishRaw {
        kind: 1,
        tags: Vec::new(),
        content: "hello".to_string(),
        target: PublishTarget::manual_override(vec!["wss://relay.example".to_string()]),
        signer: Default::default(),
    };
    PublishModule
        .start(&mut ctx(), action)
        .expect("valid explicit target should pass validation");
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
        signer: Default::default(),
    };
    let err = PublishModule
        .start(&mut ctx(), action)
        .expect_err("PublishRaw must reject kind:0");
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
        signer: Default::default(),
    };
    let err = PublishModule
        .start(&mut ctx(), action)
        .expect_err("PublishRaw must reject kind:3");
    assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("kind:3")));
}

#[test]
fn publish_raw_rejects_kind_10003_to_protect_bookmark_builder() {
    let action = PublishAction::PublishRaw {
        kind: 10003,
        tags: vec![vec!["e".to_string(), "a".repeat(64)]],
        content: String::new(),
        target: PublishTarget::Auto,
        signer: Default::default(),
    };
    let err = PublishModule
        .start(&mut ctx(), action)
        .expect_err("PublishRaw must reject kind:10003");
    assert!(matches!(err, ActionRejection::Invalid(msg) if msg.contains("nmp.nip51.add_bookmark")));
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
        signer: Default::default(),
    };
    PublishModule
        .start(&mut ctx(), action)
        .expect("valid PublishRaw with Auto target should pass validation");
}

#[test]
fn publish_reply_validates_parent_event_id_shape() {
    let action = PublishAction::PublishReply {
        content: "reply".to_string(),
        reply_to_event_id: "not-hex".to_string(),
        target: PublishTarget::Auto,
        signer: Default::default(),
    };
    let err = PublishModule
        .start(&mut ctx(), action)
        .expect_err("invalid parent event id must fail closed");
    assert!(
        matches!(err, ActionRejection::Invalid(ref msg) if msg.contains("64-character hex")),
        "got {err:?}"
    );
}

#[test]
fn publish_reply_rejects_empty_content() {
    let action = PublishAction::PublishReply {
        content: "   ".to_string(),
        reply_to_event_id: "a".repeat(64),
        target: PublishTarget::Auto,
        signer: Default::default(),
    };
    let err = PublishModule
        .start(&mut ctx(), action)
        .expect_err("empty reply content must fail closed");
    assert!(
        matches!(err, ActionRejection::Invalid(ref msg) if msg.contains("must not be empty")),
        "got {err:?}"
    );
}

#[test]
fn publish_raw_rejects_gift_wrap_with_auto_target() {
    // BLOCKER #1 regression: a kind:1059 gift-wrap published raw with `Auto`
    // would Auto-route the encrypted envelope to the author's PUBLIC relays
    // (D10 privacy leak). The Workstream C one-door gate rejects it at the
    // action boundary — private kinds require an explicit relay pin.
    let action = PublishAction::PublishRaw {
        kind: 1059,
        tags: Vec::new(),
        content: "encrypted".to_string(),
        target: PublishTarget::Auto,
        signer: Default::default(),
    };
    let err = PublishModule
        .start(&mut ctx(), action)
        .expect_err("PublishRaw kind:1059 + Auto must fail closed");
    assert!(
        matches!(&err, ActionRejection::Invalid(msg) if msg.contains("D10") && msg.contains("kind:1059")),
        "rejection must cite D10 + name kind:1059; got: {err:?}"
    );
}

#[test]
fn publish_raw_rejects_sealed_chat_with_auto_target() {
    // The old literal guard missed kind:14 entirely (blocker #2). The policy
    // table covers it: a sealed NIP-17 chat message with `Auto` is refused.
    let action = PublishAction::PublishRaw {
        kind: 14,
        tags: Vec::new(),
        content: "sealed".to_string(),
        target: PublishTarget::Auto,
        signer: Default::default(),
    };
    let err = PublishModule
        .start(&mut ctx(), action)
        .expect_err("PublishRaw kind:14 + Auto must fail closed");
    assert!(
        matches!(&err, ActionRejection::Invalid(msg) if msg.contains("D10") && msg.contains("kind:14")),
        "rejection must cite D10 + name kind:14; got: {err:?}"
    );
}

#[test]
fn publish_raw_allows_gift_wrap_with_verified_private_inbox_relays() {
    // The legitimate DM path: a kind:1059 envelope pinned to an explicit
    // non-empty recipient-inbox relay set is ALLOWED — fail-closed means
    // "no Auto", not "no publish".
    let action = PublishAction::PublishRaw {
        kind: 1059,
        tags: Vec::new(),
        content: "encrypted".to_string(),
        target: PublishTarget::explicit(
            vec!["wss://inbox.example".to_string()],
            PublishRouteClass::VerifiedPrivateInbox,
        ),
        signer: Default::default(),
    };
    PublishModule
        .start(&mut ctx(), action)
        .expect("kind:1059 with a verified private inbox relay set must be allowed");
}

#[test]
fn publish_raw_rejects_gift_wrap_with_manual_explicit_relays() {
    let action = PublishAction::PublishRaw {
        kind: 1059,
        tags: Vec::new(),
        content: "encrypted".to_string(),
        target: PublishTarget::manual_override(vec!["wss://manual.example".to_string()]),
        signer: Default::default(),
    };
    let err = PublishModule
        .start(&mut ctx(), action)
        .expect_err("kind:1059 manual explicit target must fail closed");
    assert!(
        matches!(&err, ActionRejection::Invalid(msg) if msg.contains("verified_private_inbox") && msg.contains("D10")),
        "rejection must require verified private inbox provenance; got: {err:?}"
    );
}

#[test]
fn app_dispatch_rejects_presigned_publish() {
    let action = PublishAction::Publish {
        handle: "h".to_string(),
        event: signed_event(),
        target: PublishTarget::Auto,
    };
    let err = PublishModule
        .start(&mut ctx(), action)
        .expect_err("pre-signed Publish must not be app-dispatchable");
    assert!(
        matches!(&err, ActionRejection::Invalid(msg) if msg.contains("internal/protocol-only")),
        "rejection must name the internal-only pre-signed path; got: {err:?}"
    );
}

#[test]
fn execute_rejects_presigned_publish() {
    let action = PublishAction::Publish {
        handle: "h".to_string(),
        event: signed_event(),
        target: PublishTarget::explicit(
            vec!["wss://inbox.example".to_string()],
            PublishRouteClass::ImportedOrPresigned,
        ),
    };
    let err = run_execute(action).expect_err("pre-signed Publish execute must be rejected");
    assert!(
        err.contains("internal/protocol-only"),
        "rejection must name the internal-only pre-signed path; got: {err}"
    );
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
        target: PublishTarget::manual_override(Vec::new()),
        signer: Default::default(),
    };
    let err = PublishModule
        .start(&mut ctx(), action)
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
    let ctx = ActionContext::default();
    PublishModule.execute(&ctx, action, "test-cid", &|cmd| {
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
        ActorCommand::Publish(PublishCommand::Profile {
            fields,
            correlation_id,
        }) => {
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
        signer: Default::default(),
    };
    let cmds = run_execute(action).expect("execute must succeed");
    assert_eq!(cmds.len(), 1, "must emit exactly one command");
    match cmds.into_iter().next().unwrap() {
        ActorCommand::Publish(PublishCommand::RawEvent {
            kind,
            content,
            target,
            signer_pubkey,
            correlation_id,
            ..
        }) => {
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
    // active account.
    let agent_pk = "f".repeat(64);
    let action = PublishAction::PublishRaw {
        kind: 30023,
        tags: Vec::new(),
        content: "agent-authored".to_string(),
        target: PublishTarget::Auto,
        signer: PublishSigner::registered(agent_pk.clone(), PublishSignerProvenance::AppManaged),
    };
    let cmds = run_execute(action).expect("execute must succeed");
    assert_eq!(cmds.len(), 1, "must emit exactly one command");
    match cmds.into_iter().next().unwrap() {
        ActorCommand::Publish(PublishCommand::RawEvent { signer_pubkey, .. }) => {
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
fn execute_publish_reply_emits_publish_reply_command() {
    let parent_id = "b".repeat(64);
    let action = PublishAction::PublishReply {
        content: "reply".to_string(),
        reply_to_event_id: parent_id.clone(),
        target: PublishTarget::Auto,
        signer: Default::default(),
    };
    let cmds = run_execute(action).expect("execute must succeed");
    assert_eq!(cmds.len(), 1, "must emit exactly one command");
    match cmds.into_iter().next().unwrap() {
        ActorCommand::Publish(PublishCommand::Reply {
            content,
            reply_to_event_id,
            target,
            correlation_id,
            ..
        }) => {
            assert_eq!(content, "reply");
            assert_eq!(reply_to_event_id, parent_id);
            assert_eq!(target, PublishTarget::Auto);
            assert_eq!(correlation_id.as_deref(), Some("test-cid"));
        }
        other => panic!("expected PublishReply, got {other:?}"),
    }
}

#[test]
fn publish_raw_serde_default_signer_is_active_when_field_omitted() {
    // Backward-compat: dispatch JSON may omit `signer`; `#[serde(default)]`
    // must deserialize it to Active rather than failing the decode.
    let json = r#"{"PublishRaw":{"kind":1,"tags":[],"content":"hi","target":"Auto"}}"#;
    let action: PublishAction =
        serde_json::from_str(json).expect("legacy PublishRaw JSON must deserialize");
    match action {
        PublishAction::PublishRaw { signer, .. } => assert_eq!(signer, PublishSigner::Active),
        other => panic!("expected PublishRaw, got {other:?}"),
    }
}

#[test]
fn publish_raw_serde_round_trips_registered_signer_provenance() {
    // The selector must also survive the wire when a host supplies it, so
    // a shell can address an agent key by typed provenance + hex pubkey.
    let agent_pk = "a".repeat(64);
    let json = format!(
        r#"{{"PublishRaw":{{"kind":1,"tags":[],"content":"hi","target":"Auto","signer":{{"kind":"registered","pubkey":"{agent_pk}","provenance":"app_managed"}}}}}}"#
    );
    let action: PublishAction =
        serde_json::from_str(&json).expect("PublishRaw JSON with typed signer must deserialize");
    match action {
        PublishAction::PublishRaw { signer, .. } => assert_eq!(
            signer,
            PublishSigner::registered(agent_pk, PublishSignerProvenance::AppManaged)
        ),
        other => panic!("expected PublishRaw, got {other:?}"),
    }
}
