//! NIP-29 group-chat / discovery / join dispatch + executor proofs, plus
//! the host-side `register_group_chat` / `register_group_discovery`
//! wiring proofs.

use std::ffi::CString;

use nmp_core::substrate::ActionModule;
use nmp_core::ActorCommand;
use nmp_ffi::{nmp_app_free, nmp_app_new};
use nmp_nip29::action::{
    CreatePublicGroupAction, DiscoverGroupsAction, DiscoverGroupsInput, JoinGroupAction,
    JoinGroupInput, PostChatMessageAction, PostChatMessageInput, ReactInGroupAction,
};
use nmp_nip29::group_id::GroupId;
use nmp_nip29::kinds::KIND_CHAT_MESSAGE;

use super::super::{
    nmp_app_chirp_register, nmp_app_chirp_register_group_chat,
    nmp_app_chirp_register_group_discovery, nmp_app_chirp_unregister,
};
use super::helpers::{dispatch, run_module_execute};

/// THE NIP-CRATE SEAM PROOF: after `nmp_app_chirp_register`, the NIP-29
/// `PostChatMessageAction` — an `ActionModule` impl living in the
/// `nmp-nip29` protocol crate, NOT this app crate — is reachable through
/// the generic `dispatch_action` path. A well-formed `PostChatMessageInput`
/// yields a 32-hex `correlation_id` (both the typed module validator and
/// the executor are wired); a malformed body is rejected with `error`.
///
/// This proves the ADR-0027 typed-registration seam (`register_action::<M>()`)
/// works for NIP-crate modules, not just Chirp's app-local social verbs —
/// without `nmp-core` learning any NIP-29 group nouns (D0).
#[test]
fn nip29_post_chat_message_dispatches_through_action_registry() {
    let app = nmp_app_new();
    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null());

    // Well-formed chat message: a host-pinned group + non-empty content.
    // The typed `PostChatMessageAction::start` builds the `["h", local_id]`
    // tag and enforces the host pin — a missing pin would reject here.
    let body = r#"{"group":{"host_relay_url":"wss://groups.example.com","local_id":"rust-nostr"},"content":"hello"}"#;
    let parsed = dispatch(app, "nmp.nip29.post_chat_message", body);
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
    assert_eq!(id.len(), 32, "correlation id should be 32 hex");

    // Malformed shape (missing the required `group`) is rejected by the
    // typed module validator surfaced through the host seam (D6).
    let parsed = dispatch(
        app,
        "nmp.nip29.post_chat_message",
        r#"{"content":"no group"}"#,
    );
    assert!(
        parsed.get("error").is_some(),
        "chat message without `group` must be rejected: {parsed}"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// THE EXECUTOR PROOF: the NIP-29 post-chat-message executor maps a
/// validated `PostChatMessageInput` to a concrete
/// [`ActorCommand::PublishUnsignedEventToRelays`] pinned to the group's
/// own host relay — proving the `PostChatMessageAction::execute` typed
/// path (ADR-0027) produces the right command end-to-end.
#[test]
fn nip29_post_chat_message_executor_emits_host_pinned_publish_command() {
    let input = PostChatMessageInput {
        group: GroupId::new("wss://groups.example.com", "rust-nostr"),
        content: "hello".to_string(),
        previous_event_id_prefixes: vec![],
        reply_to_event_id: None,
    };
    let cmds =
        run_module_execute::<PostChatMessageAction>(input).expect("well-formed chat message");
    let cmd = cmds
        .into_iter()
        .next()
        .expect("post-chat-message executor must send at least one command");

    match cmd {
        ActorCommand::PublishUnsignedEventToRelays {
            event,
            relays,
            correlation_id,
        } => {
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
/// namespaces `register_nip29_actions` wires is reachable through the
/// generic `dispatch_action` path. A well-formed body yields a 32-hex
/// `correlation_id` (BOTH the typed module validator AND the executor are
/// bound under that namespace); a malformed body is rejected with `error`.
///
/// Namespaces come from each `<Action>::NAMESPACE` constant — the single
/// source of truth. Asserting via the constant keeps this test correct
/// regardless of the underlying string.
#[test]
fn nip29_all_namespaces_dispatch_through_action_registry() {
    let app = nmp_app_new();
    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null());

    let group = r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#;
    // Each chat/create namespace, with a well-formed body for its typed
    // `<Input>`.
    let cases: [(&str, String); 3] = [
        (
            PostChatMessageAction::NAMESPACE,
            format!(r#"{{"group":{group},"content":"hi"}}"#),
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
        assert_eq!(id.len(), 32, "{namespace}: correlation id should be 32 hex");

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
/// the generic `dispatch_action` path with a well-formed body — the
/// validator + executor land a 32-hex `correlation_id`. The executor
/// returns an [`ActorCommand::PushInterest`] (not a publish command),
/// proving the seam supports subscribe-side actions, not just publish-side.
#[test]
fn nip29_discover_dispatches_through_action_registry_and_emits_push_interest() {
    let app = nmp_app_new();
    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null());

    // Well-formed: a `wss://` host relay URL. The executor pushes a
    // host-pinned LogicalInterest scoped to that relay.
    let body = r#"{"relay_url":"wss://groups.example.com"}"#;
    let parsed = dispatch(app, DiscoverGroupsAction::NAMESPACE, body);
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
    assert_eq!(id.len(), 32, "discover correlation id should be 32 hex");

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
/// [`ActorCommand::PushInterest`] pinned to the supplied relay, followed
/// by an [`ActorCommand::RecordActionSuccess`] terminal — a
/// subscription-only action has no async publish, so the success surface
/// is instantaneous and must be recorded inline or the host spinner waits
/// forever on `action_results`. Mirrors the in-crate shape proof at
/// `crates/nmp-nip29/src/action/discover.rs::well_formed_input_yields_push_interest_then_record_success`.
#[test]
fn nip29_discover_executor_emits_host_pinned_push_interest_command() {
    let input = DiscoverGroupsInput {
        relay_url: "wss://groups.example.com".to_string(),
    };
    let cmds =
        run_module_execute::<DiscoverGroupsAction>(input).expect("well-formed discover input");

    assert_eq!(
        cmds.len(),
        2,
        "expected PushInterest then RecordActionSuccess, got {cmds:?}"
    );

    match &cmds[0] {
        ActorCommand::PushInterest(interest) => {
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
        other => panic!("expected PushInterest, got {other:?}"),
    }

    // Terminal `RecordActionSuccess` is what closes the host spinner for
    // this subscription-only action.
    match &cmds[1] {
        ActorCommand::RecordActionSuccess { correlation_id } => {
            assert_eq!(correlation_id, "test-cid");
        }
        other => panic!("expected RecordActionSuccess, got {other:?}"),
    }
}

/// THE JOIN DISPATCH PROOF: `nmp.nip29.join` is reachable through the
/// generic `dispatch_action` path with a well-formed body — the validator
/// + executor land a 32-hex `correlation_id`. The executor returns a
/// [`ActorCommand::PublishUnsignedEventToRelays`] host-pinned to the
/// group's own relay (kind:9021), same Case-E lane as the chat actions.
#[test]
fn nip29_join_dispatches_through_action_registry() {
    let app = nmp_app_new();
    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null());

    let group = r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#;
    let body = format!(r#"{{"group":{group}}}"#);
    let parsed = dispatch(app, JoinGroupAction::NAMESPACE, &body);
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
    assert_eq!(id.len(), 32, "join correlation id should be 32 hex");

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
        ActorCommand::PublishUnsignedEventToRelays {
            event,
            relays,
            correlation_id,
        } => {
            assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
            assert_eq!(event.kind, 9021);
            assert!(event
                .tags
                .iter()
                .any(|t| t == &vec!["h".to_string(), "room".to_string()]));
            assert!(event
                .tags
                .iter()
                .any(|t| t == &vec!["code".to_string(), "abc".to_string()]));
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

/// THE DISCOVERY REGISTRATION WIRING PROOF: `nmp_app_chirp_register_group_discovery`
/// registers a `DiscoveredGroupsProjection` against `app` for a well-formed
/// relay URL — it runs to completion (event-observer + snapshot-projection
/// registration) without panicking. The snapshot closure surfacing under
/// `"nmp.nip29.discovered_groups"` is proven end-to-end by the generic seam
/// tests in `nmp-core` and the projection's own tests in `nmp-nip29`.
#[test]
fn register_group_discovery_runs_for_well_formed_relay_url() {
    let app = nmp_app_new();
    let relay = CString::new("wss://groups.example.com").unwrap();
    nmp_app_chirp_register_group_discovery(app, relay.as_ptr());
    nmp_app_free(app);
}

/// D6: a null `app`, a null `host_relay_url`, an empty `host_relay_url`,
/// and non-UTF-8 garbage all degrade to a silent no-op — the function
/// must never panic across the FFI boundary.
#[test]
fn register_group_discovery_null_and_empty_input_are_silent_noops() {
    let relay = CString::new("wss://groups.example.com").unwrap();
    // Null app — must not dereference.
    nmp_app_chirp_register_group_discovery(std::ptr::null_mut(), relay.as_ptr());

    let app = nmp_app_new();
    // Null host_relay_url — silent return.
    nmp_app_chirp_register_group_discovery(app, std::ptr::null());
    // Empty string — silent return.
    let empty = CString::new("").unwrap();
    nmp_app_chirp_register_group_discovery(app, empty.as_ptr());
    nmp_app_free(app);
}

/// THE GROUP-ID WIRE-SHAPE CONTRACT: the JSON shape documented on
/// `nmp_app_chirp_register_group_chat` — `{"host_relay_url":…,
/// "local_id":…}` — is exactly what `GroupId`'s serde derive accepts.
/// This is the contract a Swift caller depends on: a body of any other
/// shape is rejected by the `serde_json::from_str::<GroupId>` parse gate
/// inside the function and the registration silently no-ops (D6).
#[test]
fn register_group_chat_group_id_wire_shape_matches_serde() {
    let parsed: GroupId =
        serde_json::from_str(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#)
            .expect("documented group_id_json shape must deserialize to GroupId");
    assert_eq!(parsed.host_relay_url, "wss://groups.example.com");
    assert_eq!(parsed.local_id, "room");

    // A JSON object missing the required fields is NOT a `GroupId` — the
    // parse gate rejects it, so the function returns without registering.
    assert!(
        serde_json::from_str::<GroupId>(r#"{"not":"a group id"}"#).is_err(),
        "a wrong-shape body must fail the GroupId parse gate"
    );
}

/// THE GROUP-CHAT WIRING PROOF: `nmp_app_chirp_register_group_chat`
/// registers a `GroupChatProjection` against `app` for a well-formed
/// group id — it runs to completion (event-observer + snapshot-projection
/// registration) without panicking. The snapshot closure surfacing under
/// `"nmp.nip29.group_chat"` is proven end-to-end by the generic seam tests in
/// `nmp-core` (`snapshot_registry_tests.rs`) and the projection's own
/// tests in `nmp-nip29`; this asserts the Chirp-side wiring call is sound.
#[test]
fn register_group_chat_runs_for_well_formed_group() {
    let app = nmp_app_new();
    let group =
        CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#).unwrap();
    // Must register both halves (observer + snapshot projection) without
    // panicking across the FFI boundary.
    nmp_app_chirp_register_group_chat(app, group.as_ptr());
    nmp_app_free(app);
}

/// THE IDEMPOTENCY PROOF — group-chat variant. Same shape as the
/// DM-inbox test: two consecutive `register_group_chat` calls leave
/// exactly one `KernelEventObserverId` in the per-app
/// `singleton_event_observer_id` slot, with the second register's id
/// distinct from the first (proving the slot was overwritten and the
/// previous observer was unregistered against the kernel).
#[test]
fn register_group_chat_is_idempotent_on_re_invoke() {
    let app = nmp_app_new();
    // SAFETY: `app` is a valid pointer from `nmp_app_new`, live for the
    // duration of this test.
    let app_ref = unsafe { &*app };

    assert!(
        app_ref.swap_singleton_event_observer(None).is_none(),
        "slot must start empty (no group chat registered yet)"
    );

    let group_a =
        CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room-a"}"#)
            .unwrap();
    let group_b =
        CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room-b"}"#)
            .unwrap();

    // First registration.
    nmp_app_chirp_register_group_chat(app, group_a.as_ptr());
    let id1 = app_ref
        .swap_singleton_event_observer(None)
        .expect("first register must install a kernel-observer id in the per-app slot");
    let prev = app_ref.swap_singleton_event_observer(Some(id1));
    assert!(prev.is_none(), "we just swap-took, slot was empty");

    // Second registration with a different group — the multi-screen
    // navigation case that previously leaked the prior observer.
    nmp_app_chirp_register_group_chat(app, group_b.as_ptr());
    let id2 = app_ref
        .swap_singleton_event_observer(None)
        .expect("second register must install a fresh id in the per-app slot");
    assert_ne!(
        id1, id2,
        "second register must produce a fresh kernel-observer id (got {id1:?} both times)"
    );

    app_ref.unregister_event_observer(id2);
    nmp_app_free(app);
}

/// D6: a null `app`, a null `group_id_json`, and a malformed
/// `group_id_json` (valid JSON, wrong fields) all degrade to a silent
/// no-op — the function must never panic across the FFI boundary.
#[test]
fn register_group_chat_null_and_malformed_input_are_silent_noops() {
    let group =
        CString::new(r#"{"host_relay_url":"wss://groups.example.com","local_id":"room"}"#).unwrap();
    // Null app — must not dereference.
    nmp_app_chirp_register_group_chat(std::ptr::null_mut(), group.as_ptr());

    let app = nmp_app_new();
    // Null group id — silent return.
    nmp_app_chirp_register_group_chat(app, std::ptr::null());
    // Malformed JSON shape — fails the `GroupId` parse gate, silent return.
    let bad = CString::new(r#"{"not":"a group id"}"#).unwrap();
    nmp_app_chirp_register_group_chat(app, bad.as_ptr());
    // Non-JSON garbage — also fails the parse gate, silent return.
    let garbage = CString::new("not json at all").unwrap();
    nmp_app_chirp_register_group_chat(app, garbage.as_ptr());
    nmp_app_free(app);
}
