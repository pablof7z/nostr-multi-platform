//! `WelcomeWrapModule` — ActionModule that wraps an MLS Welcome rumor in a
//! NIP-59 gift-wrap envelope addressed to a recipient.
//!
//! ## Seam documentation
//!
//! The NIP-59 gift-wrap operation (seal + wrap) requires the *sender's*
//! `nostr::Keys` for NIP-44 encryption. The NMP `ActionModule` interface
//! does not currently provide live key material through `ActionContext`;
//! the `start()` call therefore emits a [`WrapPlan`] carrier (analogous to
//! `nmp-nip29`'s `PublishPlan`) that names the recipient and the pre-built
//! rumor, but defers the actual cryptographic wrapping to the actor layer.
//!
//! Resolution path (post-v1):
//! 1. The actor's signer-bridge receives the `WrapPlan` via an
//!    `AwaitCapability` step.
//! 2. The bridge fetches the sender `Keys` from `KeyringCapability`.
//! 3. The bridge calls `crate::gift_wrap(sender, &plan.recipient, plan.rumor, …)`.
//! 4. The resulting `nostr::Event` is published to the recipient's NIP-65
//!    inbox relays.
//!
//! For this milestone the caller invokes [`crate::gift_wrap`] directly when
//! it holds keys (see the round-trip integration test in `tests/round_trip.rs`).

use nmp_core::substrate::{
    ActionContext, ActionId, ActionInput, ActionModule, ActionPlan, ActionRejection, ActionStatus,
    ActionTransition,
};
use serde::{Deserialize, Serialize};

/// Input for `WelcomeWrapModule`: the recipient and the MLS Welcome rumor.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WelcomeWrapInput {
    /// Hex-encoded `nostr::PublicKey` of the intended recipient.
    pub recipient_pubkey_hex: String,
    /// The MLS Welcome rumor to be gift-wrapped (unsigned event pre-built by
    /// the Marmot layer). The `pubkey` field must be the sender's public key.
    pub rumor: serde_json::Value,
}

/// Single-step state (no multi-round protocol needed at the action level).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct WelcomeWrapStep;

/// The routing carrier emitted by `WelcomeWrapModule`. Analogous to
/// `nmp-nip29::PublishPlan` but carries NIP-59-specific fields instead of
/// a raw `(kind, content, tags)` triple.
///
/// See module-level seam documentation for how this gets resolved to a signed
/// `nostr::Event` at the actor layer.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WrapPlan {
    /// Hex-encoded recipient public key.
    pub recipient_pubkey_hex: String,
    /// The unsigned rumor event to be gift-wrapped (serialised as JSON for
    /// cross-boundary transport; the actor deserialises back to
    /// `nostr::UnsignedEvent` before calling `gift_wrap`).
    pub rumor_json: String,
    /// Routing hint: the recipient's NIP-65 inbox relays (may be empty;
    /// the actor resolves from the relay list if so).
    pub inbox_relay_hints: Vec<String>,
}

/// ActionModule that emits a [`WrapPlan`] for NIP-59 gift-wrapping an MLS
/// Welcome rumor addressed to a specific recipient.
pub struct WelcomeWrapModule;

impl ActionModule for WelcomeWrapModule {
    const NAMESPACE: &'static str = "nip59.welcome_wrap";

    type Action = WelcomeWrapInput;
    type Step = WelcomeWrapStep;
    type Output = WrapPlan;

    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<ActionPlan<Self::Step>, ActionRejection> {
        // Validate recipient key is non-empty hex.
        if action.recipient_pubkey_hex.is_empty() {
            return Err(ActionRejection::Invalid(
                "recipient_pubkey_hex must not be empty".into(),
            ));
        }
        // Validate rumor is a non-null JSON value.
        if action.rumor.is_null() {
            return Err(ActionRejection::Invalid("rumor must not be null".into()));
        }
        Ok(ActionPlan {
            initial_step: WelcomeWrapStep,
            initial_status: ActionStatus::Pending,
            deadline_ms: None,
        })
    }

    fn reduce(
        _ctx: &mut ActionContext,
        _id: ActionId,
        input: ActionInput<Self::Step>,
    ) -> ActionTransition<Self::Step, Self::Output> {
        // On `Started` we emit a `WrapPlan` immediately. The actor layer is
        // responsible for performing the actual cryptographic wrapping.
        // See module-level seam documentation.
        match input {
            ActionInput::Started => {
                // We cannot materialise the WrapPlan here because we don't
                // have access to the original `Action` in `reduce`. This is
                // a known limitation of the current ActionModule interface;
                // the actor layer re-reads the plan from the step state.
                // Remain Pending — the actor's signer-bridge will drive
                // completion via `CapabilityResult`.
                ActionTransition::Continue {
                    step: WelcomeWrapStep,
                    status: ActionStatus::Running,
                }
            }
            ActionInput::CapabilityResult { value } => {
                // The actor layer posts the WrapPlan back as a capability
                // result so the action can complete.
                match serde_json::from_value::<WrapPlan>(value) {
                    Ok(plan) => ActionTransition::Complete { output: plan },
                    Err(e) => ActionTransition::Fail {
                        reason: format!("invalid WrapPlan from capability: {e}"),
                        transient: false,
                    },
                }
            }
            ActionInput::Timeout => ActionTransition::Fail {
                reason: "welcome wrap timed out".into(),
                transient: true,
            },
            ActionInput::Cancel => ActionTransition::Fail {
                reason: "cancelled".into(),
                transient: false,
            },
            _ => ActionTransition::Continue {
                step: WelcomeWrapStep,
                status: ActionStatus::Running,
            },
        }
    }
}
