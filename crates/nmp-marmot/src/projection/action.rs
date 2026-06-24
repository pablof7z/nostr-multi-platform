//! `MarmotAction` + `MarmotActionModule` — the typed [`ActionModule`] surface
//! that routes Marmot writes through `nmp_app_dispatch_action`. This is the
//! architecturally-correct replacement for the legacy bespoke
//! `nmp_marmot_dispatch` C-ABI symbol (deleted in ADR-0025 PR 3,
//! 2026-05-23 — the ADR-0025 exception is fully retired).
//!
//! # Where this fits
//!
//! Marmot has two op streams reaching `MarmotService`:
//!
//! * **the substrate-generic seam** (this module, the SOLE host entry
//!   point) — registers a typed [`ActionModule`] under the `"nmp.marmot"`
//!   namespace; the host calls
//!   `nmp_app_dispatch_action("nmp.marmot", action_json)`; `execute`
//!   sends a typed [`MarmotProtocolCommand`] through
//!   `ActorCommand::Protocol`. Returns a `correlation_id` synchronously;
//!   the terminal verdict surfaces on `action_stages`.
//! * **the Rust-native accessor** ([`crate::ffi::MarmotHandle::dispatch`])
//!   — for in-process callers (REPL / TUI / integration tests) that need
//!   the full synchronous per-op envelope (`events`, `welcome_rumors`,
//!   `evolution_event`, …). Not a C-ABI symbol. Reaches the SAME
//!   [`crate::projection::ops::dispatch`] code path.
//!
//! Both paths reach the SAME [`crate::projection::ops::dispatch`] code so the
//! behaviour is identical — only the entry door (and the level of detail
//! returned to the caller) differs.
//!
//! # JSON shape — isomorphic with the bespoke envelope
//!
//! The enum is `#[serde(tag = "op", rename_all = "snake_case")]` so the on-
//! the-wire JSON shape is exactly the bespoke envelope the iOS bridge
//! already produces:
//!
//! ```json
//! {"op": "create_group", "name": "engineering", "description": "...", "invitee_text": "...", "signed_key_package_events_json": []}
//! {"op": "send", "group_id_hex": "abc...", "text": "hello"}
//! {"op": "publish_key_package"}
//! ```
//!
//! iOS doesn't re-encode — the ADR-0025 PR 2 migration from the legacy
//! `nmp_marmot_dispatch(json)` symbol to `nmp_app_dispatch_action("nmp.marmot",
//! json)` was a one-line call-site change per op (and PR 3 then deleted
//! the legacy symbol entirely).
//!
//! # `start()` validates shape; `MarmotProtocolCommand` does the work
//!
//! `MarmotActionModule::start` is the validator — it deserializes the
//! action JSON into the typed `MarmotAction` enum and rejects malformed
//! payloads at the boundary. `MarmotActionModule::execute` then emits
//! `ActorCommand::Protocol(Box<MarmotProtocolCommand>)` with the already
//! parsed enum and the live `MarmotProjection`. The command runs on the
//! actor thread, uses [`ProtocolCommandContext`] for actor-authored time and
//! publish/interest commands, and records the terminal action verdict.

use std::sync::Arc;

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionRejection, ProtocolCommand, ProtocolCommandContext,
    ProtocolCommandError,
};
use serde::{Deserialize, Serialize};

use crate::projection::state::{MarmotProjection, MarmotRuntimePort};

/// Namespace under which the [`MarmotActionModule`] registers in the
/// kernel's [`nmp_core::kernel::ActionRegistry`]. Hosts dispatch via
/// `nmp_app_dispatch_action("nmp.marmot", action_json)`.
///
/// Named after the Marmot protocol (the MLS-over-Nostr binding that
/// `nmp-app-marmot` implements), not the `nmp-app-marmot` crate. A second
/// app that drives the same protocol could choose to reuse the namespace
/// (with its own action-module install); the namespace is a wire
/// contract, not an implementation tag.
pub const MARMOT_ACTION_NAMESPACE: &str = "nmp.marmot";

/// Typed Marmot action enum.
///
/// `#[serde(tag = "op", rename_all = "snake_case")]` keeps the on-the-wire
/// JSON byte-identical with the legacy `nmp_marmot_dispatch` envelope
/// (the `{"op": "create_group", ...}` shape iOS already produces). See the
/// module rustdoc for the migration plan.
///
/// `#[serde(deny_unknown_fields)]` is NOT applied here — the legacy
/// envelope tolerates ignored extra fields (e.g. iOS sometimes appends
/// `signed_key_package_events_json: []` to `invite` and `create_group`
/// even when empty), and rejecting them would break the migration.
#[derive(Clone, Debug, Deserialize, Serialize)]
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
        /// cache populated by the raw-event tap.
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
/// projection captured by the typed protocol command — see the module rustdoc.
pub struct MarmotActionModule {
    projection: Arc<MarmotProjection>,
}

impl MarmotActionModule {
    #[must_use]
    pub fn new(projection: Arc<MarmotProjection>) -> Self {
        Self { projection }
    }
}

pub struct MarmotProtocolCommand {
    projection: Arc<MarmotProjection>,
    action: MarmotAction,
    correlation_id: Option<String>,
}

impl std::fmt::Debug for MarmotProtocolCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MarmotProtocolCommand")
            .field("action", &self.action)
            .field("correlation_id", &self.correlation_id)
            .finish_non_exhaustive()
    }
}

impl MarmotProtocolCommand {
    #[must_use]
    pub fn new(
        projection: Arc<MarmotProjection>,
        action: MarmotAction,
        correlation_id: String,
    ) -> Self {
        Self {
            projection,
            action,
            correlation_id: Some(correlation_id),
        }
    }

    #[must_use]
    pub fn new_internal(projection: Arc<MarmotProjection>, action: MarmotAction) -> Self {
        Self {
            projection,
            action,
            correlation_id: None,
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
        let MarmotProtocolCommand {
            projection,
            action,
            correlation_id,
        } = *self;

        if let Some(correlation_id) = correlation_id.as_deref() {
            ctx.record_action_stage_requested(correlation_id);
        }
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

    /// Hand the typed action to [`MarmotProtocolCommand`] on the `Protocol`
    /// arm. The command owns the actor-thread execution and records the
    /// terminal verdict under `correlation_id`.
    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        send(ActorCommand::Protocol(Box::new(
            MarmotProtocolCommand::new(
                Arc::clone(&self.projection),
                action,
                correlation_id.to_string(),
            ),
        )));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_module() -> MarmotActionModule {
        use crate::service::MarmotService;
        use mdk_core::MdkConfig;
        use mdk_sqlite_storage::MdkSqliteStorage;
        use nostr::Keys;

        let storage =
            MdkSqliteStorage::new_in_memory().expect("in-memory MDK storage should construct");
        let service = MarmotService::from_storage(storage, Keys::generate(), MdkConfig::default());
        MarmotActionModule::new(Arc::new(MarmotProjection::new(service, None)))
    }

    /// The typed enum's JSON shape MUST accept the supported host-produced
    /// Marmot action bodies. The raw signed-event tap, not this action seam,
    /// owns inbound event ingest.
    #[test]
    fn host_action_shapes_parse_as_typed_actions() {
        let cases = &[
            r#"{"op":"publish_key_package"}"#,
            r#"{"op":"create_group","name":"engineering","description":"the eng group","invitee_text":"npub1abc npub1def","signed_key_package_events_json":[]}"#,
            r#"{"op":"invite","group_id_hex":"aa00bb11","invitee_text":"npub1ghi","signed_key_package_events_json":[]}"#,
            r#"{"op":"send","group_id_hex":"aa00bb11","text":"hello"}"#,
            r#"{"op":"leave","group_id_hex":"aa00bb11"}"#,
            r#"{"op":"remove","group_id_hex":"aa00bb11","member_npubs":["npub1ghi"]}"#,
            r#"{"op":"accept_welcome","welcome_id_hex":"cc22dd33"}"#,
            r#"{"op":"decline_welcome","welcome_id_hex":"cc22dd33"}"#,
            r#"{"op":"clear_pending","group_id_hex":"aa00bb11"}"#,
        ];
        for json in cases {
            let parsed: MarmotAction = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("typed enum must accept host action `{json}`: {e}"));
            // Re-serializing produces a value that parses back to the same
            // variant — the round-trip is stable. We don't assert
            // byte-equality because serde may reorder fields, but the
            // re-parse witnesses the shape is faithful.
            let reserialized = serde_json::to_string(&parsed).unwrap();
            let _: MarmotAction = serde_json::from_str(&reserialized).unwrap_or_else(|e| {
                panic!("re-serialized envelope must round-trip: {reserialized}: {e}")
            });
        }
    }

    /// The `op` discriminator MUST be snake_case — the same casing the iOS
    /// bridge produces. A bug that flipped this to PascalCase would silently
    /// break every iOS dispatch site after the migration.
    #[test]
    fn op_discriminator_is_snake_case() {
        let action = MarmotAction::PublishKeyPackage { relays: Vec::new() };
        let json = serde_json::to_string(&action).unwrap();
        assert!(
            json.contains(r#""op":"publish_key_package""#),
            "op discriminator must be snake_case, got: {json}"
        );
    }

    /// `MarmotActionModule::execute` MUST emit exactly one typed `Protocol`
    /// command carrying the parsed action and registry-minted `correlation_id`.
    #[test]
    fn execute_emits_one_typed_protocol_command_with_correlation_id() {
        use nmp_core::actor::ActorCommand;
        use std::cell::RefCell;

        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        let action = MarmotAction::Send {
            group_id_hex: "aa00bb11".to_string(),
            text: "hello, group".to_string(),
        };
        test_module()
            .execute(action, "corr-test-id", &|cmd| {
                captured.borrow_mut().push(cmd);
            })
            .expect("execute should not fail for a valid action");

        let cmds = captured.into_inner();
        assert_eq!(cmds.len(), 1, "execute must emit exactly one ActorCommand");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::Protocol(cmd) => {
                // The boxed command is a `dyn ProtocolCommand`; assert its
                // payload through `Debug` (the command prints action + id).
                let dbg = format!("{cmd:?}");
                assert!(
                    dbg.contains("MarmotProtocolCommand"),
                    "expected a MarmotProtocolCommand, got: {dbg}"
                );
                assert!(
                    dbg.contains("corr-test-id"),
                    "must carry the registry-minted correlation_id, got: {dbg}"
                );
                assert!(
                    dbg.contains("Send"),
                    "must carry the typed action body, got: {dbg}"
                );
                assert!(
                    dbg.contains("hello, group"),
                    "must carry the action body, got: {dbg}"
                );
            }
            other => panic!("expected ActorCommand::Protocol, got {other:?}"),
        }
    }

    /// A malformed envelope (unknown `op` value) fails at the registry's
    /// JSON-shape parse step (the adapter calls `serde_json::from_str` into
    /// `Self::Action` before reaching `start`). The serde enum's tagged
    /// representation rejects unknown discriminators.
    #[test]
    fn unknown_op_is_rejected_at_serde_layer() {
        let err = serde_json::from_str::<MarmotAction>(r#"{"op":"nuke_everything"}"#)
            .expect_err("unknown op must be rejected by serde");
        assert!(
            err.to_string().contains("unknown variant")
                || err.to_string().contains("nuke_everything"),
            "expected serde to name the offending variant, got: {err}"
        );
    }
}
