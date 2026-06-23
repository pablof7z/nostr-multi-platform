use super::*;
use nmp_core::actor::ActorCommand;
use nmp_core::PublishCommand;
use std::cell::RefCell;

fn cache_with(pubkey: &str, urls: &[&str]) -> Arc<InMemoryBlockedRelayCache> {
    let cache = Arc::new(InMemoryBlockedRelayCache::new());
    cache.upsert(pubkey.into(), urls.iter().map(|s| s.to_string()).collect());
    cache
}

fn empty_cache() -> Arc<InMemoryBlockedRelayCache> {
    Arc::new(InMemoryBlockedRelayCache::new())
}

fn ctx() -> ActionContext {
    ActionContext::default()
}

// ── build_blocked_relay_list_event ────────────────────────────────────

#[test]
fn build_produces_kind_10006() {
    let mut urls = BTreeSet::new();
    urls.insert("wss://blocked.example".to_string());
    let event = build_blocked_relay_list_event(&urls);
    assert_eq!(event.kind, KIND_BLOCKED_RELAYS);
}

#[test]
fn build_uses_relay_tag_marker() {
    let mut urls = BTreeSet::new();
    urls.insert("wss://blocked.example".to_string());
    let event = build_blocked_relay_list_event(&urls);
    assert_eq!(event.tags[0][0], "relay", "NIP-51 tag marker is 'relay'");
}

#[test]
fn build_uses_created_at_zero_sentinel() {
    let event = build_blocked_relay_list_event(&BTreeSet::new());
    assert_eq!(event.created_at, 0, "D7: sentinel — the actor re-stamps it");
}

#[test]
fn build_leaves_pubkey_empty() {
    let event = build_blocked_relay_list_event(&BTreeSet::new());
    assert!(event.pubkey.is_empty());
}

#[test]
fn build_empty_set_produces_zero_tags() {
    // Unblocking the last entry → empty kind:10006 "I cleared my list".
    let event = build_blocked_relay_list_event(&BTreeSet::new());
    assert_eq!(event.kind, KIND_BLOCKED_RELAYS);
    assert!(event.tags.is_empty());
}

#[test]
fn build_matches_parse_blocked_relay_list_shape() {
    // Pin the tag shape: [`"relay"`, url] — exactly what
    // parse_blocked_relay_list consumes (tag[0] == "relay", url ∈ wss://).
    let mut urls = BTreeSet::new();
    urls.insert("wss://a.example".to_string());
    urls.insert("wss://b.example".to_string());
    let event = build_blocked_relay_list_event(&urls);
    for tag in &event.tags {
        assert_eq!(tag.len(), 2, "tag must be [\"relay\", url]");
        assert_eq!(tag[0], "relay");
        assert!(tag[1].starts_with("wss://"));
    }
}

// ── validate_and_canonicalize ─────────────────────────────────────────

#[test]
fn validate_accepts_wss_url() {
    assert!(validate_and_canonicalize("wss://relay.example").is_ok());
}

#[test]
fn validate_rejects_ws_url() {
    let err = validate_and_canonicalize("ws://relay.example").unwrap_err();
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn validate_rejects_https_url() {
    let err = validate_and_canonicalize("https://relay.example").unwrap_err();
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn validate_rejects_arbitrary_string() {
    let err = validate_and_canonicalize("not-a-url").unwrap_err();
    assert!(matches!(err, ActionRejection::Invalid(_)));
}

#[test]
fn validate_canonicalizes_host() {
    let canonical = validate_and_canonicalize("wss://RELAY.EXAMPLE/").unwrap();
    assert_eq!(canonical, "wss://relay.example");
}

// ── BlockRelayAction — start ──────────────────────────────────────────

#[test]
fn block_start_accepts_valid_unblocked_url() {
    let cache = empty_cache();
    let action = BlockRelayAction::new(cache);
    let input = BlockRelayInput {
        url: "wss://relay.example".into(),
        account_pubkey: "alice".into(),
    };
    assert!(action.start(&mut ctx(), input).is_ok());
}

#[test]
fn block_start_rejects_non_wss_url() {
    let cache = empty_cache();
    let action = BlockRelayAction::new(cache);
    let input = BlockRelayInput {
        url: "http://relay.example".into(),
        account_pubkey: "alice".into(),
    };
    assert!(matches!(
        action.start(&mut ctx(), input),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn block_start_rejects_ws_url_with_invalid() {
    // ws:// is rejected as Invalid (not just Conflict) because the
    // kind:10006 parser would silently drop it — the user's intent
    // would not take effect.
    let cache = empty_cache();
    let action = BlockRelayAction::new(cache);
    let input = BlockRelayInput {
        url: "ws://relay.example".into(),
        account_pubkey: "alice".into(),
    };
    assert!(matches!(
        action.start(&mut ctx(), input),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn block_start_idempotent_already_blocked_returns_conflict() {
    let cache = cache_with("alice", &["wss://blocked.example"]);
    let action = BlockRelayAction::new(cache);
    let input = BlockRelayInput {
        url: "wss://blocked.example".into(),
        account_pubkey: "alice".into(),
    };
    assert!(matches!(
        action.start(&mut ctx(), input),
        Err(ActionRejection::Conflict(_))
    ));
}

#[test]
fn block_start_canonicalizes_before_idempotent_check() {
    // "wss://BLOCKED.EXAMPLE/" canonicalises to "wss://blocked.example",
    // which IS in the cache — must be detected as a Conflict.
    let cache = cache_with("alice", &["wss://blocked.example"]);
    let action = BlockRelayAction::new(cache);
    let input = BlockRelayInput {
        url: "wss://BLOCKED.EXAMPLE/".into(),
        account_pubkey: "alice".into(),
    };
    assert!(matches!(
        action.start(&mut ctx(), input),
        Err(ActionRejection::Conflict(_))
    ));
}

// ── BlockRelayAction — execute ────────────────────────────────────────

#[test]
fn block_execute_emits_publish_unsigned_event_command() {
    let cache = empty_cache();
    let action = BlockRelayAction::new(cache);
    let input = BlockRelayInput {
        url: "wss://relay.example".into(),
        account_pubkey: "alice".into(),
    };
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    action
        .execute(input, "test-cid", &|cmd| captured.borrow_mut().push(cmd))
        .expect("execute must not fail");
    let cmds = captured.into_inner();
    assert_eq!(cmds.len(), 1);
    match cmds.into_iter().next().unwrap() {
        ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id,
            ..
        }) => {
            assert_eq!(event.kind, KIND_BLOCKED_RELAYS, "must emit kind:10006");
            assert_eq!(
                correlation_id.as_deref(),
                Some("test-cid"),
                "correlation_id must thread through"
            );
        }
        other => panic!("expected PublishUnsignedEvent, got {other:?}"),
    }
}

#[test]
fn block_execute_adds_url_to_existing_set() {
    let cache = cache_with("alice", &["wss://already-blocked.example"]);
    let action = BlockRelayAction::new(cache);
    let input = BlockRelayInput {
        url: "wss://new.example".into(),
        account_pubkey: "alice".into(),
    };
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    action
        .execute(input, "cid", &|cmd| captured.borrow_mut().push(cmd))
        .unwrap();
    let cmds = captured.into_inner();
    let ActorCommand::Publish(PublishCommand::UnsignedEvent { event, .. }) = cmds.into_iter().next().unwrap()
    else {
        panic!("expected PublishUnsignedEvent");
    };
    // Both old + new URL must be in the tags.
    let tag_urls: Vec<&String> = event.tags.iter().map(|t| &t[1]).collect();
    assert!(
        tag_urls.contains(&&"wss://already-blocked.example".to_string()),
        "existing blocked relay must be preserved"
    );
    assert!(
        tag_urls.contains(&&"wss://new.example".to_string()),
        "new relay must be added"
    );
}

#[test]
fn block_execute_threads_correlation_id() {
    let cache = empty_cache();
    let action = BlockRelayAction::new(cache);
    let input = BlockRelayInput {
        url: "wss://relay.example".into(),
        account_pubkey: "alice".into(),
    };
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    action
        .execute(input, "my-spinner-id", &|cmd| captured.borrow_mut().push(cmd))
        .unwrap();
    let ActorCommand::Publish(PublishCommand::UnsignedEvent { correlation_id, .. }) =
        captured.into_inner().into_iter().next().unwrap()
    else {
        panic!("expected PublishUnsignedEvent");
    };
    assert_eq!(correlation_id.as_deref(), Some("my-spinner-id"));
}

// ── UnblockRelayAction — start ────────────────────────────────────────

#[test]
fn unblock_start_accepts_currently_blocked_url() {
    let cache = cache_with("alice", &["wss://blocked.example"]);
    let action = UnblockRelayAction::new(cache);
    let input = UnblockRelayInput {
        url: "wss://blocked.example".into(),
        account_pubkey: "alice".into(),
    };
    assert!(action.start(&mut ctx(), input).is_ok());
}

#[test]
fn unblock_start_rejects_non_wss_url() {
    let cache = empty_cache();
    let action = UnblockRelayAction::new(cache);
    let input = UnblockRelayInput {
        url: "https://relay.example".into(),
        account_pubkey: "alice".into(),
    };
    assert!(matches!(
        action.start(&mut ctx(), input),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn unblock_start_idempotent_not_blocked_returns_conflict() {
    let cache = empty_cache();
    let action = UnblockRelayAction::new(cache);
    let input = UnblockRelayInput {
        url: "wss://not-blocked.example".into(),
        account_pubkey: "alice".into(),
    };
    assert!(matches!(
        action.start(&mut ctx(), input),
        Err(ActionRejection::Conflict(_))
    ));
}

// ── UnblockRelayAction — execute ──────────────────────────────────────

#[test]
fn unblock_execute_removes_url_from_set() {
    let cache = cache_with(
        "alice",
        &["wss://blocked-a.example", "wss://blocked-b.example"],
    );
    let action = UnblockRelayAction::new(cache);
    let input = UnblockRelayInput {
        url: "wss://blocked-a.example".into(),
        account_pubkey: "alice".into(),
    };
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    action
        .execute(input, "cid", &|cmd| captured.borrow_mut().push(cmd))
        .unwrap();
    let ActorCommand::Publish(PublishCommand::UnsignedEvent { event, .. }) =
        captured.into_inner().into_iter().next().unwrap()
    else {
        panic!("expected PublishUnsignedEvent");
    };
    let tag_urls: Vec<&String> = event.tags.iter().map(|t| &t[1]).collect();
    assert!(
        !tag_urls.contains(&&"wss://blocked-a.example".to_string()),
        "removed relay must not appear in the tag list"
    );
    assert!(
        tag_urls.contains(&&"wss://blocked-b.example".to_string()),
        "other relays must be preserved"
    );
}

#[test]
fn unblock_last_entry_publishes_empty_kind_10006() {
    // Unblocking the only blocked entry → zero-tag kind:10006 (the
    // "I cleared my blocked list" signal). Must NOT silently omit the
    // publish — the ingest path removes the cache entry on an empty event.
    let cache = cache_with("alice", &["wss://last.example"]);
    let action = UnblockRelayAction::new(cache);
    let input = UnblockRelayInput {
        url: "wss://last.example".into(),
        account_pubkey: "alice".into(),
    };
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    action
        .execute(input, "cid", &|cmd| captured.borrow_mut().push(cmd))
        .unwrap();
    let ActorCommand::Publish(PublishCommand::UnsignedEvent { event, .. }) =
        captured.into_inner().into_iter().next().unwrap()
    else {
        panic!("expected PublishUnsignedEvent");
    };
    assert_eq!(event.kind, KIND_BLOCKED_RELAYS);
    assert!(
        event.tags.is_empty(),
        "unblocking the last entry must publish an empty kind:10006"
    );
}

#[test]
fn unblock_execute_threads_correlation_id() {
    let cache = cache_with("alice", &["wss://blocked.example"]);
    let action = UnblockRelayAction::new(cache);
    let input = UnblockRelayInput {
        url: "wss://blocked.example".into(),
        account_pubkey: "alice".into(),
    };
    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    action
        .execute(input, "spinner-99", &|cmd| captured.borrow_mut().push(cmd))
        .unwrap();
    let ActorCommand::Publish(PublishCommand::UnsignedEvent { correlation_id, .. }) =
        captured.into_inner().into_iter().next().unwrap()
    else {
        panic!("expected PublishUnsignedEvent");
    };
    assert_eq!(correlation_id.as_deref(), Some("spinner-99"));
}

// ── Namespace ─────────────────────────────────────────────────────────

#[test]
fn block_namespace() {
    assert_eq!(BlockRelayAction::NAMESPACE, "nmp.nip51.block_relay");
}

#[test]
fn unblock_namespace() {
    assert_eq!(UnblockRelayAction::NAMESPACE, "nmp.nip51.unblock_relay");
}
