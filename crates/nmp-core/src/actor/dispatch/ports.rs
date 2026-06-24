//! #1927 — narrow command-family port bundles.
//!
//! A command-family `dispatch` fn that does not need the full actor runtime
//! bag receives one of these instead of `&mut ActorContext`. Each port is a
//! set of reborrows of the `ActorContext` fields that family actually touches,
//! produced by `ActorContext::protocol_ports` / `interests_ports`. The borrow
//! checker therefore proves — at compile time — that e.g. a Protocol command
//! cannot reach `relay_runtime`, `pool`, or any other field not named here.

use std::sync::mpsc::Sender;
use std::time::Instant;

use crate::kernel::Kernel;
use crate::update_envelope::UpdateFrameBytes;

use super::super::commands::IdentityRuntime;
use super::ActorConfig;

/// Exactly the fields `cmd_protocol::protocol` consumes (7).
///
/// `'p` ties every reborrow to the parent `ActorContext`'s borrow. The arm
/// moves `kernel` (the `&mut Kernel`) into a stack-local `RefCell` whose
/// lifetime is strictly shorter than `'p`; no `RefCell` is carried here, so
/// there is no second alias of the kernel.
pub(super) struct ProtocolPorts<'p> {
    pub(super) kernel: &'p mut Kernel,
    pub(super) identity: &'p IdentityRuntime,
    pub(super) command_tx_self: &'p crate::actor::CommandSender,
    pub(super) config: &'p ActorConfig,
    pub(super) update_tx: &'p Sender<UpdateFrameBytes>,
    pub(super) last_emit: &'p mut Instant,
    pub(super) running: bool,
}

/// Exactly the fields `cmd_interests` consumes (4).
pub(super) struct InterestsPorts<'p> {
    pub(super) kernel: &'p mut Kernel,
    pub(super) update_tx: &'p Sender<UpdateFrameBytes>,
    pub(super) last_emit: &'p mut Instant,
    pub(super) running: bool,
}
