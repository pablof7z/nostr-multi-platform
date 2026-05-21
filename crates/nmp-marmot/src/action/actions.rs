use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

// ─── PublishKeyPackage (kind:30443/443, standard author-write outbox) ─────────
//
// Per ADR-0025 Constraint #1, Marmot capabilities without handle-scoped MLS
// state MUST be routed through `dispatch_action`. This ActionModule satisfies
// that constraint: it validates relay coverage before the actor hands off to
// `service::MarmotService::publish_key_package`. The signed event content and
// tags are produced by MDK; only the relay-list precondition lives here.
//
// Group-scoped ActionModules (`CreateGroup`, `InviteMember`, `SendMessage`,
// `LeaveGroup`, `RemoveMember`, `UpdateKeys`) were removed — they required
// handle-scoped MLS state, placing them under the ADR-0025 exception. Until
// a state-transport mechanism exists for those ops, they are covered
// exclusively by the bespoke `nmp_app_chirp_marmot_dispatch` C cluster.

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PublishKeyPackageInput {
    /// Relays to advertise in the KeyPackage (the owner's write relays).
    pub relays: Vec<String>,
}

pub struct PublishKeyPackageAction;
impl ActionModule for PublishKeyPackageAction {
    const NAMESPACE: &'static str = "nmp.marmot.publish_key_package";
    type Action = PublishKeyPackageInput;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        if action.relays.is_empty() {
            return Err(ActionRejection::Invalid(
                "key package must advertise at least one relay".into(),
            ));
        }
        Ok(())
    }

    /// ADR-0027 — Marmot's `PublishKeyPackage` is currently not registered
    /// against any [`nmp_core::kernel::ActionRegistry`]; the call site lives
    /// in [`crate::service::MarmotService::publish_key_package`], reached
    /// through the bespoke `nmp_app_chirp_marmot_dispatch` FFI cluster
    /// (ADR-0025). The trait now requires `execute`, so this body returns
    /// a documented `Err` — if a future change ever routes this namespace
    /// through `dispatch_action`, replace this with the typed
    /// `ActorCommand` enqueue. The Err is D6: a never-reached path that
    /// fails loudly if someone wires it up without finishing the work.
    fn execute(
        _action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        Err("nmp.marmot.publish_key_package is routed through the \
             nmp_app_chirp_marmot_dispatch FFI cluster (ADR-0025), \
             not the generic dispatch_action seam"
            .to_string())
    }
}
