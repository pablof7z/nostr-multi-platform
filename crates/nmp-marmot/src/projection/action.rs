//! `MarmotAction` plus Marmot's internal typed [`ActionModule`] implementation.
//! `nmp_marmot::install` registers this namespace with the runtime action
//! dispatcher; that is the current and only Marmot write doorway. The ADR-0025
//! bespoke native write doorway is retired.
//!
//! # Where this fits
//!
//! Marmot writes reach `MarmotService` through the substrate-generic seam:
//! [`crate::install`] registers a typed [`ActionModule`] under the
//! `"nmp.marmot"` namespace, hosts dispatch a runtime action envelope for that
//! namespace, and `execute` sends a typed [`MarmotProtocolCommand`] through
//! `ActorCommand::Protocol`. Returns a `correlation_id` synchronously; the
//! terminal verdict surfaces on `action_stages`.
//!
//! # JSON shape
//!
//! The enum is `#[serde(tag = "op", rename_all = "snake_case")]` so the
//! wire shape is the stable Marmot action envelope existing shells produce:
//!
//! ```json
//! {"op": "create_group", "name": "engineering", "description": "...", "invitee_text": "...", "signed_key_package_events_json": []}
//! {"op": "send", "group_id_hex": "abc...", "text": "hello"}
//! {"op": "publish_key_package"}
//! ```
//!
//! # `start()` validates shape; `MarmotProtocolCommand` does the work
//!
//! `MarmotActionModule::start` is the validator — it deserializes the
//! action JSON into the typed `MarmotAction` enum and rejects malformed
//! payloads at the boundary. `MarmotActionModule::execute` then emits
//! `ActorCommand::Protocol(Box<MarmotProtocolCommand>)` with the already
//! parsed enum and the crate-owned live runtime. The command runs on the
//! actor thread, uses [`ProtocolCommandContext`] for actor-authored time and
//! publish/interest commands, and records the terminal action verdict.

use std::sync::Arc;

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRejection,
    ProtocolCommand, ProtocolCommandContext, ProtocolCommandError,
};
use serde::{Deserialize, Serialize};

use crate::projection::state::MarmotRuntimePort;
use crate::runtime::MarmotRuntime;

/// Namespace registered by [`crate::install`] in the kernel's
/// [`nmp_core::kernel::ActionRegistry`]. Hosts dispatch through the runtime
/// action dispatcher for this namespace.
///
/// Named after the Marmot protocol (the MLS-over-Nostr binding), not an app
/// crate. Any host that installs `nmp_marmot::install` uses the same namespace;
/// the namespace is a wire contract, not an implementation tag.
pub const MARMOT_ACTION_NAMESPACE: &str = "nmp.marmot";

/// Typed Marmot action enum.
///
/// `#[serde(tag = "op", rename_all = "snake_case")]` keeps the on-the-wire
/// JSON byte-identical with the historical Marmot action envelope (the
/// `{"op": "create_group", ...}` shape existing shells already produce).
///
/// `#[serde(deny_unknown_fields)]` is NOT applied here — the legacy
/// envelope tolerates ignored extra fields (e.g. iOS sometimes appends
/// `signed_key_package_events_json: []` to `invite` and `create_group`
/// even when empty), and rejecting them would break the migration.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum MarmotAction {
    /// Publish (or rotate) the local MLS key-package as kind:30443.
    ///
    /// `relays` is the fallback write-relay set [`crate::projection::ops`]'s
    /// `resolve_write_relays` uses when the host's NIP-65 write list is
    /// empty (the test / non-host-wired path). In production the host's
    /// kind:10002 write relays override it.
    PublishKeyPackage {
        #[serde(default)]
        relays: Vec<String>,
    },
    /// Create a new MLS group. The optional `signed_key_package_events_json`
    /// + `invitee_text` / `invitee_npubs` selects invitees; missing key
    /// packages return the soft-fail `{"ok":false,"error":"key_package_unavailable"}`
    /// envelope.
    CreateGroup {
        name: String,
        #[serde(default)]
        description: String,
        /// Whitespace / comma / semicolon / newline -separated list of npubs;
        /// preferred over `invitee_npubs` when both are present.
        #[serde(default)]
        invitee_text: Option<String>,
        /// Pre-tokenized invitee npub list (used by REPL / tests; the iOS
        /// bridge uses `invitee_text`).
        #[serde(default)]
        invitee_npubs: Option<Vec<String>>,
        /// Optional pre-fetched signed kind:30443 key-package
        /// events as JSON strings. Empty → fall back to the in-process
        /// cache populated by the Marmot ingest parser.
        #[serde(default)]
        signed_key_package_events_json: Vec<serde_json::Value>,
        /// Fallback write-relay set when the host's NIP-65 list is empty.
        /// Same role as on `PublishKeyPackage` — production hosts override
        /// via kind:10002.
        #[serde(default)]
        relays: Vec<String>,
    },
    /// Invite peers to an existing MLS group. Same `invitee_*` /
    /// `signed_key_package_events_json` semantics as `CreateGroup`.
    Invite {
        group_id_hex: String,
        #[serde(default)]
        invitee_text: Option<String>,
        #[serde(default)]
        invitee_npubs: Option<Vec<String>>,
        #[serde(default)]
        signed_key_package_events_json: Vec<serde_json::Value>,
    },
    /// Send a kind:14 NIP-44 group message — MDK builds the kind:1059
    /// gift-wrap that is published to the group's relay-pinned relays.
    Send { group_id_hex: String, text: String },
    /// Self-remove from a group (MLS SelfRemove proposal + commit).
    Leave { group_id_hex: String },
    /// Remove other members from the group (MLS Remove proposal + commit).
    Remove {
        group_id_hex: String,
        #[serde(default)]
        member_npubs: Vec<String>,
    },
    /// Accept a previously-cached pending Welcome (gift-wrap event id hex).
    AcceptWelcome { welcome_id_hex: String },
    /// Decline a previously-cached pending Welcome.
    DeclineWelcome { welcome_id_hex: String },
    /// Explicit pending-commit clear (mdk-api.md §7.7) — exposed so a
    /// caller that detected a relay-publish failure can unwedge the group.
    ClearPending { group_id_hex: String },
}

/// The substrate-generic `ActionModule` registered under
/// [`MARMOT_ACTION_NAMESPACE`].
///
/// Mirrors the shape of every other `ActionModule` in the workspace
/// (`PublishModule`, `nmp_nip02::ReactModule`, etc.): `start()` validates the typed
/// action; `execute()` emits one `ActorCommand` carrying everything the
/// actor needs to run the op. The only Marmot-specific piece is the shared
/// runtime captured by the typed protocol command — see the module rustdoc.
pub(crate) struct MarmotActionModule {
    runtime: Arc<MarmotRuntime>,
}

impl MarmotActionModule {
    #[must_use]
    pub(crate) fn new(runtime: Arc<MarmotRuntime>) -> Self {
        Self { runtime }
    }
}

pub(crate) struct MarmotProtocolCommand {
    runtime: Arc<MarmotRuntime>,
    body: MarmotProtocolCommandBody,
}

enum MarmotProtocolCommandBody {
    Action {
        action: MarmotAction,
        correlation_id: Option<String>,
    },
}

impl std::fmt::Debug for MarmotProtocolCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dbg = f.debug_struct("MarmotProtocolCommand");
        match &self.body {
            MarmotProtocolCommandBody::Action {
                action,
                correlation_id,
            } => {
                dbg.field("action", action)
                    .field("correlation_id", correlation_id);
            }
        }
        dbg.finish_non_exhaustive()
    }
}

impl MarmotProtocolCommand {
    #[must_use]
    pub(crate) fn new(
        runtime: Arc<MarmotRuntime>,
        action: MarmotAction,
        correlation_id: String,
    ) -> Self {
        Self {
            runtime,
            body: MarmotProtocolCommandBody::Action {
                action,
                correlation_id: Some(correlation_id),
            },
        }
    }
}

struct MarmotCommandPort<'a, 'ctx> {
    ctx: &'a ProtocolCommandContext<'ctx>,
}

impl MarmotRuntimePort for MarmotCommandPort<'_, '_> {
    fn publish_signed_explicit(&self, event: &nostr::Event, relays: &[nostr::RelayUrl]) {
        self.ctx.publish_signed_to_relays(
            event.clone(),
            relays
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
            nmp_core::publish::PublishRouteClass::ImportedOrPresigned,
            None,
        );
    }

    fn ensure_interest(
        &self,
        identity: nmp_core::subs::SubIdentity,
        interest: nmp_planner::LogicalInterest,
    ) {
        self.ctx.ensure_interest(identity, interest);
    }

    fn write_relay_urls(&self, author_hex: &str, kind: u32) -> Vec<String> {
        self.ctx.recipient_publish_relays(author_hex, kind)
    }

    fn send_actor_command(&self, cmd: ActorCommand) {
        self.ctx.send(cmd);
    }
}

impl ProtocolCommand for MarmotProtocolCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let MarmotProtocolCommand { runtime, body } = *self;

        let (action, correlation_id) = match body {
            MarmotProtocolCommandBody::Action {
                action,
                correlation_id,
            } => (action, correlation_id),
        };

        if let Some(correlation_id) = correlation_id.as_deref() {
            ctx.record_action_stage_requested(correlation_id);
        }
        let Some(projection) = runtime.projection() else {
            if let Some(correlation_id) = correlation_id {
                ctx.record_action_failure(
                    correlation_id,
                    "marmot requires an active local-key account".to_string(),
                );
            }
            return Ok(());
        };
        let now_secs = ctx.now_secs();
        let result = {
            let port = MarmotCommandPort { ctx };
            projection
                .with_inner_port(&port, |h| {
                    crate::projection::ops::dispatch(
                        h,
                        &action,
                        now_secs,
                        correlation_id.as_deref(),
                    )
                })
                .unwrap_or_else(|| {
                    serde_json::json!({
                        "ok": false,
                        "error": "MarmotProtocolCommand: projection mutex poisoned",
                    })
                })
        };

        let flag = |k| {
            result
                .get(k)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        };
        if flag("pending") {
            return Ok(());
        }
        let Some(correlation_id) = correlation_id else {
            return Ok(());
        };
        if flag("ok") {
            ctx.record_action_success(correlation_id, None);
        } else {
            let reason = result
                .get("error")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("marmot op failed without an error message")
                .to_string();
            ctx.record_action_failure(correlation_id, reason);
        }
        Ok(())
    }
}

impl ActionModule for MarmotActionModule {
    const NAMESPACE: &'static str = MARMOT_ACTION_NAMESPACE;
    type Action = MarmotAction;

    /// `start()` is a pure validator. The typed `MarmotAction` enum's
    /// `Deserialize` impl already enforces shape (missing required fields,
    /// wrong types). Per-op semantic validation (e.g. valid group_id hex
    /// length) deliberately stays in the existing
    /// [`crate::projection::ops::dispatch`] handlers — they ALREADY return
    /// `{"ok":false,"error":"..."}` for those cases, and re-checking here
    /// would split the validation across two layers (the doctrine of "one
    /// owner per fact").
    ///
    /// D6 — JSON shape rejection happens before this method runs (the
    /// `ActionRegistry` adapter parses the JSON into `Self::Action` first);
    /// reaching this body means the typed enum is well-formed.
    fn start(
        &self,
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Ok(())
    }

    /// Mark the module as asynchronously-completing so the kernel's
    /// `action_stages` mirror is exercised end-to-end:
    ///
    /// * the registry mints a `correlation_id` and returns it to the host;
    /// * the typed protocol command records `Requested` →
    ///   terminal (`Accepted` on `ok:true`, `Failed` on `ok:false`) under
    ///   that id;
    /// * the host's spinner clears on the next snapshot tick.
    ///
    /// Returning `false` here would skip the `action_stages` mirror writes
    /// and the host would never see a terminal verdict for a Marmot op.
    fn is_async_completing() -> bool {
        // doctrine-allow: D12 — stage transitions are recorded by the typed protocol command on the Protocol arm, not here; this is the seam declaration so the registry routes the verdict.
        true
    }

    /// Decode a typed FlatBuffers payload produced by the host builder (M14-1c /
    /// #2169). Delegates to [`ActionPayload::decode`] on [`MarmotAction`], which
    /// checks the `NMMA` file identifier and the `schema_version` tripwire
    /// before reconstructing the union arm. Returns `Some(...)` always — this
    /// module is now typed-only through the byte doorway.
    fn decode_payload(bytes: &[u8]) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<Self::Action as ActionPayload>::decode(bytes))
    }

    /// Hand the typed action to [`MarmotProtocolCommand`] on the `Protocol`
    /// arm. The command owns the actor-thread execution and records the
    /// terminal verdict under `correlation_id`.
    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Protocol(Box::new(
            MarmotProtocolCommand::new(
                Arc::clone(&self.runtime),
                action,
                correlation_id.to_string(),
            ),
        )));
        Ok(())
    }
}

#[cfg(test)]
#[path = "action/tests.rs"]
mod tests;
