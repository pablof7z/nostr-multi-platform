//! URL-keyed relay runtime owner (#1938).
//!
//! Consolidates the four per-URL relay loop-locals that used to be scattered
//! across `actor/mod.rs` into a single actor-owned bookkeeping struct:
//! `relay_controls`, `slot_to_url`, `connected_urls`, and
//! `next_relay_generation`.
//!
//! # Single-writer readiness (D4)
//!
//! The canonical socket-readiness fact is **per-URL**: a URL is connected or
//! it is not (`connected_urls`). "Is role R ready" is a *derived* question —
//! `∃ URL u ∈ connected_urls such that relay_controls[u].role == R` — never a
//! second, independently-mutated fact. The previous design kept a parallel
//! `connected_relays: HashSet<RelayRole>` that was `insert`ed on `Opened` and
//! `remove`d on `Failed`/`Closed`; because it was role-keyed, a single failed
//! sibling socket on a role dropped the whole role even while another URL on
//! that same role was still up. Deriving the role view from `connected_urls`
//! eliminates that drift: a sibling URL staying in `connected_urls` keeps the
//! role derived-ready.
//!
//! `pool: &Pool` is intentionally NOT owned here — it is process-wide and
//! stays a distinct field/param on every relay helper.

use std::collections::{HashMap, HashSet};

#[cfg(test)]
use nmp_network::role::RelayRole;

use crate::relay::CanonicalRelayUrl;

use super::relay_control::RelayControl;

/// Actor-owned per-URL relay bookkeeping (native-only).
///
/// Replaces the five scattered loop-locals with one owner. `connected_urls`
/// is THE canonical per-socket readiness fact; role readiness is derived from
/// it via [`RelayRuntime::roles_connected`] / [`RelayRuntime::any_role_connected`].
pub(in crate::actor) struct RelayRuntime {
    /// URL-keyed per-worker control rows (T105 one-socket-per-URL).
    pub(super) relay_controls: HashMap<CanonicalRelayUrl, RelayControl>,
    /// `RelayHandle.slot()` → canonical URL reverse-map for O(1) `PoolEvent`
    /// resolution (handle-carrying variants don't all carry the URL).
    pub(super) slot_to_url: HashMap<u32, CanonicalRelayUrl>,
    /// THE canonical per-socket readiness fact (T116/G1 reconnect-replay
    /// discriminator). A URL is in this set iff its socket has reported
    /// `Opened` and has not since `Failed`/`Closed`.
    pub(super) connected_urls: HashSet<CanonicalRelayUrl>,
}

impl RelayRuntime {
    pub(super) fn new() -> Self {
        Self {
            relay_controls: HashMap::new(),
            slot_to_url: HashMap::new(),
            connected_urls: HashSet::new(),
        }
    }

    /// Derived role-readiness view: the single derivation site joining
    /// `connected_urls` against each `RelayControl.role`. A URL that is
    /// connected ⇒ its control's role is ready.
    ///
    /// `#[cfg(test)]`: production reads [`Self::any_role_connected`] (the
    /// boolean send-gate); the full role set is only materialised by tests
    /// (the `all`-gate contrast reference in `send_gate_universal_tests`).
    #[cfg(test)]
    pub(super) fn roles_connected(&self) -> HashSet<RelayRole> {
        self.connected_urls
            .iter()
            .filter_map(|url| self.relay_controls.get(url).map(|c| c.role))
            .collect()
    }

    /// Derived claim/open send-gate (Fix A — `any` semantics): true iff ANY
    /// connected URL exists, i.e. at least one role lane is ready. Every
    /// connected URL has a control row with a role, so this is equivalent to
    /// `!self.roles_connected().is_empty()` but avoids rebuilding a set.
    pub(super) fn any_role_connected(&self) -> bool {
        self.connected_urls
            .iter()
            .any(|url| self.relay_controls.contains_key(url))
    }

    /// Mark a URL's socket connected. Returns `true` if the URL was newly
    /// inserted (initial dial) and `false` if it was already present (a
    /// reconnect — the T116/G1 reconnect-replay discriminator).
    pub(super) fn mark_url_connected(&mut self, url: &CanonicalRelayUrl) -> bool {
        self.connected_urls.insert(url.clone())
    }

    /// Mark a URL's socket disconnected (per-URL `Failed`/`Closed`). Sibling
    /// URLs on the same role stay in `connected_urls`, so the role stays
    /// derived-ready — this is the #1938 bug fix.
    pub(super) fn mark_url_disconnected(&mut self, url: &CanonicalRelayUrl) {
        self.connected_urls.remove(url);
    }

    /// Clear all per-URL connected state (global drain on close).
    pub(super) fn clear_connected(&mut self) {
        self.connected_urls.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_network::pool::{Pool, PoolConfig, PoolEvent};
    use std::sync::mpsc;

    use crate::actor::relay_mgmt::ensure_relay_worker;
    use crate::kernel::Kernel;

    fn fresh_pool() -> (Pool, mpsc::Receiver<PoolEvent>) {
        let (tx, rx) = mpsc::channel::<PoolEvent>();
        (Pool::new(PoolConfig::default(), tx), rx)
    }

    /// Seed a worker for `url` on `role` in the runtime (synchronous keying
    /// decision; no live socket needed).
    fn seed(rt: &mut RelayRuntime, pool: &Pool, kernel: &mut Kernel, role: RelayRole, url: &str) {
        ensure_relay_worker(rt, pool, kernel, role, url.to_string());
    }

    /// Acceptance criterion #1: two sibling URLs on the same role both
    /// connected → both readiness facts reflect Content; failing ONE sibling
    /// keeps the role ready because the other sibling stays connected.
    #[test]
    fn sibling_url_failure_keeps_role_ready() {
        let (pool, _rx) = fresh_pool();
        let mut kernel = Kernel::new(80);
        let mut rt = RelayRuntime::new();

        let url_a = CanonicalRelayUrl::parse_or_raw("wss://127.0.0.1:1");
        let url_b = CanonicalRelayUrl::parse_or_raw("wss://127.0.0.2:1");
        seed(
            &mut rt,
            &pool,
            &mut kernel,
            RelayRole::Content,
            url_a.as_str(),
        );
        seed(
            &mut rt,
            &pool,
            &mut kernel,
            RelayRole::Content,
            url_b.as_str(),
        );

        // Opened(A) + Opened(B): both connected, role Content ready.
        assert!(rt.mark_url_connected(&url_a), "first open of A is fresh");
        assert!(rt.mark_url_connected(&url_b), "first open of B is fresh");
        assert!(rt.any_role_connected());
        assert!(rt.roles_connected().contains(&RelayRole::Content));

        // Failed(A): sibling B still connected ⇒ role Content STILL ready.
        // This is the #1938 fix — the old role-set would have dropped Content.
        rt.mark_url_disconnected(&url_a);
        assert!(
            rt.any_role_connected(),
            "#1938: a single sibling URL failure must NOT drop the role"
        );
        assert!(
            rt.roles_connected().contains(&RelayRole::Content),
            "#1938: role stays derived-ready while sibling URL B is connected"
        );

        // Closed(B): now no URL on Content is connected ⇒ role no longer ready.
        rt.mark_url_disconnected(&url_b);
        assert!(!rt.any_role_connected());
        assert!(!rt.roles_connected().contains(&RelayRole::Content));
    }

    /// Acceptance criterion #4: startup/claim readiness derivation flips with
    /// per-URL socket state, including across distinct roles.
    #[test]
    fn readiness_derivation_across_roles() {
        let (pool, _rx) = fresh_pool();
        let mut kernel = Kernel::new(80);
        let mut rt = RelayRuntime::new();

        let content = CanonicalRelayUrl::parse_or_raw("wss://content.example");
        let indexer = CanonicalRelayUrl::parse_or_raw("wss://indexer.example");
        seed(
            &mut rt,
            &pool,
            &mut kernel,
            RelayRole::Content,
            content.as_str(),
        );
        seed(
            &mut rt,
            &pool,
            &mut kernel,
            RelayRole::Indexer,
            indexer.as_str(),
        );

        // Cold: nothing connected → not ready, empty role set.
        assert!(!rt.any_role_connected());
        assert!(rt.roles_connected().is_empty());

        // Content opens: any-gate true; only Content derived-ready.
        rt.mark_url_connected(&content);
        assert!(rt.any_role_connected());
        let roles = rt.roles_connected();
        assert!(roles.contains(&RelayRole::Content));
        assert!(!roles.contains(&RelayRole::Indexer));

        // Indexer opens: both lanes derived-ready.
        rt.mark_url_connected(&indexer);
        let roles = rt.roles_connected();
        assert!(roles.contains(&RelayRole::Content));
        assert!(roles.contains(&RelayRole::Indexer));

        // Global drain clears readiness.
        rt.clear_connected();
        assert!(!rt.any_role_connected());
        assert!(rt.roles_connected().is_empty());
    }

    /// Reconnect discriminator: the first `mark_url_connected` is fresh
    /// (returns true); a second without an intervening disconnect is a
    /// reconnect (returns false).
    #[test]
    fn mark_url_connected_reports_reconnect() {
        let mut rt = RelayRuntime::new();
        let url = CanonicalRelayUrl::parse_or_raw("wss://r.example");
        assert!(rt.mark_url_connected(&url), "initial dial is fresh");
        assert!(
            !rt.mark_url_connected(&url),
            "second open without disconnect is a reconnect"
        );
        rt.mark_url_disconnected(&url);
        assert!(
            rt.mark_url_connected(&url),
            "after disconnect the next open is fresh again"
        );
    }
}
