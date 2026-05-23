use super::*;

fn entry(url: &str, marker: RelayMarker) -> RelayListEntry {
    RelayListEntry {
        url: url.to_string(),
        marker,
    }
}

// --- builder ---------------------------------------------------------

#[test]
fn build_produces_kind_10002() {
    let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Both)]);
    assert_eq!(event.kind, 10002);
}

#[test]
fn build_uses_created_at_zero_sentinel() {
    let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Both)]);
    assert_eq!(
        event.created_at, 0,
        "D7: created_at is the 0 sentinel — the actor re-stamps it"
    );
}

#[test]
fn build_leaves_pubkey_empty_for_actor_to_fill() {
    let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Both)]);
    assert!(
        event.pubkey.is_empty(),
        "pubkey is a placeholder — the actor derives it from the signing key"
    );
}

#[test]
fn build_both_marker_omits_third_tag_element() {
    // NIP-65: `["r", url]` (no third element) is the canonical
    // "read + write" form. The kernel parser's `.unwrap_or("both")`
    // branch hits this directly.
    let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Both)]);
    assert_eq!(
        event.tags,
        vec![vec!["r".to_string(), "wss://relay.example".to_string()]],
    );
}

#[test]
fn build_read_marker_emits_read_third_element() {
    let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Read)]);
    assert_eq!(
        event.tags,
        vec![vec![
            "r".to_string(),
            "wss://relay.example".to_string(),
            "read".to_string()
        ]],
    );
}

#[test]
fn build_write_marker_emits_write_third_element() {
    let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Write)]);
    assert_eq!(
        event.tags,
        vec![vec![
            "r".to_string(),
            "wss://relay.example".to_string(),
            "write".to_string()
        ]],
    );
}

#[test]
fn build_uses_r_marker_not_relay_marker() {
    // NIP-65 § uses `["r", url]` tags. Using `["relay", ...]` would be
    // a kind:10050 NIP-17 shape; the kernel's `parse_relay_list` would
    // skip every tag and the round-trip would silently produce an
    // empty cache entry — exactly the kind of leak this test pins.
    let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Both)]);
    for tag in &event.tags {
        assert_eq!(
            tag.first().map(String::as_str),
            Some("r"),
            "NIP-65 tag marker is 'r' (not 'relay' — that is NIP-17 / kind:10050)"
        );
    }
}

#[test]
fn build_preserves_input_order() {
    let event = build_relay_list_event(&[
        entry("wss://b.example", RelayMarker::Both),
        entry("wss://a.example", RelayMarker::Both),
        entry("wss://c.example", RelayMarker::Both),
    ]);
    let urls: Vec<&String> = event.tags.iter().map(|t| &t[1]).collect();
    assert_eq!(
        urls,
        vec!["wss://b.example", "wss://a.example", "wss://c.example"]
    );
}

#[test]
fn build_dedups_equivalent_urls() {
    // `wss://Relay.Example/` and `wss://relay.example` canonicalise to
    // the same value — only one tag should appear. Dedup is by
    // canonical URL only, so the FIRST marker wins.
    let event = build_relay_list_event(&[
        entry("wss://Relay.Example/", RelayMarker::Read),
        entry("wss://relay.example", RelayMarker::Write),
    ]);
    assert_eq!(event.tags.len(), 1);
    assert_eq!(
        event.tags[0],
        vec![
            "r".to_string(),
            "wss://relay.example".to_string(),
            "read".to_string(),
        ]
    );
}

#[test]
fn build_canonicalises_scheme_and_host() {
    let event =
        build_relay_list_event(&[entry("WSS://Relay.Example", RelayMarker::Both)]);
    assert_eq!(
        event.tags,
        vec![vec!["r".to_string(), "wss://relay.example".to_string()]]
    );
}

#[test]
fn build_strips_trailing_slash_on_empty_path_only() {
    let trimmed = build_relay_list_event(&[entry("wss://relay.example/", RelayMarker::Both)]);
    assert_eq!(trimmed.tags[0][1], "wss://relay.example");
    let preserved =
        build_relay_list_event(&[entry("wss://relay.example/nostr/", RelayMarker::Both)]);
    assert_eq!(preserved.tags[0][1], "wss://relay.example/nostr/");
}

#[test]
fn build_drops_non_ws_wss_urls() {
    let event = build_relay_list_event(&[
        entry("http://relay.example", RelayMarker::Both),
        entry("wss://good.example", RelayMarker::Both),
    ]);
    assert_eq!(event.tags.len(), 1);
    assert_eq!(event.tags[0][1], "wss://good.example");
}

#[test]
fn build_drops_malformed_urls() {
    let event = build_relay_list_event(&[
        entry("not a url", RelayMarker::Both),
        entry("wss://", RelayMarker::Both),
        entry("wss://good.example", RelayMarker::Both),
    ]);
    assert_eq!(event.tags.len(), 1);
    assert_eq!(event.tags[0][1], "wss://good.example");
}

#[test]
fn build_with_empty_input_produces_event_with_no_tags() {
    // The builder itself is total — it never panics. The empty-tag
    // guard is the action validator's job, not the builder's.
    let event = build_relay_list_event(&[]);
    assert_eq!(event.kind, 10002);
    assert!(event.tags.is_empty());
}

#[test]
fn build_emits_mixed_markers_in_input_order() {
    let event = build_relay_list_event(&[
        entry("wss://outbox.example", RelayMarker::Write),
        entry("wss://both.example", RelayMarker::Both),
        entry("wss://inbox.example", RelayMarker::Read),
    ]);
    assert_eq!(
        event.tags,
        vec![
            vec![
                "r".to_string(),
                "wss://outbox.example".to_string(),
                "write".to_string(),
            ],
            vec!["r".to_string(), "wss://both.example".to_string()],
            vec![
                "r".to_string(),
                "wss://inbox.example".to_string(),
                "read".to_string(),
            ],
        ],
    );
}

// --- action -----------------------------------------------------------

#[test]
fn namespace_is_nmp_nip65_publish_relay_list() {
    assert_eq!(
        PublishRelayListAction::NAMESPACE,
        "nmp.nip65.publish_relay_list",
    );
}

fn ctx() -> ActionContext {
    ActionContext::default()
}

#[test]
fn start_accepts_a_non_empty_relay_list() {
    let input = PublishRelayListInput {
        relays: vec![entry("wss://relay.example", RelayMarker::Both)],
    };
    assert!(PublishRelayListAction::start(&mut ctx(), input).is_ok());
}

#[test]
fn start_rejects_empty_relay_list() {
    let input = PublishRelayListInput { relays: Vec::new() };
    assert!(matches!(
        PublishRelayListAction::start(&mut ctx(), input),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn start_rejects_input_that_produces_zero_canonical_tags() {
    // All inputs malformed — would build an empty-tag kind:10002 which
    // ingest treats as "clear the cache". The validator must catch
    // this before the actor publishes a destructive event.
    let input = PublishRelayListInput {
        relays: vec![
            entry("not a url", RelayMarker::Both),
            entry("http://nope", RelayMarker::Both),
        ],
    };
    assert!(matches!(
        PublishRelayListAction::start(&mut ctx(), input),
        Err(ActionRejection::Invalid(_))
    ));
}

#[test]
fn execute_emits_kind10002_publish_unsigned_event_command() {
    use nmp_core::ActorCommand;
    use std::cell::RefCell;

    let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
    let input = PublishRelayListInput {
        relays: vec![entry("wss://relay.example", RelayMarker::Both)],
    };
    PublishRelayListAction::execute(input, "test-cid", &|cmd| {
        captured.borrow_mut().push(cmd);
    })
    .expect("execute must not fail");
    let cmds = captured.into_inner();
    assert_eq!(cmds.len(), 1, "executor must send exactly one command, got {cmds:?}");
    match cmds.into_iter().next().unwrap() {
        ActorCommand::PublishUnsignedEvent { event, correlation_id } => {
            assert_eq!(event.kind, 10002, "relay list must emit kind:10002");
            assert_eq!(correlation_id.as_deref(), Some("test-cid"),
                "correlation_id must thread through so the host spinner closes");
        }
        other => panic!("expected PublishUnsignedEvent, got {other:?}"),
    }
}

/// Round-trip shape contract: the tag shape the builder produces here
/// must match what `nmp-core::kernel::nostr::parse_relay_list`
/// accepts. The parser is `pub(super)` inside `nmp-core`, so we mirror
/// its core acceptance rules here (tag[0] == "r", url starts with
/// "wss://", optional third element ∈ {"read","write"}) and assert the
/// builder output satisfies them. If either side drifts, this test
/// breaks.
#[test]
fn build_event_tags_match_kernel_ingest_shape() {
    let event = build_relay_list_event(&[
        entry("wss://a.example", RelayMarker::Both),
        entry("wss://b.example", RelayMarker::Read),
        entry("wss://c.example", RelayMarker::Write),
    ]);
    for tag in &event.tags {
        assert!(
            tag.len() == 2 || tag.len() == 3,
            "tag must be ['r', url] or ['r', url, marker]; got {:?}",
            tag,
        );
        assert_eq!(tag[0], "r", "NIP-65 tag marker is 'r'");
        assert!(
            tag[1].starts_with("wss://"),
            "ingest parser requires wss:// prefix; got {}",
            tag[1],
        );
        if tag.len() == 3 {
            assert!(
                tag[2] == "read" || tag[2] == "write",
                "third element must be 'read' or 'write' (any other value \
                 is parsed as 'both' but would not survive a round trip)",
            );
        }
    }
}
