//! Protocol-command expansion for the browser/headless runtime.
//!
//! The native actor has a dispatch arm for `ActorCommand::Protocol`: it runs the
//! protocol command, then feeds any emitted `ActorCommand`s back through the
//! actor loop. The browser runtime owns a single-threaded `KernelReducer`
//! instead of the native actor, so it must perform the same expansion before
//! applying commands through the headless interpreter.
//!
//! Capability ports are intentionally no-op here. Protocol commands that need
//! browser-host capabilities must fail honestly through their emitted command or
//! result; commands that only lower to publish/read commands, such as NIP-25
//! reactions, stay fully Rust-owned without requiring a native actor thread.

use std::cell::RefCell;
use std::sync::mpsc;

use nmp_core::actor::{ActorCommand, CommandSender};
use nmp_core::substrate::{
    EmptyDmInboxRelayLookup, NoopActionStageTracker, NoopErrorSurface, NoopHostOpHandlerAccess,
    NoopKernelClock, NoopLocalSignerAccess, NoopRecipientRelayLookup, NoopWalletKernelAccess,
    NoopZapProfileLookup, ProtocolCommand, ProtocolCommandContext, ProtocolCommandContextParts,
};

pub(super) fn expand_protocol_commands(
    commands: Vec<ActorCommand>,
) -> Result<Vec<ActorCommand>, String> {
    let mut pending: Vec<ActorCommand> = commands.into_iter().rev().collect();
    let mut expanded = Vec::new();

    while let Some(command) = pending.pop() {
        match command {
            ActorCommand::Protocol(protocol) => {
                let nested = run_protocol(protocol)?;
                for command in nested.into_iter().rev() {
                    pending.push(command);
                }
            }
            command => expanded.push(command),
        }
    }

    Ok(expanded)
}

fn run_protocol(protocol: Box<dyn ProtocolCommand>) -> Result<Vec<ActorCommand>, String> {
    static CLOCK: NoopKernelClock = NoopKernelClock;
    static SIGNERS: NoopLocalSignerAccess = NoopLocalSignerAccess;
    static DMS: EmptyDmInboxRelayLookup = EmptyDmInboxRelayLookup;
    static ERRORS: NoopErrorSurface = NoopErrorSurface;
    static STAGES: NoopActionStageTracker = NoopActionStageTracker;
    static RECIPIENTS: NoopRecipientRelayLookup = NoopRecipientRelayLookup;
    static HOST_OP: NoopHostOpHandlerAccess = NoopHostOpHandlerAccess;
    static WALLET: NoopWalletKernelAccess = NoopWalletKernelAccess;
    static ZAP: NoopZapProfileLookup = NoopZapProfileLookup;

    let captured = RefCell::new(Vec::new());
    let send = |command| captured.borrow_mut().push(command);
    let (tx, _rx) = mpsc::channel();
    let command_sender = CommandSender::new(tx);
    let mut ctx = ProtocolCommandContext::new(ProtocolCommandContextParts {
        send: &send,
        command_sender,
        clock: &CLOCK,
        signers: &SIGNERS,
        dms: &DMS,
        errors: &ERRORS,
        stages: &STAGES,
        recipients: &RECIPIENTS,
        host_op_handler: &HOST_OP,
        wallet_kernel: &WALLET,
        zap_profiles: &ZAP,
    });
    protocol.run(&mut ctx).map_err(|err| err.to_string())?;
    Ok(captured.into_inner())
}
