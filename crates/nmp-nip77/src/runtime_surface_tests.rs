use negentropy::{Id, Negentropy, NegentropyStorageVector};
use nmp_core::substrate::{RelayTextInterceptor, ReqFrameContext, ReqFrameInterceptor};
use nmp_core::{Kernel, OutboundMessage};
use nmp_coverage_gate::CoverageGate;
use nmp_network::role::RelayRole;
use nmp_planner::{InterestId, InterestLifecycle};
use nmp_store::{RawEvent, VerifiedEvent};
use nostr::{ClientMessage, JsonUtil as _};

use crate::codec::{hex_decode, hex_encode};
use crate::{NegentropySyncRuntime, SyncedItem, FRAME_SIZE_LIMIT};

fn author(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

fn id(n: u8) -> [u8; 32] {
    [n; 32]
}

fn id_hex(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

fn custom_ctx(filter_json: String) -> ReqFrameContext {
    ReqFrameContext {
        role: RelayRole::Content,
        relay_url: "wss://relay.example".to_string(),
        sub_id: "sub-custom".to_string(),
        filter_json,
        interest_id: InterestId(1),
        lifecycle: InterestLifecycle::OneShot,
    }
}

#[test]
fn small_static_surfaces_fall_back_to_raw_req() {
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);

    let ids = custom_ctx(serde_json::json!({"ids":[id_hex(1), id_hex(2), id_hex(3)]}).to_string());
    assert!(
        runtime.intercept_req(&mut kernel, &ids).is_none(),
        "three exact ids are cheaper as a plain REQ"
    );

    let exact_replaceable =
        custom_ctx(serde_json::json!({"authors":[author(1)],"kinds":[0,3]}).to_string());
    assert!(
        runtime
            .intercept_req(&mut kernel, &exact_replaceable)
            .is_none(),
        "one author × two replaceable kinds has a static max of two"
    );

    let exact_addressable = custom_ctx(
        serde_json::json!({"authors":[author(1)],"kinds":[30023],"#d":["hello"]}).to_string(),
    );
    assert!(
        runtime
            .intercept_req(&mut kernel, &exact_addressable)
            .is_none(),
        "one addressable key has a static max of one"
    );
}

#[test]
fn tag_and_unbounded_filters_open_negentropy() {
    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut kernel = Kernel::testing_new(50);

    let thread = custom_ctx(serde_json::json!({"#e":[id_hex(9)]}).to_string());
    assert!(
        runtime.intercept_req(&mut kernel, &thread).is_some(),
        "#e filters can have many matches and should use NIP-77"
    );

    let author_articles =
        custom_ctx(serde_json::json!({"authors":[author(1)],"kinds":[30023]}).to_string());
    assert!(
        runtime
            .intercept_req(&mut kernel, &author_articles)
            .is_some(),
        "addressable history without #d is not statically bounded"
    );
}

#[test]
fn etag_filter_with_empty_kinds_uses_local_tagged_items() {
    let cached_id = id(0xc1);
    let missing_id = id(0xd2);
    let root = id_hex(0xe3);
    let cached_author = author(0);

    let mut kernel = Kernel::testing_new(50);
    insert_cached_event_with_tags(
        &mut kernel,
        cached_id,
        &cached_author,
        1,
        1_000,
        vec![vec!["e".into(), root.clone()]],
    );

    let runtime = NegentropySyncRuntime::new(CoverageGate::default());
    let mut ctx = custom_ctx(serde_json::json!({"#e":[root]}).to_string());
    ctx.sub_id = "sub-etag".to_string();

    let opened = runtime
        .intercept_req(&mut kernel, &ctx)
        .expect("#e filter must open NIP-77");
    assert_eq!(opened.len(), 1);

    let relay_items = vec![
        SyncedItem {
            created_at: 1_000,
            id: cached_id,
        },
        SyncedItem {
            created_at: 2_000,
            id: missing_id,
        },
    ];
    let mut server = negentropy_server(relay_items);
    let mut client_payload = client_neg_payload(opened[0].text());

    let final_out = loop {
        let server_payload = server.reconcile(&client_payload).expect("server reconcile");
        let relay_msg = format!(
            r#"["NEG-MSG","sub-etag","{}"]"#,
            hex_encode(&server_payload)
        );
        let out = runtime.on_relay_text(&mut kernel, "wss://relay.example", &relay_msg);
        if let Some(next) = out.iter().find(|msg| is_client_neg_msg(msg.text())) {
            client_payload = client_neg_payload(next.text());
        } else {
            break out;
        }
    };

    let ids_req = final_out
        .iter()
        .map(OutboundMessage::text)
        .find(|text| text.starts_with(r#"["REQ","sub-etag","#))
        .expect("relay-only #e match must be fetched by ids-only REQ");
    assert!(ids_req.contains(&id_hex(0xd2)));
    assert!(
        !ids_req.contains(&id_hex(0xc1)),
        "cached #e-tagged item must seed the local negentropy set"
    );
}

fn insert_cached_event_with_tags(
    kernel: &mut Kernel,
    id: [u8; 32],
    author: &str,
    kind: u32,
    created_at: u64,
    tags: Vec<Vec<String>>,
) {
    let raw = RawEvent {
        id: hex_encode(&id),
        pubkey: author.to_string(),
        created_at,
        kind,
        tags,
        content: String::new(),
        sig: "a".repeat(128),
    };
    kernel
        .event_store_handle()
        .insert(
            VerifiedEvent::from_raw_unchecked(raw),
            &"wss://cache.example".to_string(),
            created_at.saturating_mul(1_000),
        )
        .expect("cache insert");
}

fn negentropy_server(items: Vec<SyncedItem>) -> Negentropy<'static, NegentropyStorageVector> {
    let mut storage = NegentropyStorageVector::with_capacity(items.len());
    for item in items {
        storage
            .insert(item.created_at, Id::from_byte_array(item.id))
            .expect("server insert");
    }
    storage.seal().expect("server storage seal");
    Negentropy::owned(storage, FRAME_SIZE_LIMIT).expect("server negentropy")
}

fn client_neg_payload(text: &str) -> Vec<u8> {
    match ClientMessage::from_json(text).expect("client NIP-77 message") {
        ClientMessage::NegOpen {
            initial_message, ..
        } => hex_decode(&initial_message).expect("NEG-OPEN payload hex"),
        ClientMessage::NegMsg { message, .. } => hex_decode(&message).expect("NEG-MSG payload hex"),
        other => panic!("expected client negentropy message, got {other:?}"),
    }
}

fn is_client_neg_msg(text: &str) -> bool {
    matches!(
        ClientMessage::from_json(text),
        Ok(ClientMessage::NegMsg { .. })
    )
}
