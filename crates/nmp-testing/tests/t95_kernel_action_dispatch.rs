//! T95 end-to-end: `ActorCommand::Kernel(KernelAction::OpenUri)` through the
//! spawned actor must surface the discrete `KernelUpdate` JSON on the update
//! channel. The reducer's unit tests in `nmp-core` `actor/kernel_action.rs`
//! cover pure routing + registry registration; this closes the channel-wiring
//! loop the T95 task names ("Integration test: dispatch KernelAction::OpenUri").

use nmp_core::nip19::{encode_npub, encode_nsec};
use nmp_core::testing::{spawn_actor, ActorCommand};
use nmp_core::KernelAction;
use std::time::{Duration, Instant};

const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

/// Pull JSON updates until one satisfies `pred` or the 5 s deadline lapses.
fn recv_until(
    rx: &std::sync::mpsc::Receiver<String>,
    pred: impl Fn(&str) -> bool,
) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(json) if pred(&json) => return Some(json),
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    None
}

#[test]
fn open_uri_npub_emits_view_opened_through_actor_channel() {
    let (tx, rx) = spawn_actor();
    tx.send(ActorCommand::Start {
        visible_limit: 100,
        emit_hz: 0,
    })
    .expect("send Start");

    let npub = encode_npub(PK).unwrap();
    tx.send(ActorCommand::Kernel(KernelAction::OpenUri {
        uri: format!("nostr:{npub}"),
    }))
    .expect("send Kernel(OpenUri)");

    let json = recv_until(&rx, |j| j.contains("ViewOpened"))
        .expect("actor must emit a ViewOpened KernelUpdate within 5 s");
    let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    assert_eq!(v["ViewOpened"]["namespace"], "profile");
    assert_eq!(v["ViewOpened"]["key"], PK);

    tx.send(ActorCommand::Shutdown).ok();
}

#[test]
fn open_uri_nsec_emits_uri_rejected_through_actor_channel() {
    let (tx, rx) = spawn_actor();
    tx.send(ActorCommand::Start {
        visible_limit: 100,
        emit_hz: 0,
    })
    .expect("send Start");

    let nsec = encode_nsec(PK).unwrap();
    tx.send(ActorCommand::Kernel(KernelAction::OpenUri {
        uri: format!("nostr:{nsec}"),
    }))
    .expect("send Kernel(OpenUri)");

    let json = recv_until(&rx, |j| j.contains("UriRejected"))
        .expect("actor must emit a UriRejected KernelUpdate within 5 s");
    let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
    let reason = v["UriRejected"]["reason"].as_str().expect("reason string");
    assert!(
        reason.contains("not routable"),
        "stable app-noun-free reason, got: {reason}"
    );

    tx.send(ActorCommand::Shutdown).ok();
}
