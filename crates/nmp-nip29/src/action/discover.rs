//! Group-discovery action: open a NIP-29 metadata subscription against a
//! relay so its kind:39000/39001/39002 catalog streams in.
//!
//! Unlike the user-content actions in `content.rs` / `composed.rs`, this
//! action does **not** publish an event — there is no user-authored payload
//! to sign. Instead it pushes a host-pinned [`LogicalInterest`] for the
//! three metadata kinds, scoped to one host relay. The companion
//! [`crate::projection::DiscoveredGroupsProjection`] is the read-side: as
//! events arrive on the pinned interest's REQ, the projection accumulates
//! them into a flat list of [`DiscoveredGroup`](crate::projection::DiscoveredGroup)
//! rows.
//!
//! The action returns an [`ActorCommand::PushInterest`]; the `InterestId`
//! is derived deterministically from the relay URL by
//! [`crate::interest::relay_discovery_interest`], so a re-dispatch for the
//! same relay is idempotent at the kernel level (same id replaces).

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

use crate::interest::relay_discovery_interest;

/// Action input — the relay to discover groups on.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DiscoverGroupsInput {
    /// The host relay URL (`wss://…`) whose group catalog to subscribe to.
    pub relay_url: String,
}

/// Reject empty or non-websocket-scheme URLs. The kernel's relay planner
/// will tolerate weird shapes (it just opens whatever it's handed), so the
/// gate lives here.
fn validate_relay_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("relay_url is empty".to_string());
    }
    if !(url.starts_with("wss://") || url.starts_with("ws://")) {
        return Err("relay_url must start with wss:// or ws://".to_string());
    }
    Ok(())
}

pub struct DiscoverGroupsAction;
impl ActionModule for DiscoverGroupsAction {
    const NAMESPACE: &'static str = "nmp.nip29.discover";
    type Action = DiscoverGroupsInput;
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        validate_relay_url(&action.relay_url)
            .map_err(ActionRejection::Invalid)?;
        Ok(())
    }
    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let interest = relay_discovery_interest(&action.relay_url);
        send(ActorCommand::PushInterest(interest));
        // `discover_groups` is a subscription-only action: there is no event
        // published and no async worker, so the "success" surface is instantaneous
        // (the interest has been pushed to the lifecycle). Without a terminal
        // `RecordActionSuccess` the host's `dispatch_action` spinner waits forever
        // on `action_results`. Mirror the NIP-57 zap worker's success leg.
        send(ActorCommand::RecordActionSuccess {
            correlation_id: correlation_id.to_string(),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Run the typed executor and capture every `ActorCommand` it sends.
    fn run_execute(input: DiscoverGroupsInput) -> Result<Vec<ActorCommand>, String> {
        let captured: RefCell<Vec<ActorCommand>> = RefCell::new(Vec::new());
        DiscoverGroupsAction::execute(input, "test-cid", &|cmd| {
            captured.borrow_mut().push(cmd);
        })?;
        Ok(captured.into_inner())
    }

    #[test]
    fn well_formed_input_yields_push_interest_then_record_success() {
        let input = DiscoverGroupsInput {
            relay_url: "wss://groups.example.com".to_string(),
        };
        let cmds = run_execute(input).expect("well-formed input executes");
        assert_eq!(
            cmds.len(),
            2,
            "expected PushInterest followed by RecordActionSuccess, got {cmds:?}"
        );
        match &cmds[0] {
            ActorCommand::PushInterest(interest) => {
                assert_eq!(
                    interest.shape.relay_pin.as_deref(),
                    Some("wss://groups.example.com")
                );
                // The three metadata kinds must be on the interest.
                assert!(interest.shape.kinds.contains(&39000));
                assert!(interest.shape.kinds.contains(&39001));
                assert!(interest.shape.kinds.contains(&39002));
            }
            other => panic!("expected PushInterest, got {other:?}"),
        }
        // Terminal `Accepted` stage is what closes the host spinner.
        match &cmds[1] {
            ActorCommand::RecordActionSuccess { correlation_id } => {
                assert_eq!(correlation_id, "test-cid");
            }
            other => panic!("expected RecordActionSuccess, got {other:?}"),
        }
    }

    #[test]
    fn empty_relay_url_is_rejected_in_start() {
        let mut ctx = ActionContext::default();
        assert!(matches!(
            DiscoverGroupsAction::start(
                &mut ctx,
                DiscoverGroupsInput { relay_url: String::new() },
            ),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn non_websocket_scheme_is_rejected_in_start() {
        let mut ctx = ActionContext::default();
        assert!(matches!(
            DiscoverGroupsAction::start(
                &mut ctx,
                DiscoverGroupsInput {
                    relay_url: "https://groups.example.com".to_string(),
                },
            ),
            Err(ActionRejection::Invalid(_))
        ));
    }
}
