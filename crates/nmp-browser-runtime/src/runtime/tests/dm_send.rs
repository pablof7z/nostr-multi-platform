use std::sync::Arc;

use nmp_core::dispatch_envelope::encode_dispatch_envelope;
use nmp_core::substrate::{DmInboxRelayLookup, TestDmInboxRelayCache};
use nmp_core::RelayFrame;
use nmp_signer_iface::{SignerOp, UnsignedEvent};
use nmp_signers::LocalKeySigner;

use super::*;

fn send_dm_payload_bytes(recipient_pubkey: &str, content: &str, reply_to: Option<&str>) -> Vec<u8> {
    use flatbuffers::{FlatBufferBuilder, VOffsetT, WIPOffset};

    let mut fbb = FlatBufferBuilder::new();
    let recipient = fbb.create_string(recipient_pubkey);
    let content = fbb.create_string(content);
    let reply_to = reply_to.map(|value| fbb.create_string(value));
    let start = fbb.start_table();
    fbb.push_slot::<u32>(4 as VOffsetT, 1, 0);
    fbb.push_slot_always::<WIPOffset<&str>>(6 as VOffsetT, recipient);
    fbb.push_slot_always::<WIPOffset<&str>>(8 as VOffsetT, content);
    if let Some(reply_to) = reply_to {
        fbb.push_slot_always::<WIPOffset<&str>>(10 as VOffsetT, reply_to);
    }
    let root = fbb.end_table(start);
    fbb.finish(root, Some("N17S"));
    fbb.finished_data().to_vec()
}

#[test]
fn dispatch_bytes_nip17_send_drains_to_two_giftwrap_outbound_frames() {
    let (mut handle, sender_pubkey) = handle_with_local_key_signer();
    let recipient = LocalKeySigner::from_secret_hex(&"12".repeat(32)).expect("valid secret");
    let recipient_pubkey = recipient.pubkey().to_hex();

    let dm_relays = Arc::new(TestDmInboxRelayCache::new());
    dm_relays.upsert(&recipient_pubkey, &["wss://dm.example"]);
    dm_relays.upsert(&sender_pubkey, &["wss://dm.example"]);
    handle
        .runtime
        .reducer
        .set_dm_inbox_relay_lookup(dm_relays as Arc<dyn DmInboxRelayLookup>);

    let payload =
        send_dm_payload_bytes(&recipient_pubkey, "browser runtime sends a NIP-17 DM", None);
    let bytes = encode_dispatch_envelope("cid-nip17-send", "nmp.nip17.send", 1, &payload);

    let applied = handle.apply_dispatch_bytes(&bytes);
    assert!(
        matches!(applied, crate::runtime::DispatchBytesResult::Applied { .. }),
        "dispatch must accept the typed NIP-17 send bytes: {applied:?}"
    );

    let out = handle.pump();
    let giftwrap_events = out
        .outbound
        .iter()
        .filter(|msg| msg.text().starts_with("[\"EVENT\"") && msg.text().contains("\"kind\":1059"))
        .count();
    assert_eq!(
        giftwrap_events,
        2,
        "recipient and self-copy gift-wrap EVENT frames must be emitted: {:?}",
        out.outbound
            .iter()
            .map(|msg| msg.text())
            .collect::<Vec<_>>()
    );
}

#[test]
fn dispatch_bytes_nip17_send_uses_kind10050_ingested_from_relay_frames() {
    const RELAY: &str = "wss://dm.example";

    let (mut handle, _sender_pubkey) = handle_with_local_key_signer();
    let recipient = LocalKeySigner::from_secret_hex(&"12".repeat(32)).expect("valid secret");
    let recipient_pubkey = recipient.pubkey().to_hex();

    for frame in [
        relay_event_frame(
            "sender-10050",
            signed_kind10050_json(
                &LocalKeySigner::from_secret_hex(&"ee".repeat(32)).expect("valid secret"),
                RELAY,
            ),
        ),
        relay_event_frame("recipient-10050", signed_kind10050_json(&recipient, RELAY)),
    ] {
        let outbound = handle.runtime.reducer.handle_relay_frame(
            nmp_network::role::RelayRole::Indexer,
            RELAY,
            RelayFrame::Text(frame),
        );
        handle.fan_out_outbound(outbound);
    }

    let payload = send_dm_payload_bytes(
        &recipient_pubkey,
        "browser runtime sends with relay-ingested kind10050 state",
        None,
    );
    let bytes = encode_dispatch_envelope("cid-nip17-send", "nmp.nip17.send", 1, &payload);

    let applied = handle.apply_dispatch_bytes(&bytes);
    assert!(
        matches!(applied, crate::runtime::DispatchBytesResult::Applied { .. }),
        "dispatch must accept send after relay-ingested kind:10050 lists: {applied:?}"
    );

    let out = handle.pump();
    let giftwrap_events = out
        .outbound
        .iter()
        .filter(|msg| msg.text().starts_with("[\"EVENT\"") && msg.text().contains("\"kind\":1059"))
        .count();
    assert_eq!(
        giftwrap_events,
        2,
        "recipient and self-copy gift-wrap EVENT frames must be emitted from relay-ingested cache: {:?}",
        out.outbound
            .iter()
            .map(|msg| msg.text())
            .collect::<Vec<_>>()
    );
}

fn signed_kind10050_json(signer: &LocalKeySigner, relay: &str) -> String {
    let unsigned = UnsignedEvent {
        pubkey: String::new(),
        kind: 10_050,
        tags: vec![vec!["relay".to_string(), relay.to_string()]],
        content: String::new(),
        created_at: 1,
    };
    match signer.sign(unsigned) {
        SignerOp::Ready(Ok(signed)) => signed.to_nip01_json(),
        SignerOp::Ready(Err(err)) => panic!("kind:10050 sign failed: {err}"),
        SignerOp::Pending(_) => panic!("local signer must complete synchronously"),
    }
}

fn relay_event_frame(sub_id: &str, event_json: String) -> String {
    format!(r#"["EVENT","{sub_id}",{event_json}]"#)
}
