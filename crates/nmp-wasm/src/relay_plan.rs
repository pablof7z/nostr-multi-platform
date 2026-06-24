//! Pure relay-driver planning — the single decision point for *which* sockets
//! the wasm32 transport opens at `Start`.
//!
//! # Why this exists (one socket per URL, not per (URL, role))
//!
//! The native relay pool keys sockets by URL alone: `nmp_network::pool`'s
//! `ensure_open` returns the *existing* handle and **ignores the role** when a
//! URL is already open, and the actor's `relay_controls` map is keyed by
//! `CanonicalRelayUrl`. So a `"both,indexer"` relay is **one** socket on
//! native, recorded under the first role to claim it.
//!
//! The wasm32 transport previously spawned one `BrowserRelayDriver` per
//! `(URL, role)` pair, so a `"both,indexer"` relay opened **two** WebSockets to
//! the same host — a divergence from native that surfaced as duplicate,
//! half-idle relay connections in the browser network panel. This module
//! restores native parity: collapse the bootstrap list to one driver per
//! distinct URL.
//!
//! # Role attribution is native-parity, not WASM-special
//!
//! Each collapsed driver reports inbound frames under a single
//! `primary_role` — the first of `RelayRole::all()` (`[Content, Indexer]`)
//! present in the URL's declared role set, so `"both,indexer"` reports as
//! `Content`, exactly like the native pool's first-role-wins slot. This is
//! behaviour-preserving because **inbound role is diagnostics-only**: the
//! kernel ingests events identically regardless of role and routes outbound
//! purely by URL. The host's full declared role set still reaches the UI via
//! the kernel's `configured_relays` projection (seeded from the same bootstrap
//! in `WasmRuntime::start`), independent of the driver pool — so role badges
//! are unaffected.

use nmp_network::RelayRole;

use crate::protocol::RelayBootstrapEntry;

/// One planned driver: a distinct relay URL, the role its socket reports
/// inbound frames under (native-parity primary), and the full set of roles the
/// host declared for that URL across all bootstrap entries (kept for
/// diagnostics/assertions; the driver itself only consumes `url` +
/// `primary_role`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DriverPlan {
    pub url: String,
    pub primary_role: RelayRole,
    pub roles: Vec<RelayRole>,
}

/// Expand a bootstrap role string into the role lanes it declares.
///
/// Case-insensitive; surrounding whitespace trimmed. Unrecognized tokens fall
/// back to `Content` so a typo or future-protocol role token never drops the
/// relay from the pool (substrate-grade D0: the helper rejects nothing).
fn roles_for_entry(role_str: &str) -> &'static [RelayRole] {
    const CONTENT_ONLY: &[RelayRole] = &[RelayRole::Content];
    const INDEXER_ONLY: &[RelayRole] = &[RelayRole::Indexer];
    const BOTH_LANES: &[RelayRole] = &[RelayRole::Content, RelayRole::Indexer];

    match role_str.trim().to_ascii_lowercase().as_str() {
        "indexer" => INDEXER_ONLY,
        "both" | "both,indexer" => BOTH_LANES,
        // "content" and every unrecognized value — safe fallback.
        _ => CONTENT_ONLY,
    }
}

/// Native-parity primary role for a URL's declared role set: the first of
/// `RelayRole::all()` (`[Content, Indexer]`) present in the set. `Content` wins
/// for `both` / `both,indexer`, matching the native pool's first-role-wins
/// slot. Empty sets cannot occur (`roles_for_entry` never returns `[]`), but
/// fall back to `Content` defensively.
fn primary_role(roles: &[RelayRole]) -> RelayRole {
    RelayRole::all()
        .into_iter()
        .find(|role| roles.contains(role))
        .unwrap_or(RelayRole::Content)
}

/// Collapse bootstrap entries to one [`DriverPlan`] per distinct relay URL,
/// unioning declared roles across duplicate entries and preserving first-seen
/// URL order.
///
/// URLs are matched by exact string — the same key `fan_out_outbound` uses
/// (`driver.url() == message.relay_url()`) and the same string the kernel's
/// `configured_relays` were seeded from, so a planned driver always matches the
/// kernel's outbound targeting. (URL canonicalization — trailing slash, case —
/// is a separate concern handled kernel-side by `CanonicalRelayUrl`; it is not
/// the cause of the duplicate-socket bug, which was exact-string role doubling.)
pub(crate) fn plan_drivers(bootstrap: &[RelayBootstrapEntry]) -> Vec<DriverPlan> {
    let mut plans: Vec<DriverPlan> = Vec::with_capacity(bootstrap.len());
    for entry in bootstrap {
        let lanes = roles_for_entry(&entry.role);
        if let Some(existing) = plans.iter_mut().find(|plan| plan.url == entry.url) {
            for &role in lanes {
                if !existing.roles.contains(&role) {
                    existing.roles.push(role);
                }
            }
        } else {
            plans.push(DriverPlan {
                url: entry.url.clone(),
                // Provisional — finalized below once the full role union for
                // this URL is known.
                primary_role: RelayRole::Content,
                roles: lanes.to_vec(),
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

    fn entry(url: &str, role: &str) -> RelayBootstrapEntry {
        RelayBootstrapEntry {
            url: url.to_string(),
            role: role.to_string(),
        }
    }

    #[test]
    fn both_indexer_collapses_to_one_driver_recorded_as_content() {
        // The exact shape that produced duplicate WebSockets in the browser:
        // a `both,indexer` relay must yield ONE driver, not two.
        let plans = plan_drivers(&[entry("wss://relay.primal.net", "both,indexer")]);
        assert_eq!(plans.len(), 1, "both,indexer must be a single socket");
        assert_eq!(plans[0].url, "wss://relay.primal.net");
        // Native first-role-wins: Content precedes Indexer in RelayRole::all().
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
        // Two bootstrap entries for the same URL (one Content, one Indexer)
        // must collapse to a single driver whose role union is [Content,
        // Indexer] and whose primary is Content.
        let plans = plan_drivers(&[
            entry("wss://nos.lol", "content"),
            entry("wss://nos.lol", "indexer"),
        ]);
        assert_eq!(plans.len(), 1, "same URL must not open two sockets");
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
            vec!["wss://relay.primal.net", "wss://purplepag.es", "wss://nos.lol"],
        );
        // The reported bug: primal/nos.lol doubled, purplepag once. Now all are
        // single sockets.
        assert_eq!(plans.len(), 3);
    }

    #[test]
    fn unrecognized_role_falls_back_to_content() {
        let plans = plan_drivers(&[entry("wss://relay.example", "totally-new-role")]);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].primary_role, RelayRole::Content);
    }
}
