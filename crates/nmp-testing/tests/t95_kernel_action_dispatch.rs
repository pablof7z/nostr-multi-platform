//! T95 end-to-end: `ActorCommand::Kernel(KernelAction::OpenUri)` through the
//! spawned actor must surface the discrete `KernelUpdate` JSON on the update
//! channel. The reducer's unit tests in `nmp-core` `actor/kernel_action.rs`
//! cover pure routing + registry registration; this closes the channel-wiring
//! loop the T95 task names ("Integration test: dispatch KernelAction::OpenUri").
//!
//! Every frame on the channel is wrapped per ADR-0001 (T103) as
//! `{"t":"update"|"snapshot","v":…}`; the test decodes through the canonical
//! `UpdateEnvelope` discriminated type — never by key-sniffing — so that the
//! discrete-vs-snapshot split is exercised end-to-end alongside the OpenUri
//! routing this test was created for.

use nmp_core::nip19::{encode_npub, encode_nsec};
use nmp_core::testing::{spawn_actor, ActorCommand};
use nmp_core::{KernelAction, KernelUpdate, UpdateEnvelope};
use std::time::{Duration, Instant};

const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

/// Pull discrete `KernelUpdate` frames from the channel until `pred` accepts
/// one or the 5 s deadline lapses.  Decoding flows through `UpdateEnvelope`
/// (ADR-0001 / T103): snapshot frames are skipped on the tag, never sniffed.
fn recv_update_until(
    rx: &std::sync::mpsc::Receiver<String>,
    pred: impl Fn(&KernelUpdate) -> bool,
) -> Option<KernelUpdate> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => {
                let envelope: UpdateEnvelope = serde_json::from_str(&frame).unwrap_or_else(|e| {
                    panic!(
                        "every channel frame must decode as UpdateEnvelope (ADR-0001 / T103) — got error {e} on frame: {frame}"
                    )
                });
                if let UpdateEnvelope::Update(update) = envelope {
                    if pred(&update) {
                        return Some(update);
                    }
                }
            }
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

    let update = recv_update_until(&rx, |u| matches!(u, KernelUpdate::ViewOpened { .. }))
        .expect("actor must emit a ViewOpened KernelUpdate within 5 s");
    match update {
        KernelUpdate::ViewOpened { namespace, key } => {
            assert_eq!(namespace, "profile");
            assert_eq!(key, PK);
        }
        other => panic!("expected ViewOpened, got {other:?}"),
    }

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

    let update = recv_update_until(&rx, |u| matches!(u, KernelUpdate::UriRejected { .. }))
        .expect("actor must emit a UriRejected KernelUpdate within 5 s");
    match update {
        KernelUpdate::UriRejected { reason, .. } => {
            assert!(
                reason.contains("not routable"),
                "stable app-noun-free reason, got: {reason}"
            );
        }
        other => panic!("expected UriRejected, got {other:?}"),
    }

    tx.send(ActorCommand::Shutdown).ok();
}
