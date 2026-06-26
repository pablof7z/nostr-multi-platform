//! Inbox drain loop for `BrowserRuntime`.
//!
//! `drain_inbox` is the single entry point called by `BrowserRuntime::pump()`.
//! It drains every available `ActorCommand` from the inbox channel without
//! blocking, applies each to the kernel via `KernelReducer::apply_actor_command`,
//! and accumulates the outbound messages the relay pool must send.
//!
//! # D4 (single-writer)
//!
//! `drain_inbox` takes `&mut KernelReducer` — exactly one call site (the
//! owning `BrowserRuntime`) may hold this borrow. The wasm32 runtime is
//! inherently single-threaded; on native tests the `&mut` borrow enforces
//! exclusion.
//!
//! # D8 (no blocking)
//!
//! `mpsc::Receiver::try_recv` is used — non-blocking. A missing inbox message
//! (`TryRecvError::Empty`) is the normal exit condition; `Disconnected` means
//! the sender side was dropped and the runtime should quiesce.

use std::sync::mpsc::{Receiver, TryRecvError};

use nmp_core::actor::ActorMail;
use nmp_core::{CommandApplyOutcome, KernelReducer, OutboundMessage};

/// Drain the inbox and apply each command, returning aggregated outbound frames.
///
/// Loops until the channel is empty (`TryRecvError::Empty`) or disconnected.
/// Relay messages (`ActorMail::Relay`) are skipped — those are native-only and
/// should never appear on the wasm / browser path (the browser relay pool
/// drives events differently). Only `ActorMail::Command` items are applied.
pub(super) fn drain_inbox(
    reducer: &mut KernelReducer,
    rx: &Receiver<ActorMail>,
) -> Vec<OutboundMessage> {
    let mut outbound: Vec<OutboundMessage> = Vec::new();

    loop {
        match rx.try_recv() {
            Ok(ActorMail::Command(cmd)) => {
                let outcome = reducer.apply_actor_command(cmd);
                match outcome {
                    CommandApplyOutcome::Applied(msgs) => {
                        outbound.extend(msgs);
                    }
                    CommandApplyOutcome::NeedsSign { .. } => {
                        // Browser sign round-trips are handled via a separate
                        // async signer callback seam (future #2057 follow-on).
                        // For now the command is acknowledged but not signed.
                    }
                    CommandApplyOutcome::Unsupported { .. } => {
                        // D6 — unsupported commands are silently dropped on the
                        // browser path (no actor-thread error log available).
                    }
                }
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => break,
        }
    }

    outbound
}
