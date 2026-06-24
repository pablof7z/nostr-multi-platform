//! `MarmotProtocolCommand` — the typed [`ProtocolCommand`] that carries an
//! already-parsed [`MarmotAction`] and a clonable handle to the shared
//! [`MarmotProjection`] directly to [`ops::dispatch`].
//!
//! # Why this exists (#1940)
//!
//! Before #1940 a Marmot op round-tripped through THREE indirections:
//! `MarmotActionModule::execute` re-serialized the typed enum to JSON, boxed
//! it into a `nmp_core::substrate::host_op_command`, and dispatched
//! `ActorCommand::Protocol(HostOpCommand)`; that command then reached a
//! per-app `HostOpHandlerSlot`, cloned the installed `MarmotMlsOpHandler`, and
//! that handler `serde_json::from_str`-ed the JSON back into a `MarmotAction`
//! and called `ops::dispatch`.
//!
//! `MarmotProtocolCommand` collapses all three: the typed `MarmotAction`
//! parsed ONCE by the action-registry adapter is carried verbatim to
//! `ops::dispatch`. No JSON re-serialize, no JSON re-parse, no host-op slot.
//! The only serde boundary that remains is the legitimate host-wire-JSON →
//! `MarmotAction` parse every `ActionModule` performs.
//!
//! # Terminal-verdict routing (behaviour-preserving)
//!
//! `run` records `Requested`, dispatches under the projection mutex, then
//! routes the `{"ok"/"pending"/"error"}` envelope to the kernel's
//! `action_stages` mirror via `record_action_success` / `record_action_failure`
//! — the exact flag logic the deleted `HostOpCommand::run` used.
//! `{"pending":true}` records nothing (the deferred path owns the terminal
//! write).
//!
//! # Whole-body panic isolation (ADR-0052 §D4 guarantee #1)
//!
//! The `Protocol` dispatch arm already wraps the whole `run` in `catch_unwind`,
//! but a bare arm-level catch would NOT write a `RecordActionFailure` for the
//! correlation id — the host's spinner would hang. So `run` wraps the
//! `with_inner(ops::dispatch)` call in `catch_unwind` itself and records a
//! precise failure on catch, exactly as `HostOpCommand` did (a
//! behaviour-preservation requirement, not optional).

use std::sync::Arc;

use nostr::{Event, RelayUrl};

use nmp_core::actor::ActorCommand;
use nmp_core::subs::SubIdentity;
use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_planner::LogicalInterest;

use crate::projection::action::MarmotAction;
use crate::projection::host_port::{publish_permitted, MarmotHostPort};
use crate::projection::ops;
use crate::projection::state::MarmotProjection;

/// The typed Marmot write command dispatched as
/// `ActorCommand::Protocol(Box::new(MarmotProtocolCommand::new(..)))`.
///
/// Carries the shared projection (so the command reaches per-app MLS state
/// without a kernel host-op slot — the linchpin #1940 unlock) and the
/// already-parsed action + registry correlation id.
pub struct MarmotProtocolCommand {
    projection: Arc<MarmotProjection>,
    action: MarmotAction,
    correlation_id: String,
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
            correlation_id,
        }
    }
}

// `Debug` is required by the `ProtocolCommand` supertrait. `Arc<MarmotProjection>`
// is not `Debug`, so print only the action + correlation id (matching what the
// old HostOpCommand Debug-based test asserted).
impl std::fmt::Debug for MarmotProtocolCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MarmotProtocolCommand")
            .field("action", &self.action)
            .field("correlation_id", &self.correlation_id)
            .finish()
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

        // Record `Requested` first so the host's spinner sees the action
        // entered the actor lane even if dispatch panics (mirrors the deleted
        // `HostOpCommand` and the V-41 LNURL command).
        ctx.record_action_stage_requested(&correlation_id);

        // D9: the dispatch clock comes from the kernel `KernelClock` seam — the
        // primary #1940 fix (replaces the deleted `MarmotMlsOpHandler`'s direct
        // `SystemTime::now()` read).
        let now_secs = ctx.now_secs();
        let port = ContextHostPort::new(ctx);

        // Guarantee #1 — wrap the SQLite/MDK-bound dispatch in `catch_unwind`
        // so a panic still yields a precise terminal verdict for this
        // correlation id (the arm-level catch alone would not record one).
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            projection
                .with_inner(|h| ops::dispatch(h, &action, now_secs, Some(&correlation_id), &port))
                .unwrap_or_else(|| {
                    serde_json::json!({
                        "ok": false,
                        "error": "MarmotProtocolCommand: projection mutex poisoned",
                    })
                })
        }))
        .unwrap_or_else(|_| {
            serde_json::json!({
                "ok": false,
                "error": "marmot op panicked",
            })
        });

        // Route the envelope through the single terminal-recording path
        // (`RecordAction*`). `{"pending":true}` leaves the action in
        // `Requested` and records nothing — the deferred continuation owns the
        // later terminal write (D8 callback-driven). Otherwise `{"ok":true}`
        // records success and anything else records failure.
        let flag = |k| {
            result
                .get(k)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        };
        if flag("pending") {
            // Deferred path owns the terminal write; nothing to record now.
        } else if flag("ok") {
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

/// A [`MarmotHostPort`] over a `nmp_core::substrate::ProtocolCommandContext`.
///
/// Bridges the kernel's typed capability seams to the crate-local port trait
/// so `ops` stays decoupled from the `ProtocolCommandContext` type. Publish /
/// write-relay / interest all route through the context's typed methods; the
/// command path never names `NmpApp`.
pub(crate) struct ContextHostPort<'a, 'b> {
    ctx: &'a ProtocolCommandContext<'b>,
}

impl<'a, 'b> ContextHostPort<'a, 'b> {
    #[must_use]
    pub(crate) fn new(ctx: &'a ProtocolCommandContext<'b>) -> Self {
        Self { ctx }
    }
}

impl<'a, 'b> MarmotHostPort for ContextHostPort<'a, 'b> {
    fn publish_signed_explicit(&self, event: &Event, relays: &[RelayUrl]) {
        if !publish_permitted(event, relays) {
            return;
        }
        let relays: Vec<nmp_core::publish::RelayUrl> =
            relays.iter().map(std::string::ToString::to_string).collect();
        self.ctx.publish_signed_explicit(event, relays);
    }

    fn write_relay_urls(&self) -> Vec<String> {
        self.ctx.write_relay_urls()
    }

    fn ensure_interest(&self, identity: SubIdentity, interest: LogicalInterest) {
        self.ctx.ensure_interest(identity, interest);
    }

    fn send_actor_command(&self, cmd: ActorCommand) {
        self.ctx.send(cmd);
    }
}
