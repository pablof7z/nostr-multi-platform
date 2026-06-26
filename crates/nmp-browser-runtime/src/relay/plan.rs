//! Browser relay driver planning — one socket per URL (native parity).
//!
//! Ported from `nmp-wasm/src/relay_plan.rs`. See that module's documentation
//! for the rationale (why one socket per URL, not per (URL, role) pair).
//!
//! The planner is always-compiled (no wasm32 gate) so it can be tested natively
//! without wasm32 toolchain, matching the approach in nmp-wasm.

// Plan structs and functions are consumed from wasm32-gated spawn/mod code.
// On native they are unused outside tests; suppress the lint rather than
// duplicating cfg gates throughout the file.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use nmp_network::role::RelayRole;

/// One planned driver: a distinct relay URL, the primary role its socket
/// reports inbound frames under (native-parity first-role-wins), and the
/// full declared role union for diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DriverPlan {
    pub(crate) url: String,
    pub(crate) primary_role: RelayRole,
    pub(crate) roles: Vec<RelayRole>,
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

/// Native-parity primary role: first of `RelayRole::all()` (`[Content, Indexer]`)
/// present in the set. `Content` wins for `both` / `both,indexer`.
fn primary_role(roles: &[RelayRole]) -> RelayRole {
    RelayRole::all()
        .into_iter()
        .find(|r| roles.contains(r))
        .unwrap_or(RelayRole::Content)
}

/// Collapse `(url, role_str)` pairs to one [`DriverPlan`] per distinct URL,
/// unioning declared roles and preserving first-seen order.
///
/// Each `(url, role_str)` entry matches the format stored in
/// `BrowserBuilderInner::relay_bootstrap: Vec<(String, String)>`.
pub(crate) fn plan_drivers(bootstrap: &[(String, String)]) -> Vec<DriverPlan> {
    let mut plans: Vec<DriverPlan> = Vec::with_capacity(bootstrap.len());
    for (url, role_str) in bootstrap {
        let lanes = roles_for_str(role_str);
        if let Some(existing) = plans.iter_mut().find(|p| &p.url == url) {
            for role in lanes {
                if !existing.roles.contains(&role) {
                    existing.roles.push(role);
                }
            }
        } else {
            plans.push(DriverPlan {
                url: url.clone(),
                primary_role: RelayRole::Content, // finalized below
                roles: lanes,
            });
        }
    }
    for plan in &mut plans {
        plan.primary_role = primary_role(&plan.roles);
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
    fn both_indexer_collapses_to_one_driver_recorded_as_content() {
        let plans = plan_drivers(&[entry("wss://relay.primal.net", "both,indexer")]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].primary_role, RelayRole::Content);
        assert_eq!(plans[0].roles, vec![RelayRole::Content, RelayRole::Indexer]);
    }

    #[test]
    fn indexer_only_relay_is_one_indexer_driver() {
        let plans = plan_drivers(&[entry("wss://purplepag.es", "indexer")]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].primary_role, RelayRole::Indexer);
        assert_eq!(plans[0].roles, vec![RelayRole::Indexer]);
    }

    #[test]
    fn duplicate_url_distinct_roles_unions_into_one_driver() {
        let plans = plan_drivers(&[
            entry("wss://nos.lol", "content"),
            entry("wss://nos.lol", "indexer"),
        ]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].primary_role, RelayRole::Content);
        assert_eq!(plans[0].roles, vec![RelayRole::Content, RelayRole::Indexer]);
    }

    #[test]
    fn distinct_urls_get_distinct_drivers_in_first_seen_order() {
        let plans = plan_drivers(&[
            entry("wss://relay.primal.net", "both,indexer"),
            entry("wss://purplepag.es", "indexer"),
            entry("wss://nos.lol", "both,indexer"),
        ]);
        let urls: Vec<&str> = plans.iter().map(|p| p.url.as_str()).collect();
        assert_eq!(
            urls,
            vec!["wss://relay.primal.net", "wss://purplepag.es", "wss://nos.lol"]
        );
        assert_eq!(plans.len(), 3);
    }

    #[test]
    fn unrecognized_role_falls_back_to_content() {
        let plans = plan_drivers(&[entry("wss://relay.example", "totally-new-role")]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].primary_role, RelayRole::Content);
    }

    #[test]
    fn read_indexer_composite_produces_both_roles() {
        let plans = plan_drivers(&[entry("wss://relay.example", "read,indexer")]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].primary_role, RelayRole::Content);
        assert_eq!(plans[0].roles, vec![RelayRole::Content, RelayRole::Indexer]);
    }

    #[test]
    fn write_role_is_content_lane() {
        let plans = plan_drivers(&[entry("wss://relay.example", "write")]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].primary_role, RelayRole::Content);
        assert_eq!(plans[0].roles, vec![RelayRole::Content]);
    }
}
