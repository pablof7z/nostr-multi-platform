//! `TofuSignerCache` — metadata-signer trust per `moderation.md` §4.3.
//!
//! Step ladder enforced on every 39000-39003 ingest:
//! 1. NIP-11 strict (policy A) when host declares `pubkey`.
//! 2. TOFU steady state (policy B) when group already pinned.
//! 3. Cold TOFU — only **kind:39000** establishes the pin; 39001/39002/39003
//!    are quarantined (max 64 per group) until 39000 lands.
//! 4. Signer mismatch → reject with `MetadataSignerChanged`; do not mutate.

use std::collections::{BTreeMap, VecDeque};

use crate::group_id::{GroupId, RelayUrl};

#[derive(Clone, Debug, Default)]
pub struct TofuSignerCache {
    /// Per-group pinned signer (the pubkey we accepted in the first 39000).
    pinned: BTreeMap<GroupId, String>,
    /// Per-host NIP-11 declared pubkey (policy A: strict match).
    nip11_pubkey: BTreeMap<RelayUrl, String>,
    /// Quarantine buffer: 39001/39002/39003 events held until a 39000 lands.
    quarantine: BTreeMap<GroupId, VecDeque<QuarantinedEvent>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct QuarantinedEvent {
    pub kind: u32,
    pub signer_pubkey: String,
    pub event_id: String,
    pub created_at: u64,
}

/// Outcome of a metadata-event trust check per `moderation.md` §4.3 steps 1-4.
#[derive(Clone, Debug, PartialEq)]
pub enum TrustCheckOutcome {
    /// Accepted: the event may mutate canonical state.
    Accepted,
    /// Quarantined: pinned signer not yet known for this group; the event
    /// (must be 39001/39002/39003) is held until a 39000 lands and the
    /// quarantine is replayed.
    Quarantined,
    /// Rejected: signer mismatch. Surface `MetadataSignerChanged` to the
    /// diagnostics lane; do not mutate canonical state.
    Rejected,
}

impl TofuSignerCache {
    #[must_use] 
    pub fn new() -> Self {
        Self::default()
    }

    /// Record NIP-11-declared pubkey for a host. When present, policy A
    /// (strict match) is active for the host's metadata events.
    pub fn set_nip11_pubkey(&mut self, host: impl Into<RelayUrl>, pubkey: impl Into<String>) {
        self.nip11_pubkey.insert(host.into(), pubkey.into());
    }

    /// Evaluate trust for a metadata event per the §4.3 step ladder. Caller
    /// passes the event's `kind` (must be 39000-39003), `group`, and
    /// `signer_pubkey`.
    pub fn evaluate(
        &mut self,
        kind: u32,
        group: &GroupId,
        signer_pubkey: &str,
        event_id: &str,
        created_at: u64,
    ) -> TrustCheckOutcome {
        // Step 1: NIP-11 strict match if declared.
        if let Some(declared) = self.nip11_pubkey.get(&group.host_relay_url) {
            return if declared == signer_pubkey {
                TrustCheckOutcome::Accepted
            } else {
                TrustCheckOutcome::Rejected
            };
        }
        // Step 2: TOFU steady state.
        if let Some(pinned) = self.pinned.get(group) {
            return if pinned == signer_pubkey {
                TrustCheckOutcome::Accepted
            } else {
                TrustCheckOutcome::Rejected
            };
        }
        // Step 3: cold TOFU. Only kind:39000 may establish the pin; other
        // kinds are quarantined per §4.3.
        if kind == crate::kinds::KIND_GROUP_METADATA {
            self.pinned.insert(group.clone(), signer_pubkey.to_string());
            TrustCheckOutcome::Accepted
        } else {
            self.push_quarantine(group, kind, signer_pubkey, event_id, created_at);
            TrustCheckOutcome::Quarantined
        }
    }

    fn push_quarantine(
        &mut self,
        group: &GroupId,
        kind: u32,
        signer: &str,
        event_id: &str,
        created_at: u64,
    ) {
        let q = self.quarantine.entry(group.clone()).or_default();
        q.push_back(QuarantinedEvent {
            kind,
            signer_pubkey: signer.to_string(),
            event_id: event_id.to_string(),
            created_at,
        });
        while q.len() > 64 {
            q.pop_front();
        }
    }

    /// Drain the quarantine for a group, returning entries split into
    /// accepted vs rejected by re-evaluating against the (now-pinned) signer.
    /// Caller routes accepted entries through the normal ingest path.
    pub fn replay_quarantine(
        &mut self,
        group: &GroupId,
    ) -> Vec<(QuarantinedEvent, TrustCheckOutcome)> {
        let Some(q) = self.quarantine.remove(group) else {
            return Vec::new();
        };
        let pinned = self.pinned.get(group).cloned();
        q.into_iter()
            .map(|qe| {
                let outcome = match &pinned {
                    Some(p) if *p == qe.signer_pubkey => TrustCheckOutcome::Accepted,
                    Some(_) => TrustCheckOutcome::Rejected,
                    None => TrustCheckOutcome::Quarantined,
                };
                (qe, outcome)
            })
            .collect()
    }

    #[must_use]
    pub fn pinned_signer(&self, group: &GroupId) -> Option<&str> {
        self.pinned.get(group).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group() -> GroupId {
        GroupId::new("wss://h.example.com", "g1")
    }

    #[test]
    fn tofu_first_39000_pins_signer() {
        let mut t = TofuSignerCache::new();
        let g = group();
        let r = t.evaluate(crate::kinds::KIND_GROUP_METADATA, &g, "relay-pk", "evt-id", 1);
        assert_eq!(r, TrustCheckOutcome::Accepted);
        assert_eq!(t.pinned_signer(&g), Some("relay-pk"));
        let r = t.evaluate(crate::kinds::KIND_GROUP_ADMINS, &g, "relay-pk", "evt-2", 2);
        assert_eq!(r, TrustCheckOutcome::Accepted);
    }

    #[test]
    fn tofu_quarantines_39001_before_39000() {
        let mut t = TofuSignerCache::new();
        let g = group();
        let r = t.evaluate(crate::kinds::KIND_GROUP_ADMINS, &g, "spoofer", "evt-a", 1);
        assert_eq!(r, TrustCheckOutcome::Quarantined);
        assert_eq!(t.pinned_signer(&g), None);
        let r = t.evaluate(crate::kinds::KIND_GROUP_METADATA, &g, "relay-pk", "evt-b", 2);
        assert_eq!(r, TrustCheckOutcome::Accepted);
        let replayed = t.replay_quarantine(&g);
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].1, TrustCheckOutcome::Rejected);
    }

    #[test]
    fn nip11_strict_match_rejects_mismatch() {
        let mut t = TofuSignerCache::new();
        let g = group();
        t.set_nip11_pubkey(g.host_relay_url.clone(), "declared-pk");
        let r = t.evaluate(crate::kinds::KIND_GROUP_METADATA, &g, "other-pk", "evt", 1);
        assert_eq!(r, TrustCheckOutcome::Rejected);
        let r = t.evaluate(crate::kinds::KIND_GROUP_METADATA, &g, "declared-pk", "evt", 1);
        assert_eq!(r, TrustCheckOutcome::Accepted);
    }
}
