//! Action substrate — the `ActionModule` trait + `ActionResult` shape that
//! back the kernel's `dispatch_action` runtime.
//!
//! # Theme A discriminator — one door per publish capability
//!
//! The one-door-per-capability rule codifies the governance that emerged
//! when the bespoke `nmp_app_publish_signed_event{,_to}` /
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
pub struct ActionContext {}

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
    ///
    /// Default: no-op accept. Override only when upfront validation is
    /// needed (empty fields, hex shape, invariant checks). Modules whose
    /// kernel command handler owns all error toasting can omit this method.
    ///
    /// Takes `&self` (ADR-0052 rung 5.2): the registry stores the concrete
    /// module **value**, so `start` may read state the host captured at
    /// composition time (a stateful module owns e.g. an
    /// `Arc<WalletRuntimeHandle>`). Stateless modules ignore `&self`.
    #[allow(unused_variables)]
    fn start(&self, ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        Ok(())
    }

    /// Optional: suggest the `correlation_id` the registry should assign to
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

    /// Declare that this module's actions settle ASYNCHRONOUSLY — the
    /// dispatch return value does not yet carry the terminal outcome; the
    /// actor signs / publishes / awaits an external ack, and the result
    /// arrives later through `projections["action_stages"]`.
    ///
    /// Defaults to `false`. A module that overrides this to `true` MUST
    /// record stage transitions (`Requested` → `Publishing` →
    /// `Accepted`/`Failed`) via `Kernel::record_action_stage`; doctrine-lint
    /// rule **D12** enforces this statically per file.
    #[must_use]
    fn is_async_completing() -> bool {
        false
    }

    /// Enqueue the `ActorCommand` that the validated `action` should drive.
    ///
    /// Called via `ActionModuleAdapter<M>` (see `kernel::action_registry`)
    /// after `start` returns `Ok`. Thread `correlation_id` onto any
    /// `ActorCommand` whose terminal verdict must report the dispatched id
    /// (the spinner round-trip).
    ///
    /// The pre-ADR-0027 dual-registration path (`register_action_module` /
    /// `register_action_executor`) was deleted; `execute` is now the sole
    /// executor seam for any registered module.
    ///
    /// Takes `&self` (ADR-0052 rung 5.2): the dependencies a command needs
    /// (e.g. an `Arc<WalletRuntimeHandle>`) are owned by the registered
    /// module value and captured at composition time, rather than reached
    /// through a process-global. Stateless modules ignore `&self`.
    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(crate::actor::ActorCommand),
    ) -> Result<(), String>;
}

/// App-neutral action registration seam.
///
/// Reusable protocol crates use this trait instead of naming the concrete
/// `nmp-ffi::NmpApp` C-ABI host handle. The host decides where the registry
/// lives; modules only require "register this typed action module".
pub trait ActionRegistrar {
    /// Register `M` as an **app** action module under `M::NAMESPACE` — an
    /// explicit, intentional registration that overrides a yielding default
    /// (legal) but collides loudly with another app registration of the same
    /// namespace (ADR-0049 Part 1). This is the path app-specific verbs
    /// (Chirp's NIP-29, wallet, …) use.
    ///
    /// Takes the module **value** (ADR-0052 rung 5.2): a stateful module
    /// (e.g. a wallet module owning an `Arc<WalletRuntimeHandle>`) carries
    /// its dependencies, captured by the host at composition time. Stateless
    /// modules pass a unit-shaped value (`register_action(PublishModule)`).
    fn register_action<M: ActionModule + 'static>(&mut self, module: M);

    /// Register `M` as a **yielding default** under `M::NAMESPACE` — install it
    /// ONLY if the namespace is unclaimed; otherwise yield to the existing
    /// registration REGARDLESS of call order (ADR-0049 Part 1, the
    /// Spring-Boot `@ConditionalOnMissingBean` shape). Returns `true` when
    /// installed, `false` when it yielded.
    ///
    /// The canonical NMP defaults (`nmp_nip02` / `nmp_nip17` / `nmp_nip57`
    /// action modules, the NIP-65 publish-relay-list module in `nmp-router`)
    /// register through THIS path so an app may pre-empt any of them.
    ///
    /// Default impl: delegate to [`Self::register_action`] and report `true`.
    /// This keeps non-recording / test [`ActionRegistrar`] impls valid without
    /// re-implementing yielding semantics; the real entry-or-insert behaviour
    /// lives in the kernel's `ActionRegistry` override.
    ///
    /// Takes the module **value** (ADR-0052 rung 5.2), as [`Self::register_action`].
    fn register_default_action<M: ActionModule + 'static>(&mut self, module: M) -> bool {
        self.register_action(module);
        true
    }
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
