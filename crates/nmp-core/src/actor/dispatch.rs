//! Command + relay-event dispatch reducers.
//!
//! Split out of `mod.rs` to keep both files under the 300-LOC soft cap.
//! `dispatch_command` resolves an [`ActorCommand`] into outbound relay
//! messages (or `None` for shutdown); `handle_relay_event` folds a
//! [`RelayEvent`] into the kernel + connection bookkeeping. No behavior
//! change — pure move of the two reducers off the actor loop.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::kernel::Kernel;
use crate::relay::{OutboundMessage, RelayRole};
use crate::relay_worker::RelayEvent;

use super::commands::{self, IdentityRuntime};
use super::kernel_action::dispatch_kernel_action;
use super::relay_mgmt::{
    close_relays, maybe_send_startup, send_all_outbound, spawn_missing_relays,
};
use super::tick::{emit_kernel_update, emit_now};
use super::{ActorCommand, RelayControl};

#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_command(
    command: ActorCommand,
    kernel: &mut Kernel,
    identity: &mut IdentityRuntime,
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    connected_relays: &mut HashSet<RelayRole>,
    update_tx: &Sender<String>,
    last_emit: &mut Instant,
    next_relay_generation: &mut u64,
    running: &mut bool,
    emit_hz: &mut u32,
    startup_sent: &mut bool,
    relays_ready: bool,
) -> Option<Vec<OutboundMessage>> {
    match command {
        ActorCommand::Start {
            visible_limit,
            emit_hz: hz,
        } => {
            *running = true;
            *emit_hz = hz;
            *startup_sent = false;
            kernel.set_visible_limit(visible_limit);
            kernel.start();
            spawn_missing_relays(relay_controls, relay_tx, kernel, next_relay_generation);
            emit_now(kernel, *running, update_tx, last_emit);
            // T127: boot-resume for the publish engine. Closes Residual 3
            // from T117 — `accepted_locally` rows persisted by a previous
            // process come back as `InFlight` and any due retries dispatch
            // immediately. Today the production store is fresh in-memory
            // per process so this is a no-op; once the M3 LMDB store lands
            // the resume call will drive the resurrected rows back through
            // the actor's normal outbound path. `spawn_missing_relays`
            // above ran first, so workers will spawn on demand for any
            // URL the resumed frames target (idempotent via
            // `ensure_relay_worker`). Frames flow through the regular
            // `send_all_outbound` call in `run_actor`.
            Some(kernel.resume_publish_engine())
        }
        ActorCommand::Configure {
            visible_limit,
            emit_hz: hz,
        } => {
            *emit_hz = hz;
            kernel.set_visible_limit(visible_limit);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::OpenAuthor { pubkey } => {
            let outbound = kernel.open_author(pubkey, relays_ready);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::OpenThread { event_id } => {
            let outbound = kernel.open_thread(event_id, relays_ready);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::OpenFirehoseTag { tag } => {
            let outbound = kernel.open_firehose_tag(tag, relays_ready);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::ClaimProfile {
            pubkey,
            consumer_id,
        } => {
            let outbound = kernel.claim_profile(pubkey, consumer_id, relays_ready);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::ReleaseProfile {
            pubkey,
            consumer_id,
        } => {
            let outbound = kernel.release_profile(&pubkey, &consumer_id);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::CloseAuthor { pubkey } => {
            let outbound = kernel.close_author(&pubkey);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::CloseThread { event_id } => {
            let outbound = kernel.close_thread(&event_id);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::SignInNsec { secret } => {
            let outbound = commands::sign_in_nsec(identity, kernel, &secret, relays_ready);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::SignInBunker { uri } => {
            commands::sign_in_bunker(kernel, &uri);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::CreateAccount => {
            let outbound = commands::create_account(identity, kernel, relays_ready);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::SwitchActive { identity_id } => {
            let outbound =
                commands::switch_active(identity, kernel, &identity_id, relays_ready);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::RemoveAccount { identity_id } => {
            let outbound = commands::remove_account(identity, kernel, &identity_id);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::PublishNote {
            content,
            reply_to_id,
        } => {
            let outbound =
                commands::publish_note(identity, kernel, &content, reply_to_id.as_deref());
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::PublishUnsignedEvent(unsigned) => {
            let outbound = commands::publish_unsigned_event(identity, kernel, unsigned);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::React {
            target_event_id,
            reaction,
        } => {
            let outbound =
                commands::react(identity, kernel, &target_event_id, &reaction);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::Follow { pubkey } => {
            let outbound = commands::follow(identity, kernel, &pubkey, true);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::Unfollow { pubkey } => {
            let outbound = commands::follow(identity, kernel, &pubkey, false);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::AddRelay { url, role } => {
            commands::add_relay(kernel, &url, &role);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::RemoveRelay { url } => {
            commands::remove_relay(kernel, &url);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::OpenTimeline => {
            let outbound = commands::open_timeline(identity, kernel, relays_ready);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(outbound)
        }
        ActorCommand::Kernel(action) => {
            let update = dispatch_kernel_action(kernel, action);
            // Discrete FFI update: emit as the tagged `{"t":"update","v":…}`
            // envelope so consumers decode the single `UpdateEnvelope` type
            // (D6 — the tag is the discriminant, no key sniffing).
            emit_kernel_update(&update, update_tx);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::Stop => {
            *running = false;
            *startup_sent = false;
            close_relays(relay_controls, connected_relays, kernel);
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::Reset => {
            close_relays(relay_controls, connected_relays, kernel);
            *kernel = Kernel::new(kernel.visible_limit());
            *startup_sent = false;
            if *running {
                kernel.start();
                spawn_missing_relays(relay_controls, relay_tx, kernel, next_relay_generation);
            }
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
        ActorCommand::Shutdown => {
            close_relays(relay_controls, connected_relays, kernel);
            None
        }
        #[cfg(any(test, feature = "test-support"))]
        ActorCommand::IngestPreVerifiedEvents(events) => {
            // D4 (single writer per fact): actor thread is the sole mutator.
            // Routes each event through kernel.ingest_pre_verified_event under the
            // "diag-firehose-stress" sub-id.  Note: ingest_pre_verified_event does
            // NOT call should_store_event or ingest_timeline_event — it directly
            // calls store.insert + populates the read-cache (events HashMap + timeline).
            // sort_timeline() is deferred to after the loop to avoid O(n²·log n)
            // cost for large batches (e.g. S3: 100k events).
            for verified in events {
                kernel.ingest_pre_verified_event(
                    crate::relay::RelayRole::Content,
                    "diag-firehose-stress",
                    verified,
                );
            }
            // One sort after all events are ingested: O(n log n) not O(n²·log n).
            kernel.sort_timeline_deferred();
            emit_now(kernel, *running, update_tx, last_emit);
            Some(Vec::new())
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_relay_event(
    event: RelayEvent,
    kernel: &mut Kernel,
    relay_controls: &mut HashMap<String, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    next_relay_generation: &mut u64,
    connected_relays: &mut HashSet<RelayRole>,
    update_tx: &Sender<String>,
    last_emit: &mut Instant,
    startup_sent: &mut bool,
    running: bool,
) {
    match event {
        RelayEvent::Connected { role, .. } => {
            connected_relays.insert(role);
            kernel.relay_connected(role);
            maybe_send_startup(
                running,
                startup_sent,
                connected_relays,
                relay_controls,
                relay_tx,
                kernel,
                next_relay_generation,
            );
            emit_now(kernel, running, update_tx, last_emit);
        }
        RelayEvent::Failed { role, error, .. } => {
            connected_relays.remove(&role);
            *startup_sent = false;
            kernel.relay_failed(role, error);
            emit_now(kernel, running, update_tx, last_emit);
        }
        RelayEvent::Closed { role, .. } => {
            connected_relays.remove(&role);
            *startup_sent = false;
            kernel.relay_closed(role);
            emit_now(kernel, running, update_tx, last_emit);
        }
        RelayEvent::Message {
            role,
            relay_url,
            message,
            ..
        } if running => {
            let mut outbound = kernel.handle_message(role, &relay_url, message);
            outbound.extend(kernel.pending_view_requests());
            send_all_outbound(
                relay_controls,
                relay_tx,
                kernel,
                next_relay_generation,
                outbound,
            );
        }
        RelayEvent::Message { .. } => {}
    }
}
