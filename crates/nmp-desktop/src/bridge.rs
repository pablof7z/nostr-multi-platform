//! In-process kernel bridge.
//!
//! The desktop shell runs the kernel actor on its own thread (Rust→Rust — no
//! FFI boundary) and talks to it through the generic `ActorCommand` channel,
//! exactly as the iOS FFI layer does. A reader thread drains FlatBuffers
//! update frames, decodes snapshots, and pushes them into the iced message
//! stream via a subscription.

use std::collections::HashMap;
use std::sync::mpsc::Sender;

use iced::stream;
use iced::futures::SinkExt;
use iced::futures::channel::mpsc;
use nmp_core::testing::{spawn_actor, ActorCommand};
use nmp_core::{decode_update_frame, UpdateEnvelope};

use crate::message::Message;
use crate::snapshot::Snapshot;

fn bridge_stream() -> impl iced::futures::Stream<Item = Message> {
    stream::channel(100, |mut output: mpsc::Sender<Message>| async move {
        let (tx, rx) = spawn_actor();

        // Hand the command sender to the UI so it can publish / sign-in / etc.
        let tx2 = tx.clone();
        let _ = output.send(Message::BridgeReady(tx2)).await;

        // Start with the kernel defaults (80 visible items, 4 Hz emit).
        let _ = tx.send(ActorCommand::Start {
            visible_limit: 80,
            emit_hz: 4,
        });

        // Bridge the actor's std::sync::mpsc receiver into the async channel.
        let mut output2 = output.clone();
        std::thread::spawn(move || {
            for frame in rx {
                let env = match decode_update_frame(&frame) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let UpdateEnvelope::Snapshot(v) = env else {
                    continue;
                };
                let snap: Snapshot = match serde_json::from_value(v) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if output2.try_send(Message::SnapshotUpdated(snap)).is_err() {
                    break;
                }
            }
        });

        // Keep the subscription alive until the app exits.
        std::future::pending::<()>().await;
    })
}

/// Create a subscription that spawns the kernel actor and bridges its output
/// into iced [`Message`]s.
pub fn subscription() -> iced::Subscription<Message> {
    iced::Subscription::run(bridge_stream)
}

// ---------------------------------------------------------------------------
// Action helpers — thin wrappers around `ActorCommand` sends.
// ---------------------------------------------------------------------------

/// Publish a kind:1 note with the active account.
pub fn publish_note(tx: &Sender<ActorCommand>, content: String) {
    let _ = tx.send(ActorCommand::PublishNote {
        content,
        reply_to_id: None,
        target: nmp_core::publish::PublishTarget::Auto,
        correlation_id: None,
    });
}

/// Generate a fresh keypair and sign in with it.
pub fn create_account(
    tx: &Sender<ActorCommand>,
    profile: HashMap<String, String>,
    relays: Vec<(String, String)>,
) {
    let _ = tx.send(ActorCommand::CreateAccount {
        profile,
        relays,
        mls: false,
    });
}

/// Sign in with an existing `nsec…` / hex secret.
pub fn sign_in_nsec(tx: &Sender<ActorCommand>, secret: String) {
    let _ = tx.send(ActorCommand::SignInNsec {
        secret: zeroize::Zeroizing::new(secret),
    });
}

/// (Re)open the following-timeline for the active account.
pub fn open_timeline(tx: &Sender<ActorCommand>) {
    let _ = tx.send(ActorCommand::OpenContactListSubscription {
        kinds: std::collections::BTreeSet::from([1u32, 6u32]),
    });
}
