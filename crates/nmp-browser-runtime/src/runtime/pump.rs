//! Bounded inbox drain loop for `BrowserRuntime`.
//!
//! `drain_inbox` is the single entry point called by `BrowserRuntime::pump()`.
//! It drains up to [`BROWSER_COMMAND_DRAIN_BUDGET`] `ActorCommand`s from the
//! inbox channel without blocking, applies each to the kernel via
//! `KernelReducer::apply_actor_command`, and accumulates the resulting outbound
//! frames + host events.
//!
//! # D4 (single-writer)
//!
//! `drain_inbox` takes `&mut KernelReducer` — exactly one call site (the
//! owning `BrowserRuntime`) may hold this borrow. The wasm32 runtime is
//! inherently single-threaded; on native tests the `&mut` borrow enforces
//! exclusion.
//!
//! # D8 (no blocking, bounded work)
//!
//! `mpsc::Receiver::try_recv` is used — non-blocking. The loop stops at
//! [`BROWSER_COMMAND_DRAIN_BUDGET`] applied commands per pump (mirroring the
//! native actor's `COMMAND_DRAIN_BUDGET` fairness budget in
//! `nmp-core/src/actor/inbox.rs`) so a flood cannot monopolise one turn.
//! [`DrainOutcome::yielded`] is set when the budget was hit, telling the host to
//! pump again; any leftover mail stays queued in the channel (never dropped).

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, TryRecvError};

use nmp_core::actor::{ActorCommand, ActorMail, SignCommand};
use nmp_core::{CommandApplyOutcome, CommandSender, KernelReducer, OutboundMessage};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

use super::event::BrowserRuntimeEvent;
use super::PendingSignedPublish;
use crate::relay::WakeCell;
use crate::signer::{
    broker_sign_request, dispatch_nip44_cipher, CapabilityProviderRegistry, Nip44CipherMode,
    PendingCipherCompletions, PendingSignerCompletions, SignerCompletionTx,
};

/// Maximum number of commands applied per `pump()` turn.
///
/// Mirrors `nmp-core`'s native `COMMAND_DRAIN_BUDGET` (= 64). Bounding the
/// per-turn work keeps one pump from starving the host's event loop under a
/// command flood; leftover mail stays in the channel and is reported via
/// [`DrainOutcome::yielded`] so the host re-pumps.
pub(super) const BROWSER_COMMAND_DRAIN_BUDGET: usize = 64;

/// Result of one bounded drain pass.
pub(super) struct DrainOutcome {
    /// Outbound relay frames produced by applied commands this turn.
    pub(super) outbound: Vec<OutboundMessage>,
    /// Host events (sign requests, command failures) produced this turn.
    pub(super) events: Vec<BrowserRuntimeEvent>,
    /// True when the drain budget was exhausted this turn — the host should
    /// call `pump()` again to make progress on any remaining queued mail.
    pub(super) yielded: bool,
}

/// Mutable collaborators needed while applying one command-drain turn.
pub(super) struct DrainInboxContext<'a> {
    pub(super) pending: &'a mut HashMap<String, PendingSignedPublish>,
    pub(super) registry: &'a CapabilityProviderRegistry,
    pub(super) pending_signer_completions: &'a mut PendingSignerCompletions,
    pub(super) pending_cipher_completions: &'a mut PendingCipherCompletions,
    pub(super) completion_tx: &'a SignerCompletionTx,
    pub(super) wake: &'a WakeCell,
    pub(super) command_sender: &'a CommandSender,
}

/// Drain up to [`BROWSER_COMMAND_DRAIN_BUDGET`] commands and apply each.
///
/// Each command's [`CommandApplyOutcome`] is honored:
/// * `Applied` → its outbound frames are collected.
/// * `NeedsSign` → the publish continuation is parked in `pending` keyed on the
///   sign correlation id. `broker_sign_request` is called; if a provider is
///   found it dispatches the sign (LocalKey: inline; NIP-07/wasm: async via
///   channel). Only when no provider is found is
///   [`BrowserRuntimeEvent::SignRequest`] emitted for host-brokering (D6 —
///   never a silent drop).
/// * `Unsupported` → a [`BrowserRuntimeEvent::CommandFailed`] is emitted.
pub(super) fn drain_inbox(
    reducer: &mut KernelReducer,
    rx: &Receiver<ActorMail>,
    mut ctx: DrainInboxContext<'_>,
) -> DrainOutcome {
    let mut outbound: Vec<OutboundMessage> = Vec::new();
    let mut events: Vec<BrowserRuntimeEvent> = Vec::new();
    let mut applied = 0usize;

    while applied < BROWSER_COMMAND_DRAIN_BUDGET {
        let cmd = match rx.try_recv() {
            Ok(ActorMail::Command(cmd)) => cmd,
            // `ActorMail::Relay` exists whenever `nmp-core/native` is compiled
            // in — either this crate's own `native` feature (mirroring
            // `nmp-core/native`, see Cargo.toml) is enabled, or workspace
            // feature unification turns `nmp-core/native` on because some
            // other crate in the same build graph (e.g. `nmp-native-runtime`)
            // requests it, regardless of whether *this* crate's `native`
            // feature is on.
            //
            // Named exhaustively (not a bare wildcard) whenever we can prove
            // the variant exists from our own Cargo.toml condition, so a
            // future unconditional `ActorMail` variant fails to compile here
            // instead of silently matching this arm and no-op'ing (#2769 item
            // 10 — known hazard class: silently-dropped new variants). The
            // `not(feature = "native")` wildcard below is strictly narrower
            // than before this fix: it exists only to keep the build green
            // under the workspace-unification case just described, which this
            // crate cannot detect from its own `#[cfg]` surface.
            #[cfg(feature = "native")]
            Ok(ActorMail::Relay(_)) => continue,
            #[cfg(not(feature = "native"))]
            #[allow(unreachable_patterns)]
            Ok(_) => continue,
            // Channel drained or sender gone: no more work this turn and nothing
            // was left behind, so the host need not re-pump on our account.
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => {
                return DrainOutcome {
                    outbound,
                    events,
                    yielded: false,
                };
            }
        };

        applied += 1;
        let (commands, protocol_outbound) =
            match reducer.expand_protocol_commands(vec![cmd], ctx.command_sender.clone()) {
                Ok(expanded) => expanded,
                Err(reason) => {
                    events.push(BrowserRuntimeEvent::CommandFailed { reason });
                    continue;
                }
            };
        outbound.extend(protocol_outbound);
        for cmd in commands {
            let Some(cmd) = apply_browser_cipher_command(reducer, &mut ctx, cmd) else {
                continue;
            };
            match reducer.apply_actor_command(cmd) {
                CommandApplyOutcome::Applied(msgs) => {
                    outbound.extend(msgs);
                }
                CommandApplyOutcome::NeedsSign {
                    request,
                    target,
                    action_correlation_id,
                } => {
                    // Park the publish continuation under the sign correlation id.
                    ctx.pending.insert(
                        request.correlation_id.clone(),
                        PendingSignedPublish {
                            action_correlation_id,
                            target,
                        },
                    );
                    // Try to auto-broker using a registered provider (LocalKey:
                    // inline; NIP-07/wasm: async via spawn_local → channel).
                    // If no provider is found, emit SignRequest for host-brokering
                    // (never silently drop — D6).
                    let brokered = broker_sign_request(
                        ctx.registry,
                        ctx.pending_signer_completions,
                        &request.correlation_id,
                        &request.account_pubkey,
                        &request.unsigned_json,
                        ctx.completion_tx,
                        ctx.wake,
                    );
                    if !brokered {
                        events.push(BrowserRuntimeEvent::SignRequest {
                            correlation_id: request.correlation_id,
                            account_pubkey: request.account_pubkey,
                            unsigned_json: request.unsigned_json,
                        });
                    }
                }
                CommandApplyOutcome::Unsupported { reason } => {
                    events.push(BrowserRuntimeEvent::CommandFailed { reason });
                }
            }
        }
    }

    // Budget exhausted. Any further mail stays queued (never consumed/dropped);
    // signal the host to re-pump. A spurious yield (channel actually empty) costs
    // only one cheap `try_recv` Empty on the next pump.
    DrainOutcome {
        outbound,
        events,
        yielded: true,
    }
}

pub(super) fn coalesce_transient_subscriptions(
    outbound: Vec<OutboundMessage>,
) -> Vec<OutboundMessage> {
    let mut opened = HashMap::<(String, String), Vec<usize>>::new();
    let mut drop_indexes = HashSet::new();

    for (index, message) in outbound.iter().enumerate() {
        let Some(frame) = subscription_frame(message.text()) else {
            continue;
        };
        let key = (message.relay_url().to_string(), frame.sub_id);
        match frame.kind {
            SubscriptionFrameKind::Req => {
                opened.entry(key).or_default().push(index);
            }
            SubscriptionFrameKind::Close => {
                if let Some(open_indexes) = opened.remove(&key) {
                    drop_indexes.extend(open_indexes);
                    drop_indexes.insert(index);
                }
            }
        }
    }

    if drop_indexes.is_empty() {
        return outbound;
    }

    outbound
        .into_iter()
        .enumerate()
        .filter_map(|(index, message)| (!drop_indexes.contains(&index)).then_some(message))
        .collect()
}

struct SubscriptionFrame {
    kind: SubscriptionFrameKind,
    sub_id: String,
}

enum SubscriptionFrameKind {
    Req,
    Close,
}

fn subscription_frame(text: &str) -> Option<SubscriptionFrame> {
    let value = serde_json::from_str::<serde_json::Value>(text).ok()?;
    let arr = value.as_array()?;
    let kind = arr.first()?.as_str()?;
    let sub_id = arr.get(1)?.as_str()?.to_string();
    match kind {
        "REQ" => Some(SubscriptionFrame {
            kind: SubscriptionFrameKind::Req,
            sub_id,
        }),
        "CLOSE" => Some(SubscriptionFrame {
            kind: SubscriptionFrameKind::Close,
            sub_id,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use nmp_network::role::RelayRole;

    use super::*;

    fn frame(text: &str) -> OutboundMessage {
        OutboundMessage::new(
            RelayRole::Content,
            "wss://relay.example".to_string(),
            text.to_string(),
        )
    }

    #[test]
    fn coalesces_req_closed_in_same_pump_without_dropping_existing_close() {
        let outbound = vec![
            frame(r#"["CLOSE","old-sub"]"#),
            frame(r#"["REQ","transient-sub",{"kinds":[1]}]"#),
            frame(r#"["REQ","transient-sub",{"kinds":[1],"authors":["a"]}]"#),
            frame(r#"["REQ","kept-sub",{"kinds":[1]}]"#),
            frame(r#"["CLOSE","transient-sub"]"#),
        ];

        let texts = coalesce_transient_subscriptions(outbound)
            .into_iter()
            .map(|message| message.text().to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            texts,
            vec![
                r#"["CLOSE","old-sub"]"#.to_string(),
                r#"["REQ","kept-sub",{"kinds":[1]}]"#.to_string(),
            ]
        );
    }
}

fn apply_browser_cipher_command(
    reducer: &KernelReducer,
    ctx: &mut DrainInboxContext<'_>,
    command: ActorCommand,
) -> Option<ActorCommand> {
    match command {
        ActorCommand::Sign(SignCommand::EventForAccount {
            unsigned,
            signer_pubkey,
            continuation,
        }) => {
            let result = resolve_signer_pubkey(reducer, signer_pubkey, "browser sign")
                .and_then(|account| run_sign_event(ctx.registry, &account, unsigned));
            continuation.call(result);
            None
        }
        ActorCommand::Sign(SignCommand::Nip44EncryptForAccount {
            peer_pubkey,
            plaintext,
            signer_pubkey,
            continuation,
        }) => {
            match resolve_signer_pubkey(reducer, signer_pubkey, "browser nip44") {
                Ok(account) => dispatch_nip44_cipher(
                    ctx.registry,
                    ctx.pending_cipher_completions,
                    &account,
                    &peer_pubkey,
                    &plaintext,
                    Nip44CipherMode::Encrypt,
                    continuation,
                ),
                Err(reason) => continuation.call(Err(reason)),
            }
            None
        }
        ActorCommand::Sign(SignCommand::Nip44DecryptForAccount {
            peer_pubkey,
            ciphertext,
            signer_pubkey,
            continuation,
        }) => {
            match resolve_signer_pubkey(reducer, signer_pubkey, "browser nip44") {
                Ok(account) => dispatch_nip44_cipher(
                    ctx.registry,
                    ctx.pending_cipher_completions,
                    &account,
                    &peer_pubkey,
                    &ciphertext,
                    Nip44CipherMode::Decrypt,
                    continuation,
                ),
                Err(reason) => continuation.call(Err(reason)),
            }
            None
        }
        other => Some(other),
    }
}

fn resolve_signer_pubkey(
    reducer: &KernelReducer,
    signer_pubkey: Option<String>,
    operation: &str,
) -> Result<String, String> {
    if let Some(pubkey) = signer_pubkey {
        return Ok(pubkey);
    }
    reducer
        .active_account_handle()
        .lock()
        .map_err(|_| format!("{operation}: active account lock poisoned"))?
        .clone()
        .ok_or_else(|| format!("{operation}: no active account"))
}

fn run_sign_event(
    registry: &CapabilityProviderRegistry,
    account_pubkey: &str,
    unsigned: UnsignedEvent,
) -> Result<SignedEvent, String> {
    let entry = registry
        .resolve(account_pubkey)
        .ok_or_else(|| format!("browser sign: no signer for account {account_pubkey}"))?;
    let mut op = entry.signer.sign(unsigned);
    match op.poll() {
        Some(Ok(signed)) => Ok(signed),
        Some(Err(error)) => Err(error.to_string()),
        None => Err(
            "browser sign: pending provider cannot resolve through the synchronous continuation path"
                .to_string(),
        ),
    }
}
