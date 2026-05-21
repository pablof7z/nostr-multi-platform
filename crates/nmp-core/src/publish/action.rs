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
/// sign once via the active signer and then enqueue the publish — we never
/// re-sign on retry (per the M6 exit gate "re-publish of an event preserves
/// `id` and `sig`").
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum PublishAction {
    Publish {
        handle: PublishHandle,
        event: SignedEvent,
        target: PublishTarget,
    },
    /// Sign-and-publish a kind:1 note (optionally a reply) with the active
    /// account. Unlike `Publish`, the event is *not* pre-signed — the actor
    /// signs it via the active signer. This is the `ActionModule`-native
    /// replacement for the deleted per-verb `nmp_app_publish_note` FFI symbol;
    /// the executor routes it to the existing `ActorCommand::PublishNote`
    /// handler.
    PublishNote {
        content: String,
        reply_to_id: Option<String>,
        target: PublishTarget,
    },
    /// Publish a kind:0 profile metadata event for the active account.
    /// `fields` is a flat JSON object with string-valued keys such as
    /// `"name"`, `"about"`, `"picture"` — the actor serializes it into the
    /// kind:0 `content` field, signs with the active signer, and routes
    /// through the NIP-65 outbox. Like `PublishNote`, the event is
    /// *not* pre-signed: the actor stamps `created_at` and signs. This is the
    /// `ActionModule`-native path for hosts that need to publish kind:0
    /// metadata events; PR-F deleted the prior bespoke
    /// `nmp_app_publish_unsigned_event` FFI symbol, so this `PublishAction`
    /// variant + `nmp_app_dispatch_action("nmp.publish", ...)` is the only
    /// door for it.
    PublishProfile {
        fields: serde_json::Map<String, serde_json::Value>,
    },
    /// Cancel an in-flight publish, addressed by its [`PublishHandle`].
    ///
    /// This variant is the publish *engine's* internal command shape — it is
    /// constructed by `Kernel::cancel_publish` (the handler for
    /// `ActorCommand::CancelPublish`, the FFI symbol `nmp_app_cancel_publish`)
    /// and matched by `PublishEngine::start_publish`. It is deliberately NOT
    /// dispatchable through `dispatch_action`: `PublishModule::start` rejects
    /// it so the publish lifecycle's control plane (cancel / retry) stays on
    /// the dedicated FFI symbols rather than the generic action seam.
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
    /// here means `dispatch_action`'s return and `action_results` in the
    /// snapshot share the same identifier.
    ///
    /// `PublishNote` and `PublishProfile` return `None` — the event id isn't
    /// known until the actor signs. `Cancel` is not reachable through
    /// `dispatch_action` (`start` rejects it), so it never reaches this
    /// function; it falls into the `_` arm and returns `None`.
    fn preferred_action_id(action: &Self::Action) -> Option<crate::substrate::ActionId> {
        match action {
            PublishAction::Publish { event, .. } if !event.id.is_empty() => {
                Some(event.id.clone())
            }
            _ => None,
        }
    }

    /// PR-G: publish actions settle asynchronously — the actor signs,
    /// hands the event to the publish engine, and the terminal verdict
    /// arrives through `projections["action_results"]` on a later tick.
    /// Per the trait contract this module records `Requested` →
    /// `Publishing` → `Accepted`/`Failed` stages via
    /// `Kernel::record_action_stage`. The actual call sites live in
    /// sibling files (the engine wrapper drives them, not the module type):
    ///
    /// * `Requested` — `crates/nmp-core/src/actor/dispatch.rs`
    ///   (PublishNote / PublishProfile / PublishSignedEvent arms)
    /// * `Publishing` — `crates/nmp-core/src/kernel/publish_engine.rs`
    ///   (`run_publish_engine_at` Ok arm)
    /// * `Accepted` / `Failed` — `crates/nmp-core/src/kernel/publish_engine.rs`
    ///   (`take_action_results_projection`) and
    ///   `crates/nmp-core/src/kernel/publish_cmd.rs`
    ///   (`record_action_failure`, sign-step path)
    ///
    /// The D12 grep-level lint asserts the *file* declaring the marker
    /// also contains a recording call. PublishModule's declaration file
    /// (this one) has none — the recording sites listed above live in
    /// sibling files (actor/dispatch.rs, kernel/publish_engine.rs,
    /// kernel/publish_cmd.rs). A cross-file scan would be the AST-level
    /// rule the spec deferred; until then we opt out via the
    /// `// doctrine-allow: D12 — ...` directive D12 already honours.
    /// The recording sites above are exercised end-to-end by
    /// `kernel/action_stages_tests.rs` — a recording-missing regression
    /// is caught there, just not at lint time.
    fn is_async_completing() -> bool { // doctrine-allow: D12 — recording sites are cross-file (actor/dispatch.rs + kernel/publish_*.rs); exercised by kernel/action_stages_tests.rs
        true
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
                Ok(())
            }
            // Cancel is engine-internal — it is constructed by
            // `Kernel::cancel_publish` for the `nmp_app_cancel_publish` FFI
            // symbol, never dispatched through `dispatch_action`. Reject it
            // here so the publish lifecycle's control plane stays on the
            // dedicated FFI door and `dispatch_action` carries nothing for
            // cancel. Previously this arm was an accepting no-op whose
            // executor counterpart did `Ok(())` — a dead path that looked
            // alive on the action seam.
            PublishAction::Cancel { .. } => Err(ActionRejection::Invalid(
                "publish cancel is not dispatchable via dispatch_action; \
                 use the nmp_app_cancel_publish FFI symbol"
                    .to_string(),
            )),
        }
    }
}
