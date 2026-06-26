//! Browser/default composition parity gates (#2061).
//!
//! Native default-composition coverage already asserts
//! `nmp_codegen::canonical_default_action_namespaces()` through the FFI-backed
//! app. These browser-runtime tests pin the same canonical namespace source and
//! the browser builder's deferred substrate slots, so #2053 cannot pass with a
//! browser start path that forgot NMP defaults or left the routing/publish
//! substrate unwired. There are no browser exclusions for canonical action
//! namespaces; signer/capability provider implementations remain an explicit
//! app/provider decision and are not installed by `register_defaults`.

use std::sync::Arc;

use crate::{BrowserAppBuilder, BrowserRunConfig};
use nmp_core::{substrate::KernelEvent, RelayFrame};
use nostr::JsonUtil;

use super::started_handle;

/// #1007 PR-7 — injection identity: a store handed to `inject_store` must be the
/// exact `Arc` the kernel reducer holds after `start()` (no wrapping, no swap).
///
/// This is the native, always-runnable analog of the wasm OPFS injection: the
/// async hook (`NmpWasmRuntime::prepare_store`) parks an `Arc<dyn EventStore>`
/// that `handle_start` feeds straight into this same `inject_store` seam, so
/// proving the seam preserves pointer identity proves the OPFS store reaches the
/// reducer intact.
#[test]
fn inject_store_reaches_reducer_with_pointer_identity() {
    let custom: Arc<dyn nmp_store::EventStore> = Arc::new(nmp_store::MemEventStore::new());

    let handle = BrowserAppBuilder::new()
        .inject_store(Arc::clone(&custom))
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();

    assert!(
        Arc::ptr_eq(&custom, &handle.event_store_handle()),
        "inject_store must hand the exact Arc to the kernel reducer — \
         the store the OPFS hook opens (#1007 PR-7) must reach the reducer unwrapped"
    );
}

/// Control: the default `in_memory()` start path must NOT alias an unrelated
/// injected store — guards the identity assertion above against a false positive.
#[test]
fn in_memory_start_does_not_alias_an_injected_store() {
    let unrelated: Arc<dyn nmp_store::EventStore> = Arc::new(nmp_store::MemEventStore::new());
    let handle = started_handle();
    assert!(
        !Arc::ptr_eq(&unrelated, &handle.event_store_handle()),
        "in_memory() start must use its own store, not some unrelated Arc"
    );
}

/// #1007 PR-8 — degraded-open diagnostic via the builder path: a reason handed
/// to `with_store_open_failure` must be recorded on the kernel and readable as
/// `store_open_failure` after `start()`. This is the builder-path analog of the
/// native LMDB `v67_store_open_failure` channel: a browser session that fell
/// back to in-memory reports the SAME Tier-3 diagnostic.
#[test]
fn with_store_open_failure_surfaces_through_the_kernel() {
    let reason = "opfs_store_open_failure: quota_denied".to_string();
    let handle = BrowserAppBuilder::new()
        .in_memory()
        .with_store_open_failure(Some(reason.clone()))
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();

    assert_eq!(
        handle.store_open_failure(),
        Some(reason),
        "with_store_open_failure must reach the kernel's Tier-3 store_open_failure diagnostic"
    );
}

/// Control: a builder that never declares a degraded reason (`None` / unset)
/// must start clean — no false-positive `store_open_failure`.
#[test]
fn healthy_in_memory_start_reports_no_store_open_failure() {
    let cleared = BrowserAppBuilder::new()
        .in_memory()
        .with_store_open_failure(None)
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();
    assert!(
        cleared.store_open_failure().is_none(),
        "an explicit None degraded reason must leave store_open_failure absent"
    );

    // And the default path (setter never called) is equally clean.
    assert!(
        started_handle().store_open_failure().is_none(),
        "a default in-memory start must not report a store_open_failure"
    );
}

const ACCOUNT_PK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FOLLOW_A_PK: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const FOLLOW_NOTE_ID: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const RELAY: &str = "wss://relay.example";

#[test]
fn browser_start_registers_every_canonical_default_action_namespace() {
    let handle = started_handle();
    let registered = handle.runtime.action_registry.action_namespaces();

    for ns in nmp_codegen::canonical_default_action_namespaces() {
        assert!(
            registered.iter().any(|registered| registered == ns),
            "browser default composition omitted canonical action namespace `{ns}`; \
             registered namespaces: {registered:?}"
        );
    }

    assert!(
        !registered
            .iter()
            .any(|ns| ns == "nmp.template.never.registered"),
        "control case: an unregistered namespace must not appear in the browser action registry"
    );
}

#[test]
fn browser_defaults_defer_required_substrate_slots_before_start() {
    let mut builder = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default());

    nmp_defaults::register_defaults(&mut builder);

    let inner = builder
        .inner
        .lock()
        .expect("browser builder mutex must not be poisoned");

    assert!(
        inner.routing_substrate_factory.is_some(),
        "browser defaults must install the routing-substrate factory"
    );
    assert!(
        inner.publish_resolver_factory.is_some(),
        "browser defaults must install the publish-resolver factory"
    );
    assert!(
        inner.mailbox_cache_reader.is_some(),
        "browser defaults must install the shared mailbox-cache reader"
    );
    assert!(
        inner.profile_lookup.is_some(),
        "browser defaults must install the profile lookup substrate"
    );
    assert!(
        inner.contacts_lookup.is_some(),
        "browser defaults must install the contacts lookup substrate"
    );
    assert!(
        inner.dm_inbox_relay_lookup.is_some(),
        "browser defaults must install the DM-inbox relay lookup substrate"
    );
    assert!(
        inner.blocked_relay_lookup.is_some(),
        "browser defaults must install the blocked-relay lookup substrate"
    );
    assert!(
        inner.coverage_hook.is_some(),
        "browser defaults must install the coverage hook substrate"
    );
}

#[test]
fn browser_home_feed_observer_opens_on_active_account_change() {
    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .set_relays(vec![(RELAY.to_string(), "both".to_string())])
        .decide_providers(BrowserRunConfig::default())
        .start();

    assert!(
        !handle.runtime.identity_change_observers.is_empty(),
        "browser start must retain identity-change observers registered during composition"
    );

    for role in [
        nmp_network::role::RelayRole::Content,
        nmp_network::role::RelayRole::Indexer,
    ] {
        let connected = handle
            .runtime
            .reducer
            .handle_relay_connected(role, RELAY, false);
        handle.fan_out_outbound(connected);
    }

    let outbound = handle.apply_set_active_account(ACCOUNT_PK.to_string());
    handle.fan_out_outbound(outbound);
    let out = handle.pump();

    let texts = out
        .outbound
        .iter()
        .map(|frame| frame.text().to_string())
        .collect::<Vec<_>>();
    assert!(
        texts.iter().any(|text| text.contains(r#""kinds":[3]"#) && text.contains(ACCOUNT_PK)),
        "active-account change must open the browser home-feed contact-list observer; outbound={texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains(r#""kinds":[1,5,6]"#) && text.contains(ACCOUNT_PK)),
        "active-account change must open the self-included browser home-feed observer; outbound={texts:?}"
    );
}

#[test]
fn browser_home_feed_projection_renders_public_note_before_sign_in() {
    let note_keys = nostr::Keys::generate();
    let note_pk = note_keys.public_key().to_hex();

    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .set_relays(vec![(RELAY.to_string(), "both,indexer".to_string())])
        .decide_providers(BrowserRunConfig::default())
        .start();

    let connected = handle.runtime.reducer.handle_relay_connected(
        nmp_network::role::RelayRole::Content,
        RELAY,
        false,
    );
    handle.fan_out_outbound(connected);

    let first_pump = handle.pump();
    let (feed_sub, req_text) =
        req_frame_for_kind(&first_pump.outbound, nmp_kinds::KIND_SHORT_TEXT_NOTE)
            .expect("signed-out browser start must open a public note subscription");
    assert!(
        !req_text.contains(r#""authors""#),
        "signed-out public feed must not require a follow author filter; req={req_text}"
    );

    let note_json = signed_note_json(&note_keys, "hello from signed-out public feed", 20);
    let note_id = serde_json::from_str::<serde_json::Value>(&note_json)
        .expect("signed note json decodes")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("signed note has id")
        .to_string();
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(relay_event_frame(&feed_sub, note_json)),
    );
    handle.fan_out_outbound(outbound);

    let frame = handle.next_frame(true);
    let feed = decode_home_feed(&frame);
    assert_eq!(
        feed.cards.len(),
        1,
        "signed-out public kind:1 note must render"
    );
    assert_eq!(feed.cards[0].card.id, note_id);
    assert_eq!(feed.cards[0].card.author_pubkey, note_pk);
    assert_eq!(
        feed.cards[0].card.content,
        "hello from signed-out public feed"
    );
}

#[test]
fn browser_home_feed_projection_renders_followed_note() {
    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();

    let outbound = handle.apply_set_active_account(ACCOUNT_PK.to_string());
    handle.fan_out_outbound(outbound);
    handle.pump();

    handle
        .runtime
        .reducer
        .fire_event_observers_for_test(&contact_list_event());
    handle.pump();

    handle
        .runtime
        .reducer
        .fire_event_observers_for_test(&follow_note_event());

    let frame = handle.next_frame(true);
    let feed = decode_home_feed(&frame);
    assert_eq!(feed.cards.len(), 1, "followed kind:1 note must render");
    assert_eq!(feed.cards[0].card.id, FOLLOW_NOTE_ID);
    assert_eq!(feed.cards[0].card.author_pubkey, FOLLOW_A_PK);
    assert_eq!(feed.cards[0].card.content, "hello from runtime composition");
}

#[test]
fn browser_home_feed_projection_renders_followed_note_from_relay_frames() {
    let viewer_keys = nostr::Keys::generate();
    let follow_keys = nostr::Keys::generate();
    let viewer_pk = viewer_keys.public_key().to_hex();
    let follow_pk = follow_keys.public_key().to_hex();

    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();

    let outbound = handle.apply_set_active_account(viewer_pk.clone());
    handle.fan_out_outbound(outbound);
    handle.pump();

    let contact_frame = relay_event_frame(
        "contact-list-sub",
        signed_kind3_json(&viewer_keys, std::slice::from_ref(&follow_pk), 10),
    );
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(contact_frame),
    );
    handle.fan_out_outbound(outbound);
    handle.pump();

    let note_json = signed_note_json(&follow_keys, "hello from relay frame", 20);
    let note_id = serde_json::from_str::<serde_json::Value>(&note_json)
        .expect("signed note json decodes")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("signed note has id")
        .to_string();
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(relay_event_frame("follow-feed-sub", note_json)),
    );
    handle.fan_out_outbound(outbound);

    let frame = handle.next_frame(true);
    let feed = decode_home_feed(&frame);
    assert_eq!(
        feed.cards.len(),
        1,
        "relay-frame-ingested followed kind:1 note must render"
    );
    assert_eq!(feed.cards[0].card.id, note_id);
    assert_eq!(feed.cards[0].card.author_pubkey, follow_pk);
    assert_eq!(feed.cards[0].card.content, "hello from relay frame");
}

#[test]
fn browser_home_feed_projection_renders_followed_note_from_runtime_wire_subs() {
    let viewer_keys = nostr::Keys::generate();
    let follow_keys = nostr::Keys::generate();
    let viewer_pk = viewer_keys.public_key().to_hex();
    let follow_pk = follow_keys.public_key().to_hex();

    let mut handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .set_relays(vec![(RELAY.to_string(), "both,indexer".to_string())])
        .decide_providers(BrowserRunConfig::default())
        .start();

    let connected = handle.runtime.reducer.handle_relay_connected(
        nmp_network::role::RelayRole::Content,
        RELAY,
        false,
    );
    handle.fan_out_outbound(connected);

    let outbound = handle.apply_set_active_account(viewer_pk.clone());
    handle.fan_out_outbound(outbound);
    let first_pump = handle.pump();
    let contact_sub = req_sub_for_kind(&first_pump.outbound, nmp_kinds::KIND_CONTACT_LIST)
        .expect("active account must open contact-list subscription");

    let contact_frame = relay_event_frame(
        &contact_sub,
        signed_kind3_json(&viewer_keys, std::slice::from_ref(&follow_pk), 10),
    );
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(contact_frame),
    );
    handle.fan_out_outbound(outbound);

    let after_contact = handle.pump();
    let feed_sub = req_sub_for_kind(&after_contact.outbound, nmp_kinds::KIND_SHORT_TEXT_NOTE)
        .expect("contact list must open followed-note subscription");

    let note_json = signed_note_json(&follow_keys, "hello through real wire sub", 20);
    let note_id = serde_json::from_str::<serde_json::Value>(&note_json)
        .expect("signed note json decodes")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("signed note has id")
        .to_string();
    let outbound = handle.runtime.reducer.handle_relay_frame(
        nmp_network::role::RelayRole::Content,
        RELAY,
        RelayFrame::Text(relay_event_frame(&feed_sub, note_json)),
    );
    handle.fan_out_outbound(outbound);

    let frame = handle.next_frame(true);
    let feed = decode_home_feed(&frame);
    assert_eq!(
        feed.cards.len(),
        1,
        "fixture-style event delivered on the runtime's emitted wire sub must render"
    );
    assert_eq!(feed.cards[0].card.id, note_id);
    assert_eq!(feed.cards[0].card.author_pubkey, follow_pk);
    assert_eq!(feed.cards[0].card.content, "hello through real wire sub");
}

fn contact_list_event() -> KernelEvent {
    KernelEvent {
        id: "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd".to_string(),
        author: ACCOUNT_PK.to_string(),
        kind: nmp_kinds::KIND_CONTACT_LIST,
        created_at: 10,
        tags: vec![vec!["p".to_string(), FOLLOW_A_PK.to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn follow_note_event() -> KernelEvent {
    KernelEvent {
        id: FOLLOW_NOTE_ID.to_string(),
        author: FOLLOW_A_PK.to_string(),
        kind: nmp_kinds::KIND_SHORT_TEXT_NOTE,
        created_at: 20,
        tags: Vec::new(),
        content: "hello from runtime composition".to_string(),
        relay_provenance: vec![RELAY.to_string()],
    }
}

fn signed_kind3_json(keys: &nostr::Keys, follows: &[String], created_at: u64) -> String {
    let tags = follows
        .iter()
        .map(|pk| nostr::Tag::parse(["p", pk.as_str()]).expect("valid p tag"))
        .collect::<Vec<_>>();
    nostr::EventBuilder::new(nostr::Kind::from(3u16), "")
        .tags(tags)
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:3")
        .as_json()
}

fn signed_note_json(keys: &nostr::Keys, content: &str, created_at: u64) -> String {
    nostr::EventBuilder::text_note(content)
        .custom_created_at(nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign note")
        .as_json()
}

fn relay_event_frame(sub_id: &str, event_json: String) -> String {
    format!(r#"["EVENT","{sub_id}",{event_json}]"#)
}

fn req_sub_for_kind(outbound: &[nmp_core::OutboundMessage], kind: u32) -> Option<String> {
    req_frame_for_kind(outbound, kind).map(|(sub_id, _)| sub_id)
}

fn req_frame_for_kind(
    outbound: &[nmp_core::OutboundMessage],
    kind: u32,
) -> Option<(String, String)> {
    outbound.iter().find_map(|message| {
        let value = serde_json::from_str::<serde_json::Value>(message.text()).ok()?;
        let arr = value.as_array()?;
        if arr.first()?.as_str()? != "REQ" {
            return None;
        }
        let sub_id = arr.get(1)?.as_str()?.to_string();
        let has_kind = arr.iter().skip(2).any(|filter| {
            filter
                .get("kinds")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|kinds| {
                    kinds
                        .iter()
                        .any(|candidate| candidate.as_u64() == Some(kind as u64))
                })
        });
        has_kind.then_some((sub_id, message.text().to_string()))
    })
}

fn decode_home_feed(frame: &crate::runtime::SnapshotOutcome) -> nmp_nip01::op_feed::OpFeedSnapshot {
    let crate::runtime::SnapshotOutcome::Frame(bytes) = frame else {
        panic!("expected snapshot frame, got {frame:?}");
    };
    let typed = nmp_core::decode_snapshot_typed_projections(bytes).expect("frame decodes");
    let row = typed
        .into_iter()
        .find(|row| row.key == nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY)
        .expect("home feed projection must be present");
    nmp_nip01::op_feed::decode_op_feed_snapshot(&row.payload).expect("NOFS payload decodes")
}
