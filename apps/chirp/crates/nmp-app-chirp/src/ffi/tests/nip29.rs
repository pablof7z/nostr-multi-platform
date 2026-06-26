//! NIP-29 group-chat / discovery / join dispatch + executor proofs.
//!
//! Registration wiring proofs (group-chat + discovery lifecycle) live in the
//! sibling `nip29_registration` module to keep each file under the 500-LOC cap.

use nmp_core::actor::ActorCommand;
use nmp_core::actor::{ActionLedgerCommand, InterestsCommand, PublishCommand};
use nmp_core::substrate::{ActionContext, ActionModule};
use nmp_ffi::{nmp_app_free, nmp_app_new};
use nmp_nip29::action::{
    CreatePublicGroupAction, DiscoverGroupsAction, DiscoverGroupsInput, JoinGroupAction,
    JoinGroupInput, PublishGroupEventAction, PublishGroupEventInput, ReactInGroupAction,
};
use nmp_nip29::group_id::GroupId;
use nmp_nip29::interest::relay_discovery_identity;
use nmp_nip29::kinds::KIND_CHAT_MESSAGE;

use super::super::nmp_app_chirp_unregister;
use super::helpers::{dispatch, register_app, run_module_execute};

/// THE NIP-CRATE SEAM PROOF: after `nmp_app_chirp_register`, the NIP-29
/// `PublishGroupEventAction` — an `ActionModule` impl living in the
/// `nmp-nip29` protocol crate, NOT this app crate — is reachable through
/// the typed byte doorway (ADR-0064 / Cut-B, #1756). A well-formed
/// `PublishGroupEventInput` (here a kind:9 chat message) yields an echoed
/// host-supplied `correlation_id` (both the typed module validator and the
/// executor are wired); a malformed body is rejected with `error`.
///
/// This proves the ADR-0027 typed-registration seam (`register_action::<M>()`)
/// works for NIP-crate modules, not just Chirp's app-local social verbs —
/// without `nmp-core` learning any NIP-29 group nouns (D0).
#[test]
fn nip29_publish_group_event_dispatches_through_action_registry() {
    let app = nmp_app_new();
    let handle = register_app(app);

    // Well-formed group event: a host-pinned group + a kind. The generic
    // publish action injects the `["h", local_id]` envelope and enforces the
    // host pin — a non-routable group would reject here.
    let body = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"rust-nostr"},"kind":9,"content":"hello"}"#;
    let parsed = dispatch(app, "nmp.nip29.publish_group_event", body);
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
    // ADR-0064 / Cut-B (#1756): the byte doorway echoes the host-supplied id.
    assert!(
        !id.is_empty(),
        "byte doorway must echo a non-empty correlation id"
    );

    // Malformed shape (missing the required `group`) is rejected by the
    // typed module validator surfaced through the host seam (D6).
    let parsed = dispatch(
        app,
        "nmp.nip29.publish_group_event",
        r#"{"kind":9,"content":"no group"}"#,
    );
    assert!(
        parsed.get("error").is_some(),
        "group event without `group` must be rejected: {parsed}"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// THE EXECUTOR PROOF: the NIP-29 generic publish executor maps a validated
/// `PublishGroupEventInput` to a concrete
/// [`ActorCommand::PublishUnsignedEventToRelays`] pinned to the group's
/// own host relay — proving the `PublishGroupEventAction::execute` typed
/// path (ADR-0027) produces the right command end-to-end.
#[test]
fn nip29_publish_group_event_executor_emits_host_pinned_publish_command() {
    let input = PublishGroupEventInput {
        group: GroupId::new("wss://groups.example.com", "rust-nostr"),
        kind: KIND_CHAT_MESSAGE,
        content: "hello".to_string(),
        tags: vec![],
    };
    let captured: std::cell::RefCell<Vec<ActorCommand>> = std::cell::RefCell::new(Vec::new());
    let ctx = ActionContext::default();
    PublishGroupEventAction
        .execute(&ctx, input, "test-cid", &|cmd| {
            captured.borrow_mut().push(cmd)
        })
        .expect("well-formed group event");
    let cmd = captured
        .into_inner()
        .into_iter()
        .next()
        .expect("publish-group-event executor must send at least one command");

    match cmd {
        ActorCommand::Publish(PublishCommand::UnsignedEventToRelays {
            event,
            relays,
            correlation_id,
            ..
        }) => {
            // Pinned to EXACTLY the group's host relay — never the
            // author's NIP-65 outbox.
            assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
            // kind:9 chat message, host-pin `["h", local_id]` tag.
            assert_eq!(event.kind, KIND_CHAT_MESSAGE);
            assert!(
                event
                    .tags
                    .iter()
                    .any(|t| t == &vec!["h".to_string(), "rust-nostr".to_string()]),
                "must carry the ['h', local_id] group tag, got {:?}",
                event.tags
            );
            assert_eq!(event.content, "hello");
            // `pubkey` is a placeholder — the actor derives it at sign time.
            assert!(event.pubkey.is_empty());
            // correlation_id threads through from the executor.
            assert!(
                correlation_id.is_some(),
                "correlation_id must be threaded through"
            );
        }
        other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
    }
}

/// THE GROUP-CHAT CATALOG WIRING PROOF: each NIP-29 group-chat/create
/// namespaces `register_nip29_actions` wires is reachable through the typed
/// byte doorway (ADR-0064 / Cut-B, #1756). A well-formed body yields an echoed
/// host-supplied `correlation_id` (BOTH the typed module validator AND the
/// executor are bound under that namespace); a malformed body is rejected with
/// `error`.
///
/// Namespaces come from each `<Action>::NAMESPACE` constant — the single
/// source of truth. Asserting via the constant keeps this test correct
/// regardless of the underlying string.
#[test]
fn nip29_all_namespaces_dispatch_through_action_registry() {
    let app = nmp_app_new();
    let handle = register_app(app);

    let group = r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#;
    // Each chat/create namespace, with a well-formed body for its typed
    // `<Input>`.
    let cases: [(&str, String); 3] = [
        (
            PublishGroupEventAction::NAMESPACE,
            format!(r#"{{"group":{group},"kind":9,"content":"hi"}}"#),
        ),
        (
            ReactInGroupAction::NAMESPACE,
            format!(r#"{{"group":{group},"target_event_id":"deadbeef","content":"+"}}"#),
        ),
        (
            CreatePublicGroupAction::NAMESPACE,
            format!(r#"{{"group":{group},"name":"Rust Nostr"}}"#),
        ),
    ];

    for (namespace, body) in &cases {
        let parsed = dispatch(app, namespace, body);
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{namespace}: expected correlation_id, got {parsed}"));
        // ADR-0064 / Cut-B (#1756): the byte doorway echoes the host-supplied id.
        assert!(
            !id.is_empty(),
            "{namespace}: byte doorway must echo a non-empty correlation id"
        );

        // Malformed shape (no `group`) is rejected by the typed module
        // validator surfaced through the host seam (D6).
        let parsed = dispatch(app, namespace, r#"{"bad":"shape"}"#);
        assert!(
            parsed.get("error").is_some(),
            "{namespace}: malformed body must be rejected, got {parsed}"
        );
    }

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// THE DISCOVERY DISPATCH PROOF: `nmp.nip29.discover` is reachable through
/// the typed byte doorway (ADR-0064 / Cut-B, #1756) with a well-formed body —
/// the validator + executor land an echoed host-supplied `correlation_id`. The
/// executor returns an [`ActorCommand::Interests`] command (not a publish command),
/// proving the seam supports subscribe-side actions, not just publish-side.
#[test]
fn nip29_discover_dispatches_through_action_registry_and_ensures_interest() {
    let app = nmp_app_new();
    let handle = register_app(app);

    // Well-formed: a `wss://` host relay URL. The executor ensures a
    // host-pinned LogicalInterest scoped to that relay.
    let body = r#"{"relay_url":"wss://groups.example.com"}"#;
    let parsed = dispatch(app, DiscoverGroupsAction::NAMESPACE, body);
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
    // ADR-0064 / Cut-B (#1756): the byte doorway echoes the host-supplied id.
    assert!(
        !id.is_empty(),
        "discover: byte doorway must echo a non-empty correlation id"
    );

    // Empty relay_url is rejected by the typed validator (D6).
    let parsed = dispatch(app, DiscoverGroupsAction::NAMESPACE, r#"{"relay_url":""}"#);
    assert!(
        parsed.get("error").is_some(),
        "empty relay_url must be rejected: {parsed}"
    );

    // Non-websocket scheme is rejected by the typed validator (D6).
    let parsed = dispatch(
        app,
        DiscoverGroupsAction::NAMESPACE,
        r#"{"relay_url":"https://groups.example.com"}"#,
    );
    assert!(
        parsed.get("error").is_some(),
        "non-wss relay_url must be rejected: {parsed}"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// THE DISCOVERY EXECUTOR PROOF: the `nmp.nip29.discover` executor maps
/// a validated `DiscoverGroupsInput` to a concrete
/// [`InterestsCommand::EnsureInterest`] pinned to the supplied relay, followed
/// by an [`ActorCommand::RecordActionSuccess`] terminal — a
/// subscription-only action has no async publish, so the success surface
/// is instantaneous and must be recorded inline or the host spinner waits
/// forever on `action_results`. Mirrors the in-crate shape proof at
/// `crates/nmp-nip29/src/action/discover.rs::well_formed_input_yields_ensure_interest_then_record_success`.
#[test]
fn nip29_discover_executor_emits_host_pinned_ensure_interest_command() {
    let input = DiscoverGroupsInput {
        relay_url: "wss://groups.example.com".to_string(),
    };
    let cmds =
        run_module_execute::<DiscoverGroupsAction>(input).expect("well-formed discover input");

    assert_eq!(
        cmds.len(),
        2,
        "expected EnsureInterest then RecordActionSuccess, got {cmds:?}"
    );

    match &cmds[0] {
        ActorCommand::Interests(InterestsCommand::EnsureInterest { identity, interest }) => {
            assert_eq!(
                identity,
                &relay_discovery_identity("wss://groups.example.com")
            );
            // Pinned to the relay — Case E (the third routing lane).
            assert_eq!(
                interest.shape.relay_pin.as_deref(),
                Some("wss://groups.example.com")
            );
            // Three metadata kinds, no `d` tag filter (discovery is
            // per-relay, not per-group).
            for k in [39000_u32, 39001, 39002] {
                assert!(
                    interest.shape.kinds.contains(&k),
                    "discover interest must request kind {k}"
                );
            }
            assert!(
                interest.shape.tags.get("d").is_none(),
                "discover must not constrain by group id"
            );
        }
        other => panic!("expected EnsureInterest, got {other:?}"),
    }

    // Terminal `RecordActionSuccess` is what closes the host spinner for
    // this subscription-only action.
    match &cmds[1] {
        ActorCommand::ActionLedger(ActionLedgerCommand::RecordSuccess {
            correlation_id, ..
        }) => {
            assert_eq!(correlation_id, "test-cid");
        }
        other => panic!("expected RecordActionSuccess, got {other:?}"),
    }
}

/// THE JOIN DISPATCH PROOF: `nmp.nip29.join` is reachable through the typed
/// byte doorway (ADR-0064 / Cut-B, #1756) with a well-formed body — the
/// validator + executor land an echoed host-supplied `correlation_id`. The
/// executor returns a
/// [`ActorCommand::PublishUnsignedEventToRelays`] host-pinned to the
/// group's own relay (kind:9021), same Case-E lane as the chat actions.
#[test]
fn nip29_join_dispatches_through_action_registry() {
    let app = nmp_app_new();
    let handle = register_app(app);

    let group = r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#;
    let body = format!(r#"{{"group":{group}}}"#);
    let parsed = dispatch(app, JoinGroupAction::NAMESPACE, &body);
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
    // ADR-0064 / Cut-B (#1756): the byte doorway echoes the host-supplied id.
    assert!(
        !id.is_empty(),
        "join: byte doorway must echo a non-empty correlation id"
    );

    // Malformed shape (no `group`) is rejected by the typed validator.
    let parsed = dispatch(app, JoinGroupAction::NAMESPACE, r#"{"bad":"shape"}"#);
    assert!(
        parsed.get("error").is_some(),
        "join without group must be rejected: {parsed}"
    );

    // Missing host relay URL inside the group is rejected by the
    // validator (we'd otherwise route the request through the NIP-65
    // outbox — wrong relay).
    let parsed = dispatch(
        app,
        JoinGroupAction::NAMESPACE,
        r#"{"group":{"host_relay_url":"","local_id":"room"}}"#,
    );
    assert!(
        parsed.get("error").is_some(),
        "join with empty host_relay_url must be rejected: {parsed}"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// THE JOIN EXECUTOR PROOF: kind:9021 (`["h", local_id]`), host-pinned
/// to the group's relay, optional invite-code carried as `["code", _]`,
/// optional reason carried as the event content.
#[test]
fn nip29_join_executor_emits_kind_9021_with_host_pin() {
    let input = JoinGroupInput {
        group: GroupId::new("wss://groups.example.com", "room"),
        invite_code: Some("abc".to_string()),
        reason: Some("please".to_string()),
    };
    let cmds = run_module_execute::<JoinGroupAction>(input).expect("well-formed join input");
    let cmd = cmds
        .into_iter()
        .next()
        .expect("join executor must send at least one command");
    match cmd {
        ActorCommand::Publish(PublishCommand::UnsignedEventToRelays {
            event,
            relays,
            correlation_id,
            ..
        }) => {
            assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
            assert_eq!(event.kind, 9021);
            assert!(
                event
                    .tags
                    .iter()
                    .any(|t| t == &vec!["h".to_string(), "room".to_string()])
            );
            assert!(
                event
                    .tags
                    .iter()
                    .any(|t| t == &vec!["code".to_string(), "abc".to_string()])
            );
            assert_eq!(event.content, "please");
            // correlation_id threads through from the executor.
            assert!(
                correlation_id.is_some(),
                "correlation_id must be threaded through"
            );
        }
        other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
    }
}
