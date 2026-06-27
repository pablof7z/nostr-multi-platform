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
//! `Interests(OpenInterest)`, `Interests(OpenObservedInterest)`,
//! `Interests(CloseInterest)`,
//! `Relay(SetRelayInfo)`, `Lifecycle(MarkChangedSinceEmit)`,
//! `Publish(SignedEvent)`.
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

            // OpenObservedInterest: browser/default read-model front door. The
            // observer has already been registered muted by the caller; this
            // opens the live interest, replays cache rows, activates the
            // observer, and drains lifecycle outbound inline like open_interest.
            ActorCommand::Interests(InterestsCommand::OpenObservedInterest {
                filter_json,
                consumer_id,
                scope,
                relay_pin,
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
            }) => {
                if let Some((identity, _interest)) =
                    crate::subs::interest_builder::build_interest_pair(
                        &filter_json,
                        &consumer_id,
                        scope,
                        relay_pin.as_deref(),
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

            // Reply: derive NIP-10 tags from the stored parent, then build the
            // same unsigned kind:1 JSON path as RawEvent. The host never
            // constructs protocol tags.
            ActorCommand::Publish(PublishCommand::Reply {
                content,
                reply_to_event_id,
                target,
                signer_pubkey: _,
                correlation_id: cid,
            }) => {
                let Some(tags) = self.build_reply_tags(&reply_to_event_id) else {
                    return Unsupported {
                        reason: format!("reply_target_unknown: {reply_to_event_id}"),
                    };
                };
                let Some(account_pubkey) = self.active_account_pubkey() else {
                    return Unsupported {
                        reason: "no active account for Reply sign round-trip".to_string(),
                    };
                };
                let created_at = self.now_secs();
                let unsigned_json = serde_json::json!({
                    "pubkey": account_pubkey,
                    "kind": 1u32,
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
                let content = serde_json::to_string(&fields).unwrap_or_else(|_| "{}".to_string());
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

    fn apply_contact_edit(
        &mut self,
        pubkeys: Vec<String>,
        add: bool,
        correlation_id: Option<String>,
    ) -> CommandApplyOutcome {
        use CommandApplyOutcome::{NeedsSign, Unsupported};

        let Some(account_pubkey) = self.active_account_pubkey() else {
            return Unsupported {
                reason: "no active account for contact-list edit".to_string(),
            };
        };
        let Some((current_tags, content, baseline_created_at)) =
            self.kernel.try_current_kind3_event_for_edit()
        else {
            return Unsupported {
                reason: "follow_list_not_loaded".to_string(),
            };
        };

        let mut tags = current_tags;
        let is_single_target = pubkeys.len() == 1;
        for pubkey in pubkeys {
            if !crate::kernel::is_hex_pubkey(&pubkey) {
                if is_single_target {
                    let verb = if add { "follow" } else { "unfollow" };
                    return Unsupported {
                        reason: format!("{verb}: expected 64-hex pubkey"),
                    };
                }
                continue;
            }
            if pubkey == account_pubkey {
                continue;
            }
            tags = if add {
                crate::tags::kind3_tags_after_add(&tags, &pubkey)
            } else {
                crate::tags::kind3_tags_after_remove(&tags, &pubkey)
            };
        }

        let unsigned = UnsignedEvent {
            pubkey: account_pubkey.clone(),
            kind: 3,
            tags,
            content,
            created_at: self.now_secs().max(baseline_created_at.saturating_add(1)),
        };
        let unsigned_json = serde_json::json!({
            "pubkey": account_pubkey,
            "kind": unsigned.kind,
            "tags": unsigned.tags,
            "content": unsigned.content,
            "created_at": unsigned.created_at,
        })
        .to_string();
        match self.begin_sign_roundtrip_at(
            unsigned.pubkey,
            &unsigned_json,
            crate::time::Instant::now(),
        ) {
            Ok(request) => NeedsSign {
                request,
                target: PublishTarget::Auto,
                action_correlation_id: correlation_id,
            },
            Err(reason) => Unsupported { reason },
        }
    }
}
