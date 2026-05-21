//! `PublishAction` + `PublishModule` (the `ActionModule` impl).
//!
//! `start` is wired to the actor mailbox (M6): `ffi::action::execute_action`
//! validates a `PublishAction` through `ActionRegistry`, then converts a
//! `Publish` variant into `ActorCommand::PublishSignedEvent` for the actor
//! to publish. The publish engine drives per-relay transitions in-process;
//! its terminal verdict is surfaced as a [`PublishOutcome`] on the snapshot.

use serde::{Deserialize, Serialize};

use crate::substrate::{ActionContext, ActionModule, ActionRejection, SignedEvent};

/// Stable handle returned to the caller of `Publish`. Used to key snapshot
/// entries and to address the action in the ledger when M6 wires the ledger.
pub type PublishHandle = String;

/// Relay URL — grep-able alias so the `RelayDispatcher` shim can be swapped
/// for `nmp-nip01::RelayManager` from M8 without changing call sites. Single
/// crate-wide definition lives in `crate::relay`; re-exported here so
/// `publish` import paths are unchanged.
pub use crate::relay::RelayUrl;

/// Where a publish should go.
///
/// `Auto` defers to the `OutboxResolver` (NIP-65 + indexer fallback per D3).
/// `Explicit` is the named opt-out (D3: "manual relay selection is the
/// opt-out").
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublishTarget {
    Auto,
    Explicit { relays: Vec<RelayUrl> },
}

/// The single public publish action.
///
/// The signed event is included pre-signed because the kernel ledger (M6) will
/// sign once via the active `IdentityModule` and then enqueue the publish — we
/// never re-sign on retry (per the M6 exit gate "re-publish of an event
/// preserves `id` and `sig`").
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum PublishAction {
    Publish {
        handle: PublishHandle,
        event: SignedEvent,
        target: PublishTarget,
    },
    /// Sign-and-publish a kind:1 note (optionally a reply) with the active
    /// account. Unlike `Publish`, the event is *not* pre-signed — the actor
    /// signs it via the active `IdentityModule`. This is the
    /// `ActionModule`-native replacement for the deleted per-verb
    /// `nmp_app_publish_note` FFI symbol; the executor routes it to the
    /// existing `ActorCommand::PublishNote` handler.
    PublishNote {
        content: String,
        reply_to_id: Option<String>,
        target: PublishTarget,
    },
    /// Publish a kind:0 profile metadata event for the active account.
    /// `fields` is a flat JSON object with string-valued keys such as
    /// `"name"`, `"about"`, `"picture"` — the actor serializes it into the
    /// kind:0 `content` field, signs with the active `IdentityModule`, and
    /// routes through the NIP-65 outbox. Like `PublishNote`, the event is
    /// *not* pre-signed: the actor stamps `created_at` and signs. This is the
    /// `ActionModule`-native replacement for hosts hand-rolling a kind:0
    /// event dict and calling `nmp_app_publish_unsigned_event` directly.
    PublishProfile {
        fields: serde_json::Map<String, serde_json::Value>,
    },
    Cancel {
        handle: PublishHandle,
    },
}

/// Final outcome reported to the action ledger when the engine finishes.
///
/// `Mixed` covers the common case where some relays accepted and some
/// gave up — the snapshot carries the per-relay detail; the ledger gets a
/// single coarse verdict.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublishOutcome {
    Accepted {
        relays: Vec<RelayUrl>,
    },
    Mixed {
        accepted: Vec<RelayUrl>,
        failed: Vec<RelayUrl>,
    },
    FailedAfterRetries {
        failed: Vec<RelayUrl>,
    },
    NoTargets,
    Cancelled,
}

/// `ActionModule` impl. The runtime is the engine; this trait exists so the
/// ledger sees a uniform shape across actions.
pub struct PublishModule;

impl ActionModule for PublishModule {
    const NAMESPACE: &'static str = "nmp.publish";

    type Action = PublishAction;

    /// For pre-signed `Publish` actions, use the event's `id` as the
    /// correlation_id. The publish engine's `LastTerminal.correlation_id` is
    /// already the `PublishHandle` (== `event.id`), so using the same value
    /// here means `dispatch_action`'s return and `last_action_result` in the
    /// snapshot share the same identifier.
    ///
    /// `PublishNote` and `Cancel` return `None` — the event id isn't known
    /// until the actor signs (`PublishNote`), and `Cancel` acts on an
    /// existing handle (`Cancel`).
    fn preferred_action_id(action: &Self::Action) -> Option<crate::substrate::ActionId> {
        match action {
            PublishAction::Publish { event, .. } if !event.id.is_empty() => {
                Some(event.id.clone())
            }
            _ => None,
        }
    }

    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        match action {
            PublishAction::Publish { event, .. } => {
                if event.id.is_empty() || event.sig.is_empty() {
                    return Err(ActionRejection::Invalid(
                        "publish action requires a signed event with id+sig".to_string(),
                    ));
                }
                Ok(())
            }
            PublishAction::PublishNote { content, .. } => {
                if content.is_empty() {
                    return Err(ActionRejection::Invalid(
                        "publish note requires non-empty content".to_string(),
                    ));
                }
                Ok(())
            }
            PublishAction::PublishProfile { fields } => {
                // A kind:0 `content` is a flat JSON object of string values
                // (NIP-01 metadata). Reject any non-string field up front so a
                // malformed profile never reaches the actor.
                for (key, value) in &fields {
                    if !value.is_string() {
                        return Err(ActionRejection::Invalid(format!(
                            "profile field '{key}' must be a string value"
                        )));
                    }
                }
                Ok(ActionPlan {
                    initial_step: PublishStep::Planning,
                    initial_status: ActionStatus::Pending,
                    deadline_ms: None,
                })
            }
            PublishAction::Cancel { handle } => {
                if handle.is_empty() {
                    return Err(ActionRejection::Invalid(
                        "cancel requires a publish handle".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }
}
