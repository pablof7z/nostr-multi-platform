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
//! The interpreter consolidates the partial dispatch that previously lived
//! inline in `nmp-wasm/src/runtime/dispatch.rs`, so there is now ONE command-
//! application path shared by the browser runtime and any future headless
//! runtime.
//!
//! **Handled set (Group A → `Applied`):**
//! `Interests(EnsureInterest)`, `Interests(DropInterestOwner)`,
//! `Interests(OpenInterest)`, `Interests(CloseInterest)`,
//! `Relay(SetRelayInfo)`, `Lifecycle(MarkChangedSinceEmit)`,
//! `Contacts(ClearActiveFollowsFeed)`, `Publish(SignedEvent)`.
//!
//! **Group B → `NeedsSign`:**
//! `Publish(UnsignedEvent)`, `Publish(RawEvent)`, `Publish(Profile)`.
//!
//! **Group C → `Unsupported`:** every other variant.

use crate::actor::ActorCommand;
use crate::publish::PublishTarget;
use crate::relay::OutboundMessage;
use super::wasm_signing::SignRoundTripRequest;

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
            ContactsCommand, InterestsCommand, LifecycleCommand, PublishCommand, RelayCommand,
        };
        use CommandApplyOutcome::{Applied, NeedsSign, Unsupported};

        match command {
            // ── Group A: Applied (synchronous) ───────────────────────────────

            // EnsureInterest: register-if-absent, shared Kernel method.
            ActorCommand::Interests(InterestsCommand::EnsureInterest { identity, interest }) => {
                self.kernel.ensure_interest(identity, interest);
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

            // CloseInterest: detach one owner; note relay_pin is not forwarded
            // (KernelReducer::close_interest uses build_interest_pair with
            // None; pinned close is a native-actor-only path).
            ActorCommand::Interests(InterestsCommand::CloseInterest {
                filter_json,
                consumer_id,
                scope,
                relay_pin: _,
            }) => {
                let outbound = self.close_interest(&filter_json, &consumer_id, scope);
                Applied(outbound)
            }

            // SetRelayInfo: fold a fetched NIP-11 document onto the kernel row.
            ActorCommand::Relay(RelayCommand::SetRelayInfo { relay_url, doc_json }) => {
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

            // ClearActiveFollowsFeed: withdraw all follow-feed M2 interests.
            ActorCommand::Contacts(ContactsCommand::ClearActiveFollowsFeed) => {
                let outbound = self.clear_active_follows_feed();
                Applied(outbound)
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

            // ── Group B: NeedsSign (async sign round-trip required) ──────────

            // UnsignedEvent: build unsigned JSON, begin sign round-trip.
            ActorCommand::Publish(PublishCommand::UnsignedEvent {
                event,
                correlation_id: cid,
                signer_pubkey: _,
            }) => {
                let Some(account_pubkey) = self.active_account_pubkey() else {
                    return Unsupported {
                        reason: "no active account for UnsignedEvent sign round-trip".to_string(),
                    };
                };
                let unsigned_json = serde_json::json!({
                    "pubkey": account_pubkey,
                    "kind": event.kind,
                    "tags": event.tags,
                    "content": event.content,
                    "created_at": event.created_at,
                })
                .to_string();
                match self.begin_sign_roundtrip_at(
                    account_pubkey,
                    &unsigned_json,
                    crate::time::Instant::now(),
                ) {
                    Ok(request) => NeedsSign {
                        request,
                        target: PublishTarget::Auto,
                        action_correlation_id: cid,
                    },
                    Err(reason) => Unsupported { reason },
                }
            }

            // RawEvent: build unsigned JSON from the raw fields, begin sign.
            ActorCommand::Publish(PublishCommand::RawEvent {
                kind,
                tags,
                content,
                target,
                signer_pubkey: _,
                correlation_id: cid,
            }) => {
                let Some(account_pubkey) = self.active_account_pubkey() else {
                    return Unsupported {
                        reason: "no active account for RawEvent sign round-trip".to_string(),
                    };
                };
                let created_at = self.now_secs();
                let unsigned_json = serde_json::json!({
                    "pubkey": account_pubkey,
                    "kind": kind,
                    "tags": tags,
                    "content": content,
                    "created_at": created_at,
                })
                .to_string();
                match self.begin_sign_roundtrip_at(
                    account_pubkey,
                    &unsigned_json,
                    crate::time::Instant::now(),
                ) {
                    Ok(request) => NeedsSign {
                        request,
                        target,
                        action_correlation_id: cid,
                    },
                    Err(reason) => Unsupported { reason },
                }
            }

            // Profile (kind:0): build kind:0 content JSON, begin sign round-trip.
            ActorCommand::Publish(PublishCommand::Profile {
                fields,
                correlation_id: cid,
            }) => {
                let Some(account_pubkey) = self.active_account_pubkey() else {
                    return Unsupported {
                        reason: "no active account for Profile sign round-trip".to_string(),
                    };
                };
                let content =
                    serde_json::to_string(&fields).unwrap_or_else(|_| "{}".to_string());
                let created_at = self.now_secs();
                let unsigned_json = serde_json::json!({
                    "pubkey": account_pubkey,
                    "kind": 0u32,
                    "tags": serde_json::Value::Array(vec![]),
                    "content": content,
                    "created_at": created_at,
                })
                .to_string();
                match self.begin_sign_roundtrip_at(
                    account_pubkey,
                    &unsigned_json,
                    crate::time::Instant::now(),
                ) {
                    Ok(request) => NeedsSign {
                        request,
                        target: PublishTarget::Auto,
                        action_correlation_id: cid,
                    },
                    Err(reason) => Unsupported { reason },
                }
            }

            // ── Group C: Unsupported (native actor thread required) ──────────
            //
            // Every other ActorCommand variant requires the native actor thread
            // (identity/roster management, NIP-46 sign brokering, contacts
            // follow/unfollow, relay add/remove/reconnect, protocol dispatch,
            // etc.). The discriminant name is surfaced as the reason string so
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
