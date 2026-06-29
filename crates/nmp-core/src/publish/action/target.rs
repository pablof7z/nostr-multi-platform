use serde::{Deserialize, Serialize};

use crate::publish::action::RelayUrl;
use crate::relay::CanonicalRelayUrl;

/// Why a publish bypasses default outbox planning.
///
/// D3 still makes `Auto` the default. This enum exists only for explicit relay
/// pins so the write stack can distinguish manual overrides from protocol-owned
/// routing such as NIP-29 group hosts or verified DM inboxes.
#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishRouteClass {
    ManualOverride,
    GroupHostPin,
    VerifiedPrivateInbox,
    ImportedOrPresigned,
    Diagnostic,
}

impl PublishRouteClass {
    #[must_use]
    pub fn wire_token(self) -> &'static str {
        match self {
            Self::ManualOverride => "manual_override",
            Self::GroupHostPin => "group_host_pin",
            Self::VerifiedPrivateInbox => "verified_private_inbox",
            Self::ImportedOrPresigned => "imported_or_presigned",
            Self::Diagnostic => "diagnostic",
        }
    }

    #[must_use]
    pub fn from_wire_token(token: &str) -> Option<Self> {
        Some(match token {
            "manual_override" => Self::ManualOverride,
            "group_host_pin" => Self::GroupHostPin,
            "verified_private_inbox" => Self::VerifiedPrivateInbox,
            "imported_or_presigned" => Self::ImportedOrPresigned,
            "diagnostic" => Self::Diagnostic,
            _ => return None,
        })
    }
}

/// Where a publish should go.
///
/// `Auto` defers to the `OutboxResolver` (NIP-65 + indexer fallback per D3).
/// `Explicit` is the named opt-out and must carry both relays and provenance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PublishTarget {
    Auto,
    Explicit {
        relays: Vec<RelayUrl>,
        route_class: PublishRouteClass,
    },
}

impl PublishTarget {
    #[must_use]
    pub fn explicit(relays: Vec<RelayUrl>, route_class: PublishRouteClass) -> Self {
        Self::Explicit {
            relays,
            route_class,
        }
    }
}

/// `Auto` is the unambiguous default — the kernel resolves via NIP-65 (D3).
/// `Explicit` requires deliberate caller intent (a relay set), so it would
/// never make sense as a default. Needed by `#[serde(default)]` on
/// `PublishAction::PublishRaw::target` so a host JSON payload that omits
/// the field gets outbox routing rather than a deserialize error.
impl Default for PublishTarget {
    fn default() -> Self {
        Self::Auto
    }
}

/// Validate a publish target before it can cross the action/actor boundary.
///
/// `Auto` is always valid: it deliberately asks the kernel to resolve via
/// NIP-65. `Explicit` is fail-closed: an empty or malformed relay set is a
/// caller bug, not a request to silently widen to `Auto`.
#[must_use]
pub(crate) fn validate_publish_target(target: &PublishTarget) -> Result<(), String> {
    match target {
        PublishTarget::Auto => Ok(()),
        PublishTarget::Explicit { relays, .. } => validate_explicit_relays(relays),
    }
}

/// Validate the route provenance for an externally signed/verbatim event.
///
/// Pre-signed publish is not a normal app write path. It may only leave the
/// kernel through explicit protocol/import-owned relay pins where the caller
/// has already proven why those exact relays are correct.
#[must_use]
pub(crate) fn validate_presigned_publish_target(target: &PublishTarget) -> Result<(), String> {
    let PublishTarget::Explicit {
        relays,
        route_class,
    } = target
    else {
        return Err(
            "pre-signed publish requires an explicit non-empty imported/protocol relay target"
                .to_string(),
        );
    };

    validate_explicit_relays(relays)?;

    match route_class {
        PublishRouteClass::ImportedOrPresigned
        | PublishRouteClass::GroupHostPin
        | PublishRouteClass::VerifiedPrivateInbox => Ok(()),
        PublishRouteClass::ManualOverride => Err(
            "pre-signed publish rejects manual_override routes; use unsigned app publish or an imported/protocol route"
                .to_string(),
        ),
        PublishRouteClass::Diagnostic => Err(
            "pre-signed publish rejects diagnostic routes without a reviewed protocol entrypoint"
                .to_string(),
        ),
    }
}

#[must_use]
pub(crate) fn validate_explicit_relays(relays: &[RelayUrl]) -> Result<(), String> {
    if relays.is_empty() {
        return Err("explicit publish target requires at least one relay".to_string());
    }
    for relay in relays {
        if CanonicalRelayUrl::parse(relay).is_none() {
            return Err(format!(
                "explicit publish target relay '{relay}' must be a ws:// or wss:// relay URL"
            ));
        }
    }
    Ok(())
}
