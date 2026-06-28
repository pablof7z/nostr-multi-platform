//! `TofuSignerCache` — metadata-signer trust per `moderation.md` §4.3.
//!
//! Step ladder enforced on every 39000-39003 ingest:
//! 1. NIP-11 strict (policy A) when host declares `pubkey`.
//! 2. TOFU steady state (policy B) when group already pinned.
//! 3. Cold TOFU — only **kind:39000** establishes the pin; 39001/39002/39003
//!    are quarantined (max 64 per group) until 39000 lands.
//! 4. Signer mismatch → reject with `MetadataSignerChanged`; do not mutate.
//!
//! ## Persistence (D4-compliant, #2286)
//!
//! [`TofuSignerCache::open`] loads pinned-signer and NIP-11 state from the
//! `nmp.nip29.tofu_signer` domain namespace on startup. Every mutation that
//! changes the pinned map writes through immediately. The quarantine buffer
//! is transient-only: it holds 39001/39002/39003 events before the first
//! 39000 pins a signer; after a warm-cache restart the pinned map is loaded
//! and quarantine is irrelevant. Persisting quarantine would add complexity
//! with no security benefit.

use std::collections::{BTreeMap, VecDeque};

use nmp_store::{DomainHandle, EventStore, StoreError};

use crate::group_id::{GroupId, RelayUrl};

/// Domain namespace for the durable NIP-29 TOFU signer cache.
const TOFU_NAMESPACE: &str = "nmp.nip29.tofu_signer";
/// Key prefix byte for per-group pinned-signer rows.
const PREFIX_PINNED: u8 = b'p';
/// Key prefix byte for per-host NIP-11 declared-pubkey rows.
const PREFIX_NIP11: u8 = b'n';

pub struct TofuSignerCache {
    /// Per-group pinned signer (the pubkey we accepted in the first 39000).
    pinned: BTreeMap<GroupId, String>,
    /// Per-host NIP-11 declared pubkey (policy A: strict match).
    nip11_pubkey: BTreeMap<RelayUrl, String>,
    /// Quarantine buffer: 39001/39002/39003 events held until a 39000 lands.
    /// Not persisted — transient cold-start buffer only.
    quarantine: BTreeMap<GroupId, VecDeque<QuarantinedEvent>>,
    /// Durable domain handle. `None` in the pure in-memory variant (tests).
    /// `Some` in the persistent variant opened via [`Self::open`].
    domain: Option<DomainHandle>,
}

impl Default for TofuSignerCache {
    fn default() -> Self {
        Self {
            pinned: BTreeMap::new(),
            nip11_pubkey: BTreeMap::new(),
            quarantine: BTreeMap::new(),
            domain: None,
        }
    }
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
    /// Construct a pure in-memory cache (no persistence). For tests and
    /// contexts that have no durable store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a store-backed, durable cache.
    ///
    /// Loads existing pinned-signer and NIP-11 state from the
    /// `nmp.nip29.tofu_signer` domain namespace; subsequent mutations that
    /// establish or update pins write through to the store immediately.
    ///
    /// Single-writer per D4: the caller serialises access via whatever
    /// synchronisation primitive owns the returned struct.
    ///
    /// # Errors
    ///
    /// Returns `StoreError` if the domain namespace cannot be opened or if the
    /// startup scan fails.
    pub fn open(store: &dyn EventStore) -> Result<Self, StoreError> {
        let domain = store.domain_open(TOFU_NAMESPACE)?;
        let mut cache = Self {
            domain: Some(domain),
            ..Default::default()
        };
        cache.load()?;
        Ok(cache)
    }

    /// Record NIP-11-declared pubkey for a host. When present, policy A
    /// (strict match) is active for the host's metadata events.
    pub fn set_nip11_pubkey(&mut self, host: impl Into<RelayUrl>, pubkey: impl Into<String>) {
        let host = host.into();
        let pubkey = pubkey.into();
        self.persist_nip11(&host, &pubkey);
        self.nip11_pubkey.insert(host, pubkey);
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
            self.persist_pinned(group, signer_pubkey);
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

    // ── Persistence helpers ───────────────────────────────────────────────────

    /// Load pinned-signer and NIP-11 rows from the domain store into memory.
    fn load(&mut self) -> Result<(), StoreError> {
        let rows = match &self.domain {
            Some(d) => d.scan_prefix(b"")?.collect::<Result<Vec<_>, _>>()?,
            None => return Ok(()),
        };
        for (key, val) in rows {
            match key.first().copied() {
                Some(PREFIX_PINNED) if key.len() > 2 => {
                    // key: [PREFIX_PINNED, 0x00, ...host_relay_url..., 0x00, ...local_id...]
                    let rest = &key[2..]; // skip [prefix, 0x00]
                    if let Some(sep) = rest.iter().position(|&b| b == 0) {
                        if let (Ok(host), Ok(local), Ok(pk)) = (
                            std::str::from_utf8(&rest[..sep]),
                            std::str::from_utf8(&rest[sep + 1..]),
                            std::str::from_utf8(&val),
                        ) {
                            if !host.is_empty() && !local.is_empty() && !pk.is_empty() {
                                self.pinned.insert(GroupId::new(host, local), pk.to_string());
                            }
                        }
                    }
                }
                Some(PREFIX_NIP11) if key.len() > 2 => {
                    // key: [PREFIX_NIP11, 0x00, ...relay_url...]
                    let rest = &key[2..]; // skip [prefix, 0x00]
                    if let (Ok(relay), Ok(pk)) =
                        (std::str::from_utf8(rest), std::str::from_utf8(&val))
                    {
                        if !relay.is_empty() && !pk.is_empty() {
                            self.nip11_pubkey.insert(relay.to_string(), pk.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Write-through: persist a newly-pinned signer to the domain store.
    fn persist_pinned(&self, group: &GroupId, pubkey: &str) {
        if let Some(d) = &self.domain {
            let _ = d.put(&pinned_key(group), pubkey.as_bytes());
        }
    }

    /// Write-through: persist a NIP-11 declared pubkey to the domain store.
    fn persist_nip11(&self, relay: &str, pubkey: &str) {
        if let Some(d) = &self.domain {
            let _ = d.put(&nip11_key(relay), pubkey.as_bytes());
        }
    }
}

/// Build the domain key for a pinned-signer row.
///
/// Format: `[PREFIX_PINNED, 0x00, host_relay_url_bytes, 0x00, local_id_bytes]`
/// Both relay URLs and local IDs are ASCII-safe with no embedded null bytes.
fn pinned_key(group: &GroupId) -> Vec<u8> {
    let mut k =
        Vec::with_capacity(2 + group.host_relay_url.len() + 1 + group.local_id.len());
    k.push(PREFIX_PINNED);
    k.push(0u8);
    k.extend_from_slice(group.host_relay_url.as_bytes());
    k.push(0u8);
    k.extend_from_slice(group.local_id.as_bytes());
    k
}

/// Build the domain key for a NIP-11 declared-pubkey row.
///
/// Format: `[PREFIX_NIP11, 0x00, relay_url_bytes]`
fn nip11_key(relay_url: &str) -> Vec<u8> {
    let mut k = Vec::with_capacity(2 + relay_url.len());
    k.push(PREFIX_NIP11);
    k.push(0u8);
    k.extend_from_slice(relay_url.as_bytes());
    k
}

#[cfg(test)]
#[path = "tofu/tests.rs"]
mod tests;
