//! Headless `ActorCommand::Protocol` expansion for [`KernelReducer`].
//!
//! Browser runtimes do not run the native actor dispatch arm, but protocol
//! commands still need the real kernel-backed capability context: active account,
//! kind:10050 DM relay lookup, action failure recording, wallet/zap access, and
//! the command sender used by continuation chains.

use std::cell::RefCell;

use crate::actor::{ActorCommand, CommandSender};
use crate::kernel::wallet_access::KernelWalletAccess;
use crate::kernel::Kernel;
use crate::relay::OutboundMessage;
use crate::substrate::{
    ActionStageTracker, ErrorSurface, KernelClock, LocalSignerAccess, NoopHostOpHandlerAccess,
    ProtocolCommand, ProtocolCommandContext, ProtocolCommandContextParts, RecipientRelayLookup,
};

impl super::KernelReducer {
    /// Expand protocol commands through a kernel-backed context and return the
    /// non-protocol commands that should be applied by the headless interpreter.
    ///
    /// `command_sender` is the runtime's self-sender. Protocol bodies that clone
    /// it for continuation hops enqueue follow-up commands into the same inbox
    /// that `pump()` drains.
    pub fn expand_protocol_commands(
        &mut self,
        commands: Vec<ActorCommand>,
        command_sender: CommandSender,
    ) -> Result<(Vec<ActorCommand>, Vec<OutboundMessage>), String> {
        let mut pending: Vec<ActorCommand> = commands.into_iter().rev().collect();
        let mut expanded = Vec::new();
        let mut outbound = Vec::new();

        while let Some(command) = pending.pop() {
            match command {
                ActorCommand::Protocol(protocol) => {
                    let (nested, mut frames) =
                        self.run_protocol_command(protocol, command_sender.clone())?;
                    outbound.append(&mut frames);
                    for command in nested.into_iter().rev() {
                        pending.push(command);
                    }
                }
                command => expanded.push(command),
            }
        }

        Ok((expanded, outbound))
    }

    fn run_protocol_command(
        &mut self,
        protocol: Box<dyn ProtocolCommand>,
        command_sender: CommandSender,
    ) -> Result<(Vec<ActorCommand>, Vec<OutboundMessage>), String> {
        let captured = RefCell::new(Vec::new());
        let send = |command| captured.borrow_mut().push(command);
        let dm_lookup = self.kernel.dm_inbox_relays_arc();
        let kernel_cell = RefCell::new(&mut self.kernel);

        let clock = HeadlessKernelClock {
            kernel: &kernel_cell,
        };
        let signers = HeadlessLocalSignerAccess {
            kernel: &kernel_cell,
        };
        let errors = HeadlessErrorSurface {
            kernel: &kernel_cell,
        };
        let stages = HeadlessActionStageTracker {
            kernel: &kernel_cell,
        };
        let recipients = HeadlessRecipientRelayLookup {
            kernel: &kernel_cell,
        };
        let wallet = KernelWalletAccess::borrowed(&kernel_cell);
        let mut outbound = Vec::new();

        let run_err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut ctx = ProtocolCommandContext::new(ProtocolCommandContextParts {
                send: &send,
                command_sender,
                clock: &clock,
                signers: &signers,
                dms: &*dm_lookup,
                errors: &errors,
                stages: &stages,
                recipients: &recipients,
                host_op_handler: &NoopHostOpHandlerAccess,
                wallet_kernel: &wallet,
                zap_profiles: &wallet,
            })
            .with_outbound(&mut outbound);
            protocol.run(&mut ctx)
        }))
        .unwrap_or_else(|_| {
            Err(crate::substrate::ProtocolCommandError::new(
                "ProtocolCommand panicked",
            ))
        });

        drop(wallet);
        drop(recipients);
        drop(stages);
        drop(errors);
        drop(signers);
        drop(clock);

        self.kernel.mark_changed_since_emit();

        run_err
            .map(|_| (captured.into_inner(), outbound))
            .map_err(|err| err.to_string())
    }
}

struct HeadlessKernelClock<'a> {
    kernel: &'a RefCell<&'a mut Kernel>,
}

impl KernelClock for HeadlessKernelClock<'_> {
    fn now_secs(&self) -> u64 {
        self.kernel.borrow().now_secs()
    }
}

struct HeadlessLocalSignerAccess<'a> {
    kernel: &'a RefCell<&'a mut Kernel>,
}

impl LocalSignerAccess for HeadlessLocalSignerAccess<'_> {
    fn active_local_keys(&self) -> Option<nostr::Keys> {
        None
    }

    fn active_account_pubkey(&self) -> Option<String> {
        self.kernel
            .borrow()
            .active_account_pubkey()
            .map(ToString::to_string)
    }
}

struct HeadlessErrorSurface<'a> {
    kernel: &'a RefCell<&'a mut Kernel>,
}

impl ErrorSurface for HeadlessErrorSurface<'_> {
    fn set_last_error_toast(&self, message: Option<String>) {
        if let Ok(mut kernel) = self.kernel.try_borrow_mut() {
            kernel.set_last_error_toast(message);
        }
    }

    fn set_last_error_token(&self, token: &crate::ui_token::UiToken) {
        if let Ok(mut kernel) = self.kernel.try_borrow_mut() {
            kernel.set_last_error_token(token);
        }
    }

    fn record_action_failure(&self, correlation_id: String, reason: String) {
        if let Ok(mut kernel) = self.kernel.try_borrow_mut() {
            kernel.record_action_failure(correlation_id, reason);
        }
    }
}

struct HeadlessActionStageTracker<'a> {
    kernel: &'a RefCell<&'a mut Kernel>,
}

impl ActionStageTracker for HeadlessActionStageTracker<'_> {
    fn record_requested(&self, correlation_id: &str) {
        if let Ok(mut kernel) = self.kernel.try_borrow_mut() {
            kernel.record_action_stage(
                correlation_id,
                crate::kernel::action_stages::ActionStage::Requested,
                None,
            );
        }
    }
}

struct HeadlessRecipientRelayLookup<'a> {
    kernel: &'a RefCell<&'a mut Kernel>,
}

impl RecipientRelayLookup for HeadlessRecipientRelayLookup<'_> {
    fn recipient_publish_relays(&self, recipient: &str, kind: u32) -> Vec<String> {
        self.kernel
            .try_borrow()
            .ok()
            .map(|kernel| kernel.recipient_publish_relays(recipient, kind))
            .unwrap_or_default()
    }
}
