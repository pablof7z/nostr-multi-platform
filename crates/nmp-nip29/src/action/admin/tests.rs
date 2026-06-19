use super::*;
use std::cell::RefCell;

fn group() -> GroupId {
    GroupId::new("wss://groups.example.com", "rust-nostr")
}

fn put_input() -> PutUserInput {
    PutUserInput {
        group: group(),
        target_pubkey: "a".repeat(64),
        role: Some("admin".to_string()),
        reason: Some("trusted maintainer".to_string()),
    }
}

fn capture_put(input: PutUserInput) -> Vec<ActorCommand> {
    let captured = RefCell::new(Vec::new());
    PutUserAction
        .execute(input, "cid-admin", &|cmd| captured.borrow_mut().push(cmd))
        .expect("put-user executes");
    captured.into_inner()
}

fn capture_invites(input: CreateInviteInput) -> Vec<ActorCommand> {
    let captured = RefCell::new(Vec::new());
    CreateInviteAction
        .execute(input, "cid-invite", &|cmd| captured.borrow_mut().push(cmd))
        .expect("create-invite executes");
    captured.into_inner()
}

#[test]
fn put_user_emits_host_pinned_kind_9000_with_role_on_p_tag() {
    let cmds = capture_put(put_input());
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        ActorCommand::PublishUnsignedEventToRelays {
            event,
            relays,
            correlation_id,
            ..
        } => {
            assert_eq!(event.kind, KIND_PUT_USER);
            assert_eq!(relays, &vec!["wss://groups.example.com".to_string()]);
            assert_eq!(correlation_id.as_deref(), Some("cid-admin"));
            assert!(event
                .tags
                .iter()
                .any(|t| t == &vec!["h".to_string(), "rust-nostr".to_string()]));
            assert!(event
                .tags
                .iter()
                .any(|t| t == &vec!["p".to_string(), "a".repeat(64), "admin".to_string()]));
            assert!(event
                .tags
                .iter()
                .any(|t| { t == &vec!["reason".to_string(), "trusted maintainer".to_string()] }));
        }
        other => panic!("expected publish command, got {other:?}"),
    }
}

#[test]
fn create_invite_fans_out_at_ten_codes_per_event() {
    let codes: Vec<String> = (0..23).map(|n| format!("code-{n}")).collect();
    let cmds = capture_invites(CreateInviteInput {
        group: group(),
        codes,
    });
    assert_eq!(cmds.len(), 3);
    let code_counts: Vec<usize> = cmds
        .iter()
        .map(|cmd| match cmd {
            ActorCommand::PublishUnsignedEventToRelays { event, relays, .. } => {
                assert_eq!(event.kind, KIND_CREATE_INVITE);
                assert_eq!(relays, &vec!["wss://groups.example.com".to_string()]);
                event
                    .tags
                    .iter()
                    .filter(|t| t.first().is_some_and(|k| k == "code"))
                    .count()
            }
            other => panic!("expected publish command, got {other:?}"),
        })
        .collect();
    assert_eq!(code_counts, vec![10, 10, 3]);
}

#[test]
fn invalid_pubkey_is_rejected() {
    let mut ctx = ActionContext::default();
    let action = PutUserInput {
        target_pubkey: "not-hex".to_string(),
        ..put_input()
    };
    assert!(matches!(
        PutUserAction.start(&mut ctx, action),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn invalid_invite_code_is_rejected() {
    let mut ctx = ActionContext::default();
    let action = CreateInviteInput {
        group: group(),
        codes: vec!["good".to_string(), "has space".to_string()],
    };
    assert!(matches!(
        CreateInviteAction.start(&mut ctx, action),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn well_formed_inputs_pass_validation() {
    let mut ctx = ActionContext::default();
    assert!(PutUserAction.start(&mut ctx, put_input()).is_ok());
    assert!(CreateInviteAction
        .start(
            &mut ctx,
            CreateInviteInput {
                group: group(),
                codes: vec!["alpha".to_string()]
            }
        )
        .is_ok());
}
