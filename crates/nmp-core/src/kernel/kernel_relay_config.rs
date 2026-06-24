//! Bootstrap relay resolution, AUTH signers, persistent + wire subs.
//!
//! Extracted from `kernel/mod.rs` (`impl Kernel`) to honour the 500-LOC ceiling.

use super::*;

impl Kernel {
    /// Resolve configured relay URLs for a given `RelayRole`; empty when none are configured.
    pub(crate) fn bootstrap_urls_for_role(&self, role: RelayRole) -> Vec<String> {
        let matches = |row_role: &str| match role {
            RelayRole::Content => {
                crate::actor::has_role(row_role, "read")
                    || crate::actor::has_role(row_role, "write")
            }
            RelayRole::Indexer => crate::actor::has_role(row_role, "indexer"),
            RelayRole::Wallet => false,
        };
        self.configured_relays
            .iter()
            .filter(|r| matches(&r.role))
            .map(|r| r.url.clone())
            .collect()
    }

    /// Cold-start discovery seed (Indexer + Content URLs, sorted/deduped).
    pub(crate) fn bootstrap_discovery_relays(&self) -> Vec<String> {
        let mut urls: Vec<String> = self
            .bootstrap_urls_for_role(RelayRole::Indexer)
            .into_iter()
            .chain(self.bootstrap_urls_for_role(RelayRole::Content))
            .collect();
        sort_dedup(&mut urls);
        urls
    }

    /// Bind a per-role NIP-42 signer callback; replaces any previously-bound signer (D0).
    pub fn set_relay_auth_signer(
        &mut self,
        role: RelayRole,
        pubkey_hex: String,
        signer: AuthSignerFn,
    ) {
        self.auth_signers
            .insert(role, RelayAuthCredentials { signer, pubkey_hex });
    }

    /// Drop the signer for `role`; challenges from that role are then recorded but unanswered.
    pub fn clear_relay_auth_signer(&mut self, role: RelayRole) {
        self.auth_signers.remove(&role);
    }

    /// Bind the shared relay-edit rows slot so the FFI layer can read relay rows.
    pub(crate) fn set_app_relay_slot(&mut self, handle: AppRelaySlot) {
        self.configured_relays_handle = Some(handle);
    }

    /// Extract the relay-edit rows handle before a `Reset` replaces the kernel.
    pub(crate) fn take_app_relay_slot_for_reset(&mut self) -> Option<AppRelaySlot> {
        self.configured_relays_handle.take()
    }

    /// Test-only: clear `configured_relays` for the empty-bootstrap diagnostic test path.
    #[cfg(test)]
    pub(crate) fn clear_configured_relays_for_test(&mut self) {
        self.configured_relays.clear();
        if let Some(handle) = self.configured_relays_handle.as_ref() {
            if let Ok(mut guard) = handle.lock() {
                guard.replace(Vec::new());
            }
        }
    }

    /// Mark `(relay_url, sub_id)` as persistent — EOSE will not auto-CLOSE it.
    pub fn register_persistent_sub(
        &mut self,
        relay_url: impl Into<String>,
        sub_id: impl Into<String>,
    ) {
        let relay_url = relay_url.into();
        let key = CanonicalRelayUrl::parse_or_raw(&relay_url);
        self.wire.persistent.insert((key, sub_id.into()));
    }

    /// Remove `(relay_url, sub_id)` from the persistent set. Idempotent.
    pub fn unregister_persistent_sub(&mut self, relay_url: &str, sub_id: &str) {
        let key = CanonicalRelayUrl::parse_or_raw(relay_url);
        self.wire.persistent.remove(&(key, sub_id.to_string()));
    }

    /// True when `(relay_url, sub_id)` is registered as persistent.
    pub(crate) fn is_persistent_sub(&self, relay_url: &str, sub_id: &str) -> bool {
        let key = CanonicalRelayUrl::parse_or_raw(relay_url);
        self.wire.persistent.contains(&(key, sub_id.to_string()))
    }

    /// Single-writer insert into `self.wire.subs` (PD-033-C Stage 0).
    pub(crate) fn insert_wire_sub(
        &mut self,
        role: RelayRole,
        relay_url: CanonicalRelayUrl,
        sub_id: String,
        filter_summary: String,
        initial_state: &str,
        since_floor: Option<u64>,
    ) {
        self.wire.subs.insert(
            (relay_url.clone(), sub_id.clone()),
            WireSub {
                id: sub_id,
                role,
                relay_url,
                filter_summary,
                state: initial_state.to_string(),
                events_rx: 0,
                opened_at: Instant::now(), // doctrine-allow: D9 — wire-sub diagnostic elapsed-time marker; not replay policy
                last_event_at: None,
                eose_at: None,
                close_reason: None,
                since_floor,
            },
        );
        self.changed_since_emit = true;
    }
}
