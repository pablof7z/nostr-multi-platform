//! ADR-0052 §D4 (K2 rung 5.4) — guarantee #1: WHOLE-BODY panic isolation on
//! the `ActorCommand::Protocol` dispatch arm.
//!
//! Before this rung the `Protocol` arm called `cmd.run(&mut pctx)` BARE — only
//! the per-capability-accessor D15 `catch_unwind` shortcuts caught panics. A
//! `ProtocolCommand::run` that panicked in its OWN non-capability logic
//! unwound the actor thread. The `DispatchHostOp` arm we deleted in this rung
//! wrapped its handler in `catch_unwind`; merging the two seams into one MUST
//! preserve that, so the arm now wraps the entire `cmd.run` in `catch_unwind`.
//!
//! These tests pin that the arm-level catch is real: a `ProtocolCommand` whose
//! `run` panics directly (not through a capability accessor) is caught — proven
//! by `dispatch_one` (which drives a single `dispatch_command`) RETURNING
//! normally rather than unwinding the test thread.
//!
//! FAIL-BEFORE: with the bare `cmd.run` these tests would unwind through
//! `dispatch_command` and abort the test thread (a failed test, not a pass).

use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use crate::ActorCommand;

use super::commands::{self, IdentityRuntime};
use super::signer_port_test_harness::dispatch_one;

fn fresh_identity() -> IdentityRuntime {
    IdentityRuntime::new(
        commands::new_bunker_handshake_slot(),
        commands::new_signer_state_slot(),
    )
}

/// A `ProtocolCommand` whose `run` panics in its own body — NOT inside a
/// capability accessor (so the D15 per-accessor shortcuts cannot save it). Only
/// the whole-body `catch_unwind` on the `Protocol` arm catches this.
#[derive(Debug)]
struct PanickingProtocolCommand;

impl ProtocolCommand for PanickingProtocolCommand {
    fn run(
        self: Box<Self>,
        _ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        panic!("protocol command body intentionally exploded");
    }
}

/// ORACLE — a panicking `ProtocolCommand::run` is now caught by the arm's
/// whole-body `catch_unwind`. `dispatch_one` returning at all witnesses it: a
/// bare `cmd.run` would have unwound the dispatch frame and aborted this test.
#[test]
fn protocol_command_panic_is_caught_whole_body() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // If the panic were NOT caught, this call unwinds and the test fails.
    let _parked = dispatch_one(
        ActorCommand::Protocol(Box::new(PanickingProtocolCommand)),
        &mut identity,
        &mut kernel,
    );

    // Reaching here proves the actor's dispatch arm survived the panic.
    // A subsequent dispatch on the same kernel still works (the kernel's
    // RefCell borrow taken inside the catch_unwind closure was released on
    // unwind, so no double-borrow panic on the next dispatch).
    let _parked2 = dispatch_one(
        ActorCommand::MarkChangedSinceEmit,
        &mut identity,
        &mut kernel,
    );
}
