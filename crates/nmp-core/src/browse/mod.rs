//! Single-relay browsing action module — V-52.
//!
//! Exposes the `nmp.browse_relay` `ActionModule` namespace so a host can
//! subscribe or browse events scoped to ONE specific relay without touching
//! NIP-65 mailbox routing, outbox fan-out, or the actor command enum directly.
//!
//! ## Design choice — reuse `relay_pin`, not a new routing field
//!
//! `InterestShape::relay_pin` (planner `case_e_relay_pinned`) already enforces
//! the "exactly one relay, no NIP-65 fan-out" contract — see
//! `docs/architecture/crate-boundaries.md` §3.1 and Rule 9 of the merge
//! lattice. Rather than adding a parallel `scope_relays` field (fragmentation),
//! this module builds a `LogicalInterest` with `relay_pin = Some(url)` and
//! dispatches `ActorCommand::PushInterest`.  No `actor/mod.rs` changes needed.
//!
//! ## Doctrine
//! - D0: substrate-pure types only (RelayUrl = String, kinds = Vec<u32>).
//! - D3: relay selection is the caller's intent (`relay_pin`), not the router's
//!   automatic algorithm — this is the explicit opt-out path.
//! - D8: no polling; the registered interest fires reactive events when
//!   matching events arrive through the relay worker.
//!
//! ## Action variants
//!
//! - [`BrowseRelayAction::Open`] — register a relay-pinned `LogicalInterest`.
//! - [`BrowseRelayAction::Close`] — withdraw it by interest id.
//!
//! Both are synchronous-completing (default `is_async_completing() = false`).
//!
//! ## Wire JSON example
//! ```json
//! {
//!   "Open": {
//!     "relay_url": "wss://relay.damus.io",
//!     "kinds": [1],
//!     "lifecycle": "tailing",
//!     "interest_id": 9001
//!   }
//! }
//! ```
//!
//! ```json
//! { "Close": { "interest_id": 9001 } }
//! ```

use serde::{Deserialize, Serialize};

use crate::actor::ActorCommand;
use crate::planner::{
    InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest,
};
use crate::relay::CanonicalRelayUrl;
use crate::substrate::{ActionContext, ActionModule, ActionRejection};

/// V-52: relay browsing action — `nmp.browse_relay` namespace.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum BrowseRelayAction {
    /// Register a relay-pinned subscription for the given relay URL and kind set.
    ///
    /// `interest_id` is the caller-supplied stable identifier used to de-duplicate
    /// re-registrations and to address the `Close` command. The registry replaces
    /// an existing entry with the same id — idempotent on re-open.
    ///
    /// `kinds` is the event kind set to subscribe to. Empty vec = wildcard (any kind),
    /// which is rarely the right choice for a browse subscription; callers should
    /// supply an explicit kind set.
    ///
    /// `lifecycle` controls whether the REQ closes on EOSE (`"one_shot"`) or stays
    /// open for live events (`"tailing"`). Defaults to `"tailing"` if absent.
    Open {
        relay_url: String,
        kinds: Vec<u32>,
        #[serde(default = "default_lifecycle")]
        lifecycle: BrowseLifecycle,
        interest_id: u64,
    },
    /// Withdraw the relay-pinned subscription registered under `interest_id`.
    ///
    /// Triggers a plan recompile that removes the corresponding REQ from the relay.
    /// A no-op if the id was never registered or was already withdrawn.
    Close { interest_id: u64 },
}

/// Lifecycle for a browse subscription.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub enum BrowseLifecycle {
    /// Stay open after EOSE (live subscription). Default.
    #[default]
    #[serde(rename = "tailing")]
    Tailing,
    /// Close on EOSE.
    #[serde(rename = "one_shot")]
    OneShot,
}

fn default_lifecycle() -> BrowseLifecycle {
    BrowseLifecycle::Tailing
}

/// `ActionModule` impl for the `nmp.browse_relay` namespace.
pub struct BrowseRelayModule;

impl ActionModule for BrowseRelayModule {
    const NAMESPACE: &'static str = "nmp.browse_relay";

    type Action = BrowseRelayAction;

    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        match &action {
            BrowseRelayAction::Open {
                relay_url,
                interest_id,
                ..
            } => {
                // Relay URL must be a valid ws:// or wss:// URL.
                if CanonicalRelayUrl::parse(relay_url).is_none() {
                    return Err(ActionRejection::Invalid(format!(
                        "browse_relay: '{relay_url}' is not a valid ws:// or wss:// relay URL"
                    )));
                }
                // interest_id = 0 is the sentinel "unassigned" value in the planner.
                // Callers must supply a non-zero id so the registry can address it.
                if *interest_id == 0 {
                    return Err(ActionRejection::Invalid(
                        "browse_relay: interest_id must be non-zero".to_string(),
                    ));
                }
                Ok(())
            }
            BrowseRelayAction::Close { .. } => Ok(()),
        }
    }

    fn execute(
        &self,
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        match action {
            BrowseRelayAction::Open {
                relay_url,
                kinds,
                lifecycle,
                interest_id,
            } => {
                let lc = match lifecycle {
                    BrowseLifecycle::Tailing => InterestLifecycle::Tailing,
                    BrowseLifecycle::OneShot => InterestLifecycle::OneShot,
                };
                let kinds_set = kinds.into_iter().collect();
                let shape = InterestShape {
                    kinds: kinds_set,
                    relay_pin: Some(relay_url),
                    ..Default::default()
                };
                let interest = LogicalInterest {
                    id: InterestId(interest_id),
                    scope: InterestScope::Global,
                    shape,
                    hints: Vec::new(),
                    lifecycle: lc,
                    is_indexer_discovery: false,
                };
                send(ActorCommand::PushInterest(interest));
                Ok(())
            }
            BrowseRelayAction::Close { interest_id } => {
                send(ActorCommand::WithdrawInterest(InterestId(interest_id)));
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests;
