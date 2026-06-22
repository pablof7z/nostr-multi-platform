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
//!     - publish-lifecycle control plane — `nmp_app_retry_publish` (by
//!       publish *handle*) and `nmp_app_cancel_action` (by operation
//!       `correlation_id`; S7/#1754 replaced the bespoke
//!       `nmp_app_cancel_publish` handle symbol). Neither produces events, and
//!       neither has a `dispatch_action` equivalent (and never should — the
//!       action seam is for content actions).
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

/// Fail-closed reasons a typed [`ActionPayload`] decode can reject. Errors are
/// **data** (D6) — never a panic, never a `Result`/exception across the FFI or
/// worker boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionPayloadDecodeError {
    /// The buffer's `schema_version` is not the version this crate compiled
    /// against. The RAW value is reported; the payload is NOT decoded further.
    /// This is the **before-`start()`** fail-closed tripwire (ADR-0064 §1).
    SchemaVersionMismatch { found: u32, expected: u32 },
    /// The buffer is not a valid root for this payload (missing/wrong file
    /// identifier, truncated/corrupt FlatBuffers, or a missing required field).
    Malformed { reason: String },
}

impl core::fmt::Display for ActionPayloadDecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SchemaVersionMismatch { found, expected } => write!(
                f,
                "action payload schema_version mismatch: {found} (expected {expected})"
            ),
            Self::Malformed { reason } => write!(f, "action payload malformed: {reason}"),
        }
    }
}

/// A typed FlatBuffers action payload (ADR-0064 / S3 #1751).
///
/// This is the **inbound** typed decode contract for a migrated
/// [`ActionModule`]: the open [`crate::transport::dispatch_envelope`] carries
/// the per-crate payload as opaque bytes; the registry adapter decodes those
/// bytes through [`ActionPayload::decode`] into the module's `Action` type,
/// then runs `start()`.
///
/// # Fail-closed schema_version, BEFORE `start()`
///
/// Each payload buffer self-describes its `schema_version` as a field. The
/// registry reads the RAW value and compares it to [`Self::SCHEMA_VERSION`]
/// **before** the module's `start()` validator runs — a mismatch is rejected
/// ([`ActionPayloadDecodeError::SchemaVersionMismatch`]) and the module never
/// sees the action. The decoder NEVER guesses a version; a mismatch is a
/// reject, not a multi-version decode (ADR-0064 §1).
///
/// # Opaque pre-signed bytes
///
/// A payload carrying a pre-signed event (`nmp.publish`'s `Publish` variant)
/// keeps the canonical NIP-01 event as OPAQUE BYTES — it must round-trip
/// byte-for-byte so the signature stays valid. The typed table NEVER re-models
/// the signed event (ADR-0064 / #1751).
pub trait ActionPayload: Sized {
    /// Stable identity of this payload schema (e.g. `"nmp.publish"`), carried in
    /// diagnostics. Distinct from the host-routing `ActionModule::NAMESPACE`,
    /// though they commonly match.
    const SCHEMA_ID: &'static str;

    /// The payload schema version this crate compiled against. A buffer carrying
    /// any other value is rejected before `start()` (fail-closed tripwire).
    const SCHEMA_VERSION: u32;

    /// Decode typed FlatBuffers `bytes` into the action shape, running the
    /// fail-closed `schema_version` gate FIRST. Returns
    /// [`ActionPayloadDecodeError::SchemaVersionMismatch`] on a version trip and
    /// [`ActionPayloadDecodeError::Malformed`] on any structural error.
    fn decode(bytes: &[u8]) -> Result<Self, ActionPayloadDecodeError>;

    /// Encode the action shape to finished, file-identified FlatBuffers bytes
    /// (stamping [`Self::SCHEMA_VERSION`]). The production app-facing path is the
    /// generated typed builders (ADR-0064 §3); this is the kernel-side primitive
    /// they (and round-trip tests) build on.
    #[must_use]
    fn encode(&self) -> Vec<u8>;
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

    /// Typed FlatBuffers payload decode (ADR-0064 / S3 #1751), the OPT-IN
    /// counterpart to the serde-JSON `Action` path.
    ///
    /// Returns:
    /// * `None` — this module is **not** typed-payload-capable (the default;
    ///   the 30+ modules ADR-0064 migrates per-crate stay on JSON until their
    ///   own stage). The registry's typed-bytes doorway rejects the namespace.
    /// * `Some(Ok(action))` — the bytes decoded into `Self::Action`.
    /// * `Some(Err(_))` — fail-closed: a `schema_version` trip (gated BEFORE
    ///   `start()`) or a malformed buffer. The module never sees the action.
    ///
    /// A migrated module overrides this to delegate to
    /// [`ActionPayload::decode`] on its `Action` type, e.g.:
    ///
    /// ```ignore
    /// fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
    ///     Some(<Self::Action as ActionPayload>::decode(bytes))
    /// }
    /// ```
    ///
    /// This opt-in keeps a single typed-decode SITE (the registry adapter calls
    /// exactly this method) while letting the migration stay staged: a module
    /// whose `Action` is e.g. `serde_json::Value` cannot implement
    /// [`ActionPayload`], and simply leaves this defaulted.
    #[allow(unused_variables)]
    fn decode_payload(
        bytes: &[u8],
    ) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        None
    }

    /// OPT-IN to the opaque-passthrough byte route (#1756 / S9, the Cut-B
    /// enabler), the second of the two byte-doorway opt-ins (the first being
    /// [`Self::decode_payload`] for typed FlatBuffers modules).
    ///
    /// APP-OWNED host-op namespaces (e.g. `podcast.tasks`) carry an `Action`
    /// the kernel does NOT model — typically `serde_json::Value` — so they
    /// cannot implement [`crate::substrate::ActionPayload`] and leave
    /// [`Self::decode_payload`] defaulted (`None`). Without an opt-in such a
    /// module is unreachable through the byte doorway (`NotTypedCapable`),
    /// which is what blocks deleting the JSON doorway at Cut B.
    ///
    /// A module that overrides this to `true` declares: *the
    /// [`DispatchEnvelope`](crate::transport::dispatch_envelope)'s opaque
    /// `payload` bytes ARE my action, in a format **I** own (JSON-bytes); the
    /// kernel must pass them through UNDECODED by any kernel schema.* The
    /// registry then `serde_json::from_slice`s those bytes into `Self::Action`
    /// (the app's own deserialize) and runs `start()` / `execute()`. NMP imposes
    /// no FlatBuffers `schema_version` gate on the payload — it is opaque to NMP
    /// — but the ENVELOPE framing (file identifier, envelope `schema_version`
    /// tripwire, oversize bound, namespace + correlation_id presence) is still
    /// validated by the S2 decoder before this route is reached.
    ///
    /// # Fail-closed (#1756)
    ///
    /// This is a DELIBERATE per-module opt-in, NOT a blanket fallback. A module
    /// that is neither typed ([`Self::decode_payload`] `Some`) nor opaque-opted
    /// (this returns the default `false`) is REJECTED by the byte doorway — an
    /// unknown namespace or a non-opted untyped module never reaches `start()`.
    ///
    /// A module MUST NOT return `true` here AND a `Some` from
    /// [`Self::decode_payload`]: the two byte routes are mutually exclusive (one
    /// payload format per namespace). The adapter resolves typed-first and the
    /// opaque route is only taken when `decode_payload` is `None`, so a module
    /// that sets both silently never exercises the opaque path; doctrine treats
    /// the typed route as canonical for such a (mis)configured module.
    #[must_use]
    fn accepts_opaque_payload() -> bool {
        false
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
    /// A structured rejection carrying a stable machine `code` and an English
    /// `message` fallback (UiToken shape, issue #1734). The FFI layer surfaces
    /// `{"error":"…","code":"…"}` so shells can localize. Prefer this over
    /// `Invalid` when the rejection site is NWC-connect or another curated path
    /// that owns its prose in the crate's `ui_codes` module.
    InvalidCoded {
        /// Stable machine key from the owning crate's closed `ui_codes` set.
        code: &'static str,
        /// English prose for non-localizing shells / diagnostics.
        message: String,
    },
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
