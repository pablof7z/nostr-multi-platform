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
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        validate_relay_url(&action.relay_url)?;
        let interest = relay_discovery_interest(&action.relay_url);
        send(ActorCommand::PushInterest(interest));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Run the typed executor and capture the single `ActorCommand` it sends.
    fn run_execute(input: DiscoverGroupsInput) -> Result<ActorCommand, String> {
        let captured: RefCell<Option<ActorCommand>> = RefCell::new(None);
        DiscoverGroupsAction::execute(input, "test-cid", &|cmd| {
            *captured.borrow_mut() = Some(cmd);
        })?;
        captured
            .into_inner()
            .ok_or_else(|| "executor sent no command".to_string())
    }

    #[test]
    fn well_formed_input_yields_push_interest_command() {
        let input = DiscoverGroupsInput {
            relay_url: "wss://groups.example.com".to_string(),
        };
        match run_execute(input).expect("well-formed input executes") {
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
    }

    #[test]
    fn empty_relay_url_is_rejected() {
        assert!(run_execute(DiscoverGroupsInput {
            relay_url: String::new(),
        })
        .is_err());
    }

    #[test]
    fn non_websocket_scheme_is_rejected() {
        assert!(run_execute(DiscoverGroupsInput {
            relay_url: "https://groups.example.com".to_string(),
        })
        .is_err());
    }
}
