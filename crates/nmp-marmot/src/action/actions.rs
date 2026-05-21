//! 7 `ActionModule` impls per `docs/plan/marmot-mls.md` §Step 2 + mdk-api.md
//! §6: `PublishKeyPackage`, `CreateGroup`, `InviteMember`, `SendMessage`,
//! `LeaveGroup`, `RemoveMember`, `UpdateKeys`.
//!
//! These mirror `nmp-nip29::action`'s Step-0 pattern exactly: the trait impls
//! are intentionally thin. `ActionModule::start` has only `&mut ActionContext`
//! — no `nostr::Keys`, no MDK handle — so it cannot drive MLS. The real
//! MDK-driving logic lives in [`crate::service::MarmotService`], which the
//! actor invokes (and which the in-crate round-trip tests exercise). These
//! ActionModule impls satisfy registry wiring + the kernel-boundary grep
//! (they import ZERO MDK / openmls types).
//!
//! Each action carries the typed group identity (`group_id_hex` +
//! `group_relay_url`) so the publish planner gets the relay pin from the
//! [`PublishPlan`] carrier and never derives routing from raw tags.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use serde::{Deserialize, Serialize};

use super::publish_plan::PublishPlan;
use crate::interest::{KIND_GROUP_MESSAGE, KIND_KEY_PACKAGE};

/// Group-scoped action input: the typed group identity drives the relay pin.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GroupActionInput {
    pub group_id_hex: String,
    pub group_relay_url: String,
    /// Action-specific free-form fields (target pubkeys, message body, …).
    #[serde(default)]
    pub fields: MarmotActionFields,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct MarmotActionFields {
    /// Pubkeys to invite / remove (hex).
    #[serde(default)]
    pub target_pubkeys: Vec<String>,
    /// Plaintext message body for `SendMessage`.
    pub message: Option<String>,
}

/// A group-scoped action that emits a relay-pinned kind:445 `PublishPlan`.
macro_rules! pinned_group_action {
    ($Module:ident, $ns:literal) => {
        pub struct $Module;
        impl ActionModule for $Module {
            const NAMESPACE: &'static str = $ns;
            type Action = GroupActionInput;
            fn start(
                _ctx: &mut ActionContext,
                action: Self::Action,
            ) -> Result<(), ActionRejection> {
                if action.group_relay_url.is_empty() {
                    return Err(ActionRejection::Invalid(
                        "missing group relay url for group event".into(),
                    ));
                }
                // The signed kind:445 ciphertext is produced by MDK via
                // crate::service; here we validate the pinned-plan shape so
                // the privacy guard (MissingGroupRelayPin) holds before the
                // signer-bridge runs.
                let plan = PublishPlan::pinned(
                    action.group_relay_url.clone(),
                    action.group_id_hex.clone(),
                    KIND_GROUP_MESSAGE,
                    String::new(),
                    Vec::new(),
                );
                if plan.validate_group_event_pinned().is_err() {
                    return Err(ActionRejection::Invalid(
                        "missing group relay pin for kind:445 event".into(),
                    ));
                }
                Ok(())
            }
        }
    };
}

// kind:445 group events — relay-pinned.
pinned_group_action!(CreateGroupAction, "marmot.create_group");
pinned_group_action!(InviteMemberAction, "marmot.invite_member");
pinned_group_action!(SendMessageAction, "marmot.send_message");
pinned_group_action!(LeaveGroupAction, "marmot.leave_group");
pinned_group_action!(RemoveMemberAction, "marmot.remove_member");
pinned_group_action!(UpdateKeysAction, "marmot.update_keys");

// ─── PublishKeyPackage (kind:30443/443, standard author-write outbox) ─────────

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PublishKeyPackageInput {
    /// Relays to advertise in the KeyPackage (the owner's write relays).
    pub relays: Vec<String>,
}

pub struct PublishKeyPackageAction;
impl ActionModule for PublishKeyPackageAction {
    const NAMESPACE: &'static str = "marmot.publish_key_package";
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
        // KeyPackage uses standard author-write outbox (NOT relay-pinned);
        // the real content+tags are produced by MDK via crate::service.
        let plan = PublishPlan::outbox(KIND_KEY_PACKAGE, String::new(), Vec::new());
        debug_assert!(plan.validate_group_event_pinned().is_ok());
        Ok(())
    }
}
