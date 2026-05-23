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
//!   `nmp_app_dispatch_action("nmp.marmot", action_json)`; the actor's
//!   `set_mls_op_handler`-installed
//!   [`crate::projection::handler::MarmotMlsOpHandler`] runs the op
//!   against the live `MarmotProjection`. Returns a `correlation_id`
//!   synchronously; the terminal verdict surfaces on `action_stages`.
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
//! iOS doesn't need to re-encode — flipping the dispatch call from
//! `nmp_marmot_dispatch(json)` to `nmp_app_dispatch_action("nmp.marmot",
//! json)` is a one-line change at every call site.
//!
//! # `start()` validates shape; the handler does the work
//!
//! `MarmotActionModule::start` is the validator — it deserializes the
//! action JSON into the typed `MarmotAction` enum and rejects malformed
//! payloads at the boundary. `MarmotActionModule::execute` then re-serializes
//! the typed enum and emits `ActorCommand::DispatchMlsOp { action_json,
//! correlation_id }`. The actor's `DispatchMlsOp` arm pulls the host-installed
//! [`MarmotMlsOpHandler`](crate::projection::handler::MarmotMlsOpHandler)
//! out of the slot and runs the op against the live `MarmotProjection`.
//!
//! Why re-serialize an already-parsed enum? Because `MlsOpHandler::handle`
//! takes `&str` (D0 — `nmp-core` cannot name `MarmotAction`); the typed enum
//! provides the validation gate without coupling the kernel to the app's
//! noun. The serde round-trip is sub-microsecond and only happens once per
//! dispatch — irrelevant next to the actual MLS / SQLite work.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

/// Namespace under which the [`MarmotActionModule`] registers in the
/// kernel's [`nmp_core::kernel::ActionRegistry`]. Hosts dispatch via
/// `nmp_app_dispatch_action("nmp.marmot", action_json)`.
///
/// Named after the Marmot protocol (the MLS-over-Nostr binding that
/// `nmp-app-marmot` implements), not the `nmp-app-marmot` crate. A second
/// app that drives the same protocol could choose to reuse the namespace
/// (with its own `MarmotMlsOpHandler` install); the namespace is a wire
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
    /// Publish (or rotate) the local MLS key-package — kind:30443 + legacy
    /// kind:443 dual publish.
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
        /// Optional pre-fetched signed kind:30443 / kind:443 key-package
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
    Send {
        group_id_hex: String,
        text: String,
    },
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
    /// Manual ingest of a signed inbound event (back-compat alias over the
    /// raw-event tap's automatic ingest path — used by REPL / tests).
    IngestSignedEvent { event_json: String },
    /// Explicit pending-commit clear (mdk-api.md §7.7) — exposed so a
    /// caller that detected a relay-publish failure can unwedge the group.
    ClearPending { group_id_hex: String },
}

/// The substrate-generic `ActionModule` registered under
/// [`MARMOT_ACTION_NAMESPACE`].
///
/// Mirrors the shape of every other `ActionModule` in the workspace
/// (PublishModule, ChirpReactModule, etc.): `start()` validates the typed
/// action; `execute()` emits one `ActorCommand` carrying everything the
/// actor needs to run the op. The only Marmot-specific piece is the
/// handler the actor's `DispatchMlsOp` arm reaches through the host-
/// installed slot — see the module rustdoc.
pub struct MarmotActionModule;

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
        _ctx: &mut ActionContext,
        _action: Self::Action,
    ) -> Result<(), ActionRejection> {
        Ok(())
    }

    /// Mark the module as asynchronously-completing so the kernel's
    /// `action_stages` mirror is exercised end-to-end:
    ///
    /// * the registry mints a `correlation_id` and returns it to the host;
    /// * the actor's `DispatchMlsOp` arm records `Requested` → terminal
    ///   (`Accepted` on `ok:true`, `Failed` on `ok:false`) under that id;
    /// * the host's spinner clears on the next snapshot tick.
    ///
    /// Returning `false` here would skip the `action_stages` mirror writes
    /// and the host would never see a terminal verdict for a Marmot op.
    fn is_async_completing() -> bool { // doctrine-allow: D12 — stage transitions are recorded by the `DispatchMlsOp` arm in `nmp-core/src/actor/dispatch.rs`, not here; this is the seam declaration so the registry routes the verdict.
        true
    }

    /// Re-serialize the typed action and hand it to the actor's
    /// `DispatchMlsOp` arm. The matching handler
    /// ([`crate::projection::handler::MarmotMlsOpHandler`]) installed via
    /// [`nmp_core::NmpApp::set_mls_op_handler`] parses the JSON back out
    /// and runs the op against the live `MarmotProjection`.
    ///
    /// `serde_json::to_string` cannot fail for a value the registry
    /// already deserialized successfully — but D6 demands we still treat
    /// it as a fallible point: an unexpected `Err` becomes the executor's
    /// `Err` return, and the `dispatch_action` envelope surfaces it as
    /// `{"correlation_id":...,"error":...}` (the same post-mint failure
    /// path `PublishModule` uses).
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let action_json = serde_json::to_string(&action)
            .map_err(|e| format!("failed to re-serialize MarmotAction: {e}"))?;
        send(ActorCommand::DispatchMlsOp {
            action_json,
            correlation_id: correlation_id.to_string(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The typed enum's JSON shape MUST stay byte-identical with the
    /// bespoke envelope so iOS can flip its dispatch call without
    /// re-encoding the action body. The full set of variants the iOS
    /// bridge produces is exercised here.
    #[test]
    fn ios_legacy_envelope_round_trips() {
        let cases = &[
            r#"{"op":"publish_key_package"}"#,
            r#"{"op":"create_group","name":"engineering","description":"the eng group","invitee_text":"npub1abc npub1def","signed_key_package_events_json":[]}"#,
            r#"{"op":"invite","group_id_hex":"aa00bb11","invitee_text":"npub1ghi","signed_key_package_events_json":[]}"#,
            r#"{"op":"send","group_id_hex":"aa00bb11","text":"hello"}"#,
            r#"{"op":"leave","group_id_hex":"aa00bb11"}"#,
            r#"{"op":"remove","group_id_hex":"aa00bb11","member_npubs":["npub1ghi"]}"#,
            r#"{"op":"accept_welcome","welcome_id_hex":"cc22dd33"}"#,
            r#"{"op":"decline_welcome","welcome_id_hex":"cc22dd33"}"#,
            r#"{"op":"ingest_signed_event","event_json":"{}"}"#,
            r#"{"op":"clear_pending","group_id_hex":"aa00bb11"}"#,
        ];
        for json in cases {
            let parsed: MarmotAction = serde_json::from_str(json).unwrap_or_else(|e| {
                panic!("typed enum must accept legacy envelope `{json}`: {e}")
            });
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

    /// `MarmotActionModule::execute` MUST emit exactly one `DispatchMlsOp`
    /// command carrying the registry-minted `correlation_id` and the
    /// re-serialized action JSON. Mirrors the
    /// `host_registered_executor_dispatches_successfully` shape in
    /// `nmp-core::ffi::action::tests`.
    #[test]
    fn execute_emits_one_dispatch_mls_op_command_with_correlation_id() {
        use nmp_core::ActorCommand;
        use std::cell::RefCell;

        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        let action = MarmotAction::Send {
            group_id_hex: "aa00bb11".to_string(),
            text: "hello, group".to_string(),
        };
        MarmotActionModule::execute(action, "corr-test-id", &|cmd| {
            captured.borrow_mut().push(cmd);
        })
        .expect("execute should not fail for a valid action");

        let cmds = captured.into_inner();
        assert_eq!(cmds.len(), 1, "execute must emit exactly one ActorCommand");
        match cmds.into_iter().next().unwrap() {
            ActorCommand::DispatchMlsOp {
                action_json,
                correlation_id,
            } => {
                assert_eq!(correlation_id, "corr-test-id");
                let parsed: serde_json::Value = serde_json::from_str(&action_json).unwrap();
                assert_eq!(
                    parsed.get("op").and_then(|v| v.as_str()),
                    Some("send"),
                );
                assert_eq!(
                    parsed.get("text").and_then(|v| v.as_str()),
                    Some("hello, group"),
                );
            }
            other => panic!("expected DispatchMlsOp, got {other:?}"),
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
            err.to_string().contains("unknown variant") || err.to_string().contains("nuke_everything"),
            "expected serde to name the offending variant, got: {err}"
        );
    }
}
