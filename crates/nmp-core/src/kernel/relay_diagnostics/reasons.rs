//! Human-readable relay connection reason strings derived from [`RelayAttribution`].

use nmp_planner::plan::{RelayAttribution, UserConfiguredCategory};
use serde::{Deserialize, Serialize};

/// One entry in the `reasons` list on a [`super::RelayDiagnosticsRow`].
///
/// `kind` is a stable machine tag (e.g. `"nip65"`, `"app_relay"`,
/// `"blocked"`); `label` is the pre-formatted human label the shell renders
/// directly — no translation, no protocol-number parsing in the shell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RelayConnectionReason {
    pub(crate) kind: String,
    pub(crate) label: String,
}

/// Build the connection-reason list for one relay row.
///
/// `attr` is the [`RelayAttribution`] snapshot captured from
/// `SubscriptionLifecycle::current_plan_attribution()` before the blocked-relay
/// post-pass. `None` means no compile has run yet (empty reasons list).
///
/// Special case: when `is_blocked` is `true` the first element is always a
/// `"blocked"` reason so the shell can surface the icon / tone even before it
/// looks at `connection_tone`.
pub(crate) fn build_reasons(
    attr: Option<&RelayAttribution>,
    is_blocked: bool,
) -> Vec<RelayConnectionReason> {
    let mut out = Vec::new();

    if is_blocked {
        out.push(RelayConnectionReason {
            kind: "blocked".to_string(),
            label: "Blocked".to_string(),
        });
    }

    let Some(attr) = attr else { return out };

    // NIP-65 outbox authors.
    let outbox_count = attr.outbox_authors.len();
    if outbox_count > 0 {
        out.push(RelayConnectionReason {
            kind: "nip65".to_string(),
            label: if outbox_count == 1 {
                "Outbox of 1 person".to_string()
            } else {
                format!("Outbox of {outbox_count} people")
            },
        });
    }

    // Relay hints (includes Provenance and DM relay).
    if !attr.hints.is_empty() {
        out.push(RelayConnectionReason {
            kind: "hint".to_string(),
            label: "Relay hint".to_string(),
        });
    }

    // User-configured sub-categories.
    for cat in &attr.user_configured {
        let (kind, label) = match cat {
            UserConfiguredCategory::AccountRead => ("account_read", "Account read relay"),
            UserConfiguredCategory::AccountWrite => ("account_write", "Account write relay"),
            UserConfiguredCategory::Indexer => ("indexer", "Indexer relay"),
            UserConfiguredCategory::AppRelay => ("app_relay", "App relay"),
            UserConfiguredCategory::Debug => ("debug", "Debug relay"),
            UserConfiguredCategory::Bootstrap => ("bootstrap", "Bootstrap relay"),
        };
        out.push(RelayConnectionReason {
            kind: kind.to_string(),
            label: label.to_string(),
        });
    }

    out
}
