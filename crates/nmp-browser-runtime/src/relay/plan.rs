//! Browser relay driver planning — one socket per URL/role pair.
//!
//! Browser WebSocket frames do not carry role metadata, while the kernel keys
//! wire subscriptions by `(role, relay_url, sub_id)`. A relay declared for both
//! content and indexer lanes therefore needs one browser driver per lane so
//! inbound frames are reported under the same role that emitted the REQ.
//!
//! The planner is always-compiled (no wasm32 gate) so it can be tested natively
//! without a wasm32 toolchain.

// Plan structs and functions are consumed from wasm32-gated spawn/mod code.
// On native they are unused outside tests; suppress the lint rather than
// duplicating cfg gates throughout the file.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use nmp_network::role::RelayRole;

/// One planned driver: a distinct relay URL/role pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DriverPlan {
    pub(crate) url: String,
    pub(crate) role: RelayRole,
}

/// Expand a bootstrap role string into the role lanes it declares.
///
/// Case-insensitive, whitespace-trimmed. Supports `read`, `write`, `both`,
/// `content`, `indexer` and composites. Unrecognized tokens fall back to
/// `Content` (D0: never drops the relay from the pool for a bad token).
fn roles_for_str(role_str: &str) -> Vec<RelayRole> {
    let mut roles = Vec::new();
    for token in role_str
        .split(',')
        .map(|part| part.trim().to_ascii_lowercase())
    {
        match token.as_str() {
            "read" | "write" | "both" | "content" => {
                if !roles.contains(&RelayRole::Content) {
                    roles.push(RelayRole::Content);
                }
            }
            "indexer" if !roles.contains(&RelayRole::Indexer) => {
                roles.push(RelayRole::Indexer);
            }
            "indexer" => {}
            _ => {}
        }
    }
    if roles.is_empty() {
        roles.push(RelayRole::Content);
    }
    roles
}

/// Expand `(url, role_str)` pairs to one [`DriverPlan`] per distinct URL/role,
/// preserving first-seen order.
///
/// Each `(url, role_str)` entry matches the format stored in
/// `BrowserBuilderInner::relay_bootstrap: Vec<(String, String)>`.
pub(crate) fn plan_drivers(bootstrap: &[(String, String)]) -> Vec<DriverPlan> {
    let mut plans: Vec<DriverPlan> = Vec::with_capacity(bootstrap.len());
    for (url, role_str) in bootstrap {
        for role in roles_for_str(role_str) {
            if plans.iter().any(|p| p.url == *url && p.role == role) {
                continue;
            }
            plans.push(DriverPlan {
                url: url.clone(),
                role,
            });
        }
    }
    plans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str, role: &str) -> (String, String) {
        (url.to_string(), role.to_string())
    }

    #[test]
    fn both_indexer_expands_to_content_and_indexer_drivers() {
        let plans = plan_drivers(&[entry("wss://relay.primal.net", "both,indexer")]);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].role, RelayRole::Content);
        assert_eq!(plans[1].role, RelayRole::Indexer);
    }

    #[test]
    fn indexer_only_relay_is_one_indexer_driver() {
        let plans = plan_drivers(&[entry("wss://purplepag.es", "indexer")]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].role, RelayRole::Indexer);
    }

    #[test]
    fn duplicate_url_distinct_roles_produces_one_driver_per_role() {
        let plans = plan_drivers(&[
            entry("wss://nos.lol", "content"),
            entry("wss://nos.lol", "indexer"),
        ]);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].role, RelayRole::Content);
        assert_eq!(plans[1].role, RelayRole::Indexer);
    }

    #[test]
    fn distinct_urls_get_distinct_drivers_in_first_seen_order() {
        let plans = plan_drivers(&[
            entry("wss://relay.primal.net", "both,indexer"),
            entry("wss://purplepag.es", "indexer"),
            entry("wss://nos.lol", "both,indexer"),
        ]);
        let pairs: Vec<(&str, RelayRole)> =
            plans.iter().map(|p| (p.url.as_str(), p.role)).collect();
        assert_eq!(
            pairs,
            vec![
                ("wss://relay.primal.net", RelayRole::Content),
                ("wss://relay.primal.net", RelayRole::Indexer),
                ("wss://purplepag.es", RelayRole::Indexer),
                ("wss://nos.lol", RelayRole::Content),
                ("wss://nos.lol", RelayRole::Indexer),
            ]
        );
    }

    #[test]
    fn unrecognized_role_falls_back_to_content() {
        let plans = plan_drivers(&[entry("wss://relay.example", "totally-new-role")]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].role, RelayRole::Content);
    }

    #[test]
    fn read_indexer_composite_produces_both_roles() {
        let plans = plan_drivers(&[entry("wss://relay.example", "read,indexer")]);
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].role, RelayRole::Content);
        assert_eq!(plans[1].role, RelayRole::Indexer);
    }

    #[test]
    fn write_role_is_content_lane() {
        let plans = plan_drivers(&[entry("wss://relay.example", "write")]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].role, RelayRole::Content);
    }
}
