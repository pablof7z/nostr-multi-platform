//! Narrow browser/headless command interpreter for [`super::KernelReducer`].
//!
//! Implements [`KernelReducer::apply_actor_command`] — a thin interpreter for
//! the [`ActorCommand`] variants that wasm-safe default runtimes actually emit.
//! It has three outcomes:
//!
//! - [`CommandApplyOutcome::Applied`] — the command was applied synchronously.
//! - [`CommandApplyOutcome::NeedsSign`] — the command requires an async NIP-07
//!   sign round-trip before the publish can complete.
//! - [`CommandApplyOutcome::Unsupported`] — the command requires the native
//!   actor thread (roster management, contacts follow, protocol dispatch, etc.).
//!
//! The interpreter consolidates the browser/headless partial dispatch, so there
//! is now ONE command-application path shared by the browser runtime and any
//! future headless runtime.
//!
//! **Handled set (Group A → `Applied`):**
//! `Interests(EnsureInterest)`, `Interests(DropInterestOwner)`,
//! `Interests(ApplyDependentInterestDelta)`,
//! `Interests(OpenInterest)`, `Interests(OpenObservedInterest)`,
//! `Interests(CloseInterest)`,
//! `Relay(SetRelayInfo)`, `Lifecycle(MarkChangedSinceEmit)`,
//! `Publish(SignedEvent)`, `ActionLedger(RecordFailure|RecordSuccess)`,
//! `ShowToast`, `ShowErrorToken`, `EnqueueOutbound`.
//!
//! **Group B → `NeedsSign`:**
//! `Publish(UnsignedEvent)`, `Publish(RawEvent)`, `Publish(Reply)`,
//! `Publish(Profile)`, `Contacts(Follow)`, `Contacts(Unfollow)`,
//! `Contacts(FollowMany)`.
//!
//! **Group C → `Unsupported`:** every other variant.

use super::wasm_signing::SignRoundTripRequest;
use crate::actor::ActorCommand;
use crate::publish::PublishTarget;
use crate::relay::OutboundMessage;
use nmp_signer_iface::UnsignedEvent;

/// Outcome of applying one [`ActorCommand`] through the narrow headless
/// interpreter.
#[derive(Debug)]
pub enum CommandApplyOutcome {
    /// The command was applied synchronously. Fan the outbound frames to the
    /// relay pool, then push an `ActionAccepted` + snapshot to the host.
    Applied(Vec<OutboundMessage>),
    /// The command requires an async sign round-trip (NIP-07 / NIP-55). Park
    /// the pending publish keyed on `request.correlation_id`, then push a
    /// `WorkerEvent::SignRequest` to the main-thread broker.
    NeedsSign {
        request: SignRoundTripRequest,
        target: PublishTarget,
        /// Per-command correlation id extracted from the command variant, or
        /// `None` if the command did not carry one. Callers fall back to the
        /// envelope-level correlation id when `None`.
        action_correlation_id: Option<String>,
    },
    /// The command is not handled by the headless runtime — it requires the
    /// native actor thread. Callers should surface a `CapabilityFailure` with
    /// this `reason` string, which names the variant discriminant (D6-honest).
    Unsupported { reason: String },
}

impl super::KernelReducer {
    /// Apply one [`ActorCommand`] through the narrow headless interpreter.
    ///
    /// See the [module docs](self) for the full handled/unhandled set.
    pub fn apply_actor_command(&mut self, command: ActorCommand) -> CommandApplyOutcome {
        use crate::actor::{
            ActionLedgerCommand, ContactsCommand, InterestsCommand, LifecycleCommand,
            PublishCommand, RelayCommand,
        };
        use CommandApplyOutcome::{Applied, Unsupported};

        match command {
            // ── Group A: Applied (synchronous) ───────────────────────────────

            // EnsureInterest: register-if-absent, shared Kernel method.
            ActorCommand::Interests(InterestsCommand::EnsureInterest { identity, interest }) => {
                self.kernel.ensure_interest(identity, interest);
                Applied(Vec::new())
            }

            ActorCommand::Interests(InterestsCommand::ApplyDependentInterestDelta {
                owner,
                delta,
                reason,
            }) => {
                self.kernel
                    .apply_dependent_interest_delta(owner, delta, &reason);
                Applied(Vec::new())
            }

            // DropInterestOwner: drop one owner + optional recompile trigger.
            ActorCommand::Interests(InterestsCommand::DropInterestOwner(identity)) => {
                self.kernel.drop_interest_owner(identity);
                Applied(Vec::new())
            }

            // OpenInterest: generic M2 feed subscription front-door.
            ActorCommand::Interests(InterestsCommand::OpenInterest {
                filter_json,
                consumer_id,
                scope,
            }) => {
                let outbound = self.open_interest(&filter_json, &consumer_id, scope);
                Applied(outbound)
            }

            // OpenObservedInterest: browser/default read-model front door. The
            // observer has already been registered muted by the caller; this
            // opens the live interest, replays cache rows, activates the
            // observer, and drains lifecycle outbound inline like open_interest.
            ActorCommand::Interests(InterestsCommand::OpenObservedInterest {
                filter_json,
                consumer_id,
                scope,
                relay_pin,
                is_indexer_discovery,
                observer_id,
                replay_shapes,
                replay_limit,
            }) => {
                if let Some((identity, interest)) =
                    crate::subs::interest_builder::build_interest_pair(
                        &filter_json,
                        &consumer_id,
                        scope,
                        relay_pin.as_deref(),
                        is_indexer_discovery,
                    )
                {
                    let replay = crate::kernel::ObserverReplayRequest {
                        observer_id,
                        shapes: replay_shapes,
                        limit: replay_limit,
                    };
                    let _ = self.kernel.open_interest_with_observer_replay(
                        identity,
                        interest,
                        replay,
                        "open-observed-interest",
                    );
                }
                let outbound = self.kernel.drain_lifecycle_outbound();
                let outbound = self.kernel.partition_auth_paused(outbound);
                Applied(outbound)
            }

            // CloseInterest: detach one owner. The relay pin participates in
            // the subscription identity, so preserve it when reconstructing
            // the close.
            ActorCommand::Interests(InterestsCommand::CloseInterest {
                filter_json,
                consumer_id,
                scope,
                relay_pin,
                is_indexer_discovery,
            }) => {
                if let Some((identity, _interest)) =
                    crate::subs::interest_builder::build_interest_pair(
                        &filter_json,
                        &consumer_id,
                        scope,
                        relay_pin.as_deref(),
                        is_indexer_discovery,
                    )
                {
                    let _ = self.kernel.close_interest_sub(&identity);
                }
                let outbound = self.kernel.drain_lifecycle_outbound();
                let outbound = self.kernel.partition_auth_paused(outbound);
                Applied(outbound)
            }

            // SetRelayInfo: fold a fetched NIP-11 document onto the kernel row.
            ActorCommand::Relay(RelayCommand::SetRelayInfo {
                relay_url,
                doc_json,
            }) => {
                if let Some(doc) = crate::substrate::RelayInfoDoc::from_json(&doc_json) {
                    self.kernel
                        .set_relay_info_at(&relay_url, doc, crate::time::Instant::now());
                }
                Applied(Vec::new())
            }

            // MarkChangedSinceEmit: force the next snapshot to emit.
            ActorCommand::Lifecycle(LifecycleCommand::MarkChangedSinceEmit) => {
                self.kernel.mark_changed_since_emit();
                Applied(Vec::new())
            }

            // SignedEvent: route through the shared publish helper (now verifies
            // the signature — closes the forged-event gap on the wasm path).
            ActorCommand::Publish(PublishCommand::SignedEvent {
                raw,
                target,
                correlation_id: cid,
            }) => {
                let outbound = self.kernel.publish_externally_signed(raw, target, cid);
                let outbound = self.kernel.partition_auth_paused(outbound);
                Applied(outbound)
            }

            // ActionLedger terminals are part of the protocol-continuation
            // surface. NIP-17 gift-wrap failures enqueue these from continuation
            // closures after the initial `ProtocolCommand` has returned.
            ActorCommand::ActionLedger(ActionLedgerCommand::RecordFailure {
                correlation_id,
                reason,
            }) => {
                self.kernel.record_action_failure(correlation_id, reason);
                Applied(Vec::new())
            }
            ActorCommand::ActionLedger(ActionLedgerCommand::RecordSuccess {
                correlation_id,
                result_json,
            }) => {
                self.kernel
                    .record_action_success(correlation_id, result_json);
                Applied(Vec::new())
            }
            ActorCommand::ActionLedger(ActionLedgerCommand::Ack(correlation_id)) => {
                self.kernel.ack_action_stage(&correlation_id);
                Applied(Vec::new())
            }

            ActorCommand::ShowToast { message } => {
                self.kernel.set_last_error_toast(Some(message));
                Applied(Vec::new())
            }
            ActorCommand::ShowErrorToken { token } => {
                self.kernel.set_last_error_token(&token);
                Applied(Vec::new())
            }
            ActorCommand::EnqueueOutbound {
                relay_url,
                text,
                role,
            } => Applied(vec![OutboundMessage {
                relay_url,
                text,
                role,
            }]),

            // ── Group B: NeedsSign (async sign round-trip required) ──────────

            // UnsignedEvent: build unsigned JSON, begin sign round-trip.
            ActorCommand::Publish(PublishCommand::UnsignedEvent {
                event,
                correlation_id: cid,
                signer_pubkey: _,
            }) => self.begin_unsigned_publish_roundtrip(
                event,
                None,
                PublishTarget::Auto,
                cid,
                false,
                "UnsignedEvent",
            ),

            ActorCommand::Publish(PublishCommand::OwnedUnsignedEvent {
                event,
                ownership,
                correlation_id: cid,
                signer_pubkey: _,
            }) => self.begin_unsigned_publish_roundtrip(
                event,
                Some(ownership),
                PublishTarget::Auto,
                cid,
                false,
                "UnsignedEvent",
            ),

            // RawEvent: build unsigned JSON from the raw fields, begin sign.
            ActorCommand::Publish(PublishCommand::RawEvent {
                kind,
                tags,
                content,
                target,
                signer_pubkey: _,
                correlation_id: cid,
            }) => {
                let is_group_host_pin = matches!(
                    &target,
                    PublishTarget::Explicit {
                        route_class: crate::publish::PublishRouteClass::GroupHostPin,
                        ..
                    }
                );
                self.begin_unsigned_publish_roundtrip(
                    UnsignedEvent {
                        pubkey: String::new(),
                        kind,
                        tags,
                        content,
                        created_at: 0,
                    },
                    None,
                    target,
                    cid,
                    is_group_host_pin,
                    "RawEvent",
                )
            }

            // Reply: resolve the user intent through the registered protocol
            // draft builder, then publish the returned unsigned event.
            ActorCommand::Publish(PublishCommand::Reply {
                content,
                reply_to_event_id,
                target,
                signer_pubkey: _,
                correlation_id: cid,
            }) => {
                let Some(account_pubkey) = self.active_account_pubkey() else {
                    return Unsupported {
                        reason: "no active account for Reply sign round-trip".to_string(),
                    };
                };
                let created_at = self.now_secs();
                let intent = crate::substrate::DraftIntent::Reply {
                    content,
                    reply_to_event_id,
                };
                let unsigned = match self
                    .kernel
                    .build_draft(&intent, &account_pubkey, created_at)
                {
                    Ok(unsigned) => unsigned,
                    Err(err) => {
                        return Unsupported {
                            reason: err.to_string(),
                        }
                    }
                };
                self.begin_unsigned_publish_roundtrip(unsigned, None, target, cid, false, "Reply")
            }

            // Profile: resolve through the registered protocol draft builder.
            ActorCommand::Publish(PublishCommand::Profile {
                fields,
                correlation_id: cid,
            }) => {
                let Some(account_pubkey) = self.active_account_pubkey() else {
                    return Unsupported {
                        reason: "no active account for Profile sign round-trip".to_string(),
                    };
                };
                let created_at = self.now_secs();
                let intent = crate::substrate::DraftIntent::Profile { fields };
                let unsigned = match self
                    .kernel
                    .build_draft(&intent, &account_pubkey, created_at)
                {
                    Ok(unsigned) => unsigned,
                    Err(err) => {
                        return Unsupported {
                            reason: err.to_string(),
                        }
                    }
                };
                self.begin_unsigned_publish_roundtrip(
                    unsigned,
                    None,
                    PublishTarget::Auto,
                    cid,
                    false,
                    "Profile",
                )
            }

            // Contacts: splice the active account's loaded kind:3 and publish
            // the full replaceable event through the same wasm sign round-trip
            // path as other unsigned browser writes.
            ActorCommand::Contacts(ContactsCommand::Follow {
                pubkey,
                correlation_id: cid,
            }) => self.apply_contact_edit(vec![pubkey], true, cid),
            ActorCommand::Contacts(ContactsCommand::Unfollow {
                pubkey,
                correlation_id: cid,
            }) => self.apply_contact_edit(vec![pubkey], false, cid),
            ActorCommand::Contacts(ContactsCommand::FollowMany {
                pubkeys,
                correlation_id: cid,
            }) => self.apply_contact_edit(pubkeys, true, cid),

            // ── Group C: Unsupported (native actor thread required) ──────────
            //
            // Every other ActorCommand variant requires the native actor thread
            // (identity/roster management, NIP-46 sign brokering, relay
            // add/remove/reconnect, protocol dispatch, etc.). The discriminant name
            // is surfaced as the reason string so
            // the host receives an honest D6 "not handled" signal.
            other => Unsupported {
                reason: format!(
                    "browser_command_unsupported: ActorCommand::{:?} requires the native \
                     actor thread and is not handled by the headless runtime",
                    std::mem::discriminant(&other)
                ),
            },
        }
    }
}
