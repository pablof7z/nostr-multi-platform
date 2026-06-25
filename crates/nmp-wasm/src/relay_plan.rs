//! Pure relay-driver planning — the single decision point for *which* sockets
//! the wasm32 transport opens at `Start`.
//!
//! # Why this exists (one socket per URL, not per (URL, role))
//!
//! The native relay pool keys sockets by URL alone: `nmp_network::pool`'s
//! `ensure_open` returns the *existing* handle and **ignores the role** when a
//! URL is already open, and the actor's relay maps are keyed by the
//! `nmp-relay-url` canonical form. So a `"both,indexer"` relay is **one** socket on
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

use std::fmt;

use nmp_network::role::RelayRole;

use crate::protocol::RelayBootstrapEntry;

/// Upper bound for host-supplied startup relays on either startup relay list.
pub(crate) const MAX_STARTUP_RELAY_COUNT: usize = 32;
/// Upper bound for one host-supplied relay URL string before canonicalization.
pub(crate) const MAX_RELAY_URL_BYTES: usize = 512;
/// Browser-wide relay socket budget for startup + kernel-discovered targets.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) const MAX_BROWSER_RELAY_SOCKETS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RelayAdmissionError {
    reason: String,
}

impl RelayAdmissionError {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    #[must_use]
    pub(crate) fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Display for RelayAdmissionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl std::error::Error for RelayAdmissionError {}

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
/// Case-insensitive; surrounding whitespace trimmed. Unrecognized tokens fail
/// closed so a typo or hostile startup config cannot silently create a content
/// relay.
fn roles_for_entry(role_str: &str) -> Result<&'static [RelayRole], RelayAdmissionError> {
    const CONTENT_ONLY: &[RelayRole] = &[RelayRole::Content];
    const INDEXER_ONLY: &[RelayRole] = &[RelayRole::Indexer];
    const BOTH_LANES: &[RelayRole] = &[RelayRole::Content, RelayRole::Indexer];

    match role_str.trim().to_ascii_lowercase().as_str() {
        "content" => Ok(CONTENT_ONLY),
        "indexer" => Ok(INDEXER_ONLY),
        "both" | "both,indexer" => Ok(BOTH_LANES),
        "" => Err(RelayAdmissionError::new("relay role is required")),
        other => Err(RelayAdmissionError::new(format!(
            "unknown relay role `{other}`"
        ))),
    }
}

fn role_label(roles: &[RelayRole]) -> &'static str {
    let has_content = roles.contains(&RelayRole::Content);
    let has_indexer = roles.contains(&RelayRole::Indexer);
    match (has_content, has_indexer) {
        (true, true) => "both",
        (false, true) => "indexer",
        _ => "content",
    }
}

fn canonicalize_relay_url(raw: &str) -> Result<String, RelayAdmissionError> {
    if raw.len() > MAX_RELAY_URL_BYTES {
        return Err(RelayAdmissionError::new(format!(
            "relay URL exceeds {MAX_RELAY_URL_BYTES} bytes"
        )));
    }
    nmp_relay_url::canonicalize(raw)
        .ok_or_else(|| RelayAdmissionError::new("relay URL must be ws:// or wss:// with a host"))
}

/// Native-parity primary role for a URL's declared role set: the first of
/// `RelayRole::all()` (`[Content, Indexer]`) present in the set. `Content` wins
/// for `both` / `both,indexer`, matching the native pool's first-role-wins
/// slot. Empty sets cannot occur (`roles_for_entry` never returns `[]`), but
/// fall back to `Content` defensively.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn primary_role(roles: &[RelayRole]) -> RelayRole {
    RelayRole::all()
        .into_iter()
        .find(|role| roles.contains(role))
        .unwrap_or(RelayRole::Content)
}

/// Collapse bootstrap entries to one [`DriverPlan`] per distinct relay URL,
/// unioning declared roles across duplicate entries and preserving first-seen
/// URL order.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn plan_drivers(bootstrap: &[RelayBootstrapEntry]) -> Vec<DriverPlan> {
    let mut plans: Vec<DriverPlan> = Vec::with_capacity(bootstrap.len());
    for entry in bootstrap {
        let lanes = roles_for_entry(&entry.role).expect("startup bootstrap is admitted first");
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

/// Admit and canonicalize host-supplied startup relay configuration.
///
/// The returned entries are canonical URL keys, deduplicated by URL, with role
/// unions collapsed to the canonical host-facing role strings (`content`,
/// `indexer`, `both`). Any off-contract URL, role, list length, or missing
/// accepted relay fails the whole `Start` closed so the runtime never silently
/// dials an unintended socket.
pub(crate) fn admit_startup_relays(
    relays: Vec<String>,
    relay_bootstrap: Vec<RelayBootstrapEntry>,
) -> Result<Vec<RelayBootstrapEntry>, RelayAdmissionError> {
    if relays.len() > MAX_STARTUP_RELAY_COUNT {
        return Err(RelayAdmissionError::new(format!(
            "relays exceeds {MAX_STARTUP_RELAY_COUNT} entries"
        )));
    }
    if relays.iter().any(|url| url.len() > MAX_RELAY_URL_BYTES) {
        return Err(RelayAdmissionError::new(format!(
            "relays contains a URL over {MAX_RELAY_URL_BYTES} bytes"
        )));
    }
    if relay_bootstrap.len() > MAX_STARTUP_RELAY_COUNT {
        return Err(RelayAdmissionError::new(format!(
            "relay_bootstrap exceeds {MAX_STARTUP_RELAY_COUNT} entries"
        )));
    }

    let raw = crate::protocol::relay_bootstrap_from_config(relays, relay_bootstrap);
    if raw.is_empty() {
        return Err(RelayAdmissionError::new("at least one relay is required"));
    }

    let mut plans: Vec<DriverPlan> = Vec::with_capacity(raw.len());
    for (idx, entry) in raw.into_iter().enumerate() {
        let url = canonicalize_relay_url(&entry.url).map_err(|err| {
            RelayAdmissionError::new(format!("relay[{idx}] rejected: {}", err.reason()))
        })?;
        let lanes = roles_for_entry(&entry.role).map_err(|err| {
            RelayAdmissionError::new(format!("relay[{idx}] rejected: {}", err.reason()))
        })?;
        if let Some(existing) = plans.iter_mut().find(|plan| plan.url == url) {
            for &role in lanes {
                if !existing.roles.contains(&role) {
                    existing.roles.push(role);
                }
            }
        } else {
            plans.push(DriverPlan {
                url,
                primary_role: RelayRole::Content,
                roles: lanes.to_vec(),
            });
        }
    }

    if plans.is_empty() {
        return Err(RelayAdmissionError::new("at least one relay is required"));
    }

    Ok(plans
        .into_iter()
        .map(|plan| RelayBootstrapEntry {
            url: plan.url,
            role: role_label(&plan.roles).to_string(),
        })
        .collect())
}

/// Admit a kernel-targeted on-demand outbound URL before matching or spawning a
/// browser driver.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn admit_on_demand_url(raw: &str) -> Result<String, RelayAdmissionError> {
    canonicalize_relay_url(raw)
}

#[must_use]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub(crate) fn diagnostic_target_for(raw: &str) -> String {
    if raw.len() > MAX_RELAY_URL_BYTES {
        "<relay-url-too-long>".to_string()
    } else {
        raw.trim().to_string()
    }
}

#[cfg(test)]
#[path = "relay_plan/tests.rs"]
mod tests;
