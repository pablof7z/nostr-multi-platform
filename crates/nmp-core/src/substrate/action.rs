//! Action substrate — the `ActionModule` trait + `ActionResult` shape that
//! back the kernel's `dispatch_action` runtime.
//!
//! # Theme A discriminator — one door per publish capability
//!
//! PR-F (one door per capability) codified the rule that already governed
//! the FFI surface after the bespoke `nmp_app_publish_signed_event{,_to}` /
//! `nmp_app_publish_unsigned_event` symbols were deleted:
//!
//! - **Generic user/app-authored publish-engine events go through
//!   [`crate::ffi::action::nmp_app_dispatch_action`]** under the
//!   `nmp.publish` namespace (or a per-NIP namespace whose executor builds
//!   `PublishAction::*` and routes via the same engine). The host hands the
//!   action seam an `UnsignedEvent` / pre-signed `Event`; the kernel signs
//!   (when needed), verifies, and dispatches through the publish engine
//!   with a registry-minted `correlation_id` reported in
//!   `action_results`. This is the single, observable, host-extensible
//!   door for content actions.
//!
//! - **System-authored / lifecycle / wallet capabilities stay bespoke.**
//!   They are not "actions a user dispatches"; they are mechanisms the
//!   kernel or a sibling crate uses to keep the system honest:
//!     - publish-lifecycle control plane —
//!       [`crate::ffi::publish::nmp_app_retry_publish`] /
//!       [`crate::ffi::publish::nmp_app_cancel_publish`] address an
//!       already-queued publish *handle*, never produce events, and have
//!       no `dispatch_action` equivalent (and never should — the action
//!       seam is for content actions).
//!     - MLS / gift-wrap publish — [`crate::NmpApp::publish_signed_explicit`]
//!       carries events signed by an MLS group credential (kind:445) or an
//!       ephemeral key (kind:1059 gift-wrap) that the kernel's signer
//!       cannot re-mint. The generic action seam signs + publishes; this
//!       entrypoint publishes verbatim without re-signing.
//!     - NIP-47 wallet — bespoke `nmp_app_wallet_*` symbols (gated by the
//!       `wallet` feature). NWC RPC is a connection-oriented protocol, not
//!       a content action.
//!
//! The discriminator a reviewer applies to any new symbol:
//!
//! > *Is this a user or app intent to author a Nostr event, where the
//! > kernel decides which identity signs and where it lands?* If yes,
//! > register an `ActionModule` and route through `dispatch_action`. If
//! > no — it is system-authored, addresses a publish handle, or operates
//! > on a non-content protocol — it may live on a bespoke entrypoint, but
//! > it MUST NOT construct `ActorCommand::PublishSignedEvent` /
//! > `PublishUnsignedEvent` inside an `extern "C" fn nmp_app_*` body
//! > (D11 lint catches that regression).

use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub type ActionId = String;

#[derive(Clone, Debug, Default)]
pub struct ActionContext {
    pub now_ms: u64,
}

pub trait ActionModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;

    type Action: Clone + Serialize + DeserializeOwned + Send + 'static;

    /// Validate `action`. `Ok(())` accepts it (the registry mints a
    /// correlation id and the executor enqueues it); `Err` rejects it.
    ///
    /// `start` carries no return payload: it is a pure validator. The
    /// per-action lifecycle (step / status / deadline) was discarded at the
    /// `dispatch_action` boundary and never reached the host or the actor, so
    /// the `ActionPlan` return type it once produced has been removed.
    fn start(
        ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection>;

    /// Optional: suggest the correlation_id the registry should assign to
    /// this action instead of the auto-generated one. Returning `Some(id)`
    /// makes `dispatch_action`'s return value and `action_results` in the
    /// snapshot use the same identifier — a requirement for hosts that key
    /// spinners on the returned id.
    ///
    /// Default: `None` — the registry generates a unique 32-hex id.
    ///
    /// Override when the action's natural identity is already a stable,
    /// collision-free string visible to the engine (e.g. the pre-signed
    /// event's `id` for `PublishAction::Publish`).
    fn preferred_action_id(_action: &Self::Action) -> Option<ActionId> {
        None
    }

    /// PR-G — declare that this module's actions settle ASYNCHRONOUSLY (i.e.
    /// the dispatch return value does NOT yet carry the terminal outcome;
    /// the actor signs / publishes / awaits an ack / etc., and the result
    /// arrives later through the snapshot path).
    ///
    /// Defaults to `false`. A module that overrides this to `true` is
    /// declaring a contract with the host: the action will produce a
    /// lifecycle the host can observe through `projections["action_stages"]`
    /// (`Requested` → `Publishing` → `Accepted`/`Failed`) and MUST record
    /// stage transitions via `Kernel::record_action_stage` so the mirror
    /// reflects reality. The doctrine-lint rule **D12** enforces this:
    /// any file declaring `fn is_async_completing(...) -> bool` with a
    /// non-`false` body must also contain a `record_action_stage` call,
    /// otherwise the module ships an empty stage seam.
    ///
    /// `PublishModule` returns `true` (the publish actor lifecycle is the
    /// canonical async-completing example). Synchronous actions — those
    /// whose result is already committed when `dispatch_action` returns —
    /// leave the default.
    fn is_async_completing() -> bool {
        false
    }

    /// ADR-0027 typed-executor seam: enqueue the `ActorCommand` that the
    /// validated `action` should drive. Called via `ActionModuleAdapter<M>`
    /// (see `kernel::action_registry`) when the module is registered through
    /// `ActionRegistry::register::<M>()` and `has_typed_executor` returns
    /// `true`. The pre-ADR-0027 closure path (`register_executor`) remains
    /// available for hosts that haven't migrated yet; the typed path takes
    /// precedence when both are present.
    ///
    /// Thread `correlation_id` onto any `ActorCommand` whose terminal verdict
    /// must report the dispatched id (the spinner round-trip — see PR-A).
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ActionRejection {
    Invalid(String),
    Unauthorized(String),
    Conflict(String),
}

/// Delivered to a registered result observer when an action has been
/// **accepted by the registry and enqueued** for execution.
///
/// This is a *push* "action accepted" signal, NOT a completion carrier.
/// Delivery happens after [`crate::kernel::ActionRegistry`]'s `execute`
/// returns `Ok` — i.e. once the action's [`crate::actor::ActorCommand`] has
/// been queued. For an action like `nmp.publish` the actor still has to
/// verify and publish the event after this fires; that eventual outcome is
/// reported through the snapshot-projection (pull) path, not this channel.
///
/// Built-in executors are fire-and-forget and deliver `result_json: null`.
/// A host executor that needs to return a value to the caller writes that
/// value into a snapshot projection (the pull model); `ActionResult` then
/// stays a uniform "accepted" signal, consistent with the single-actor model.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActionResult {
    pub correlation_id: String,
    /// JSON-encoded result value, or `null` for fire-and-forget actions.
    pub result_json: serde_json::Value,
}
