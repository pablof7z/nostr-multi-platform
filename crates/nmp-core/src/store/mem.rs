//! In-memory `EventStore` backend.
//!
//! Used for tests and the pre-M15 web target. Every method is fully implemented
//! against a `Mutex<MemState>` so tests cover the same logic that the LMDB
//! backend will eventually call.
//!
//! See `docs/design/lmdb/trait.md` §5 ("Two backends in v1").

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Shared storage map for a single domain namespace.
type DomainMap = Arc<Mutex<HashMap<Vec<u8>, Vec<u8>>>>;

use super::events::{DomainHandle, DomainHandleInner, EventIter, EventStore};
use super::types::{
    ClaimerId, Coverage, DeleteFilter, DumpFormat, DumpStats, EventId, GcBudget, GcReport,
    InsertOutcome, ProvenanceEntry, PubKey, RawEvent, RejectReason, RelayUrl, StoredEvent,
    TombstoneOrigin, TombstoneRow, WatermarkKey, WatermarkRow,
};
use super::StoreError;
use crate::substrate::DomainMigration;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Default maximum pinned events per view.
const DEFAULT_VIEW_CEILING: usize = 1_000;

/// Maximum provenance entries kept per event.
const MAX_PROVENANCE_ENTRIES: usize = 32;

/// Tombstones older than this many seconds are purged by `gc_step`.
const TOMBSTONE_MAX_AGE_SECS: u64 = 90 * 24 * 3600; // 90 days

// ─── Inner state ─────────────────────────────────────────────────────────────

struct MemState {
    /// Primary event store: hex id → StoredEvent.
    events: HashMap<String, StoredEvent>,

    /// Tombstone rows: hex target_id → TombstoneRow.
    tombstones: HashMap<String, TombstoneRow>,

    /// Address tombstones (kind:5 `a`-tag): "kind:pubkey:dtag" → TombstoneRow.
    addr_tombstones: HashMap<String, TombstoneRow>,

    /// Provenance: hex event_id → sorted Vec<ProvenanceEntry>.
    provenance: HashMap<String, Vec<ProvenanceEntry>>,

    /// Watermarks: (filter_hash_hex, relay_url) → WatermarkRow.
    watermarks: HashMap<(String, String), WatermarkRow>,

    /// Domain data per namespace.
    domain_data: HashMap<&'static str, DomainMap>,

    /// Domain schema versions.
    domain_versions: HashMap<&'static str, u32>,

    /// Claim budgets: claimer → max pinned.
    claim_budgets: HashMap<ClaimerId, usize>,

    /// Current claims: claimer → set of hex event ids.
    claims: HashMap<ClaimerId, Vec<String>>,
}

impl MemState {
    fn new() -> Self {
        Self {
            events: HashMap::new(),
            tombstones: HashMap::new(),
            addr_tombstones: HashMap::new(),
            provenance: HashMap::new(),
            watermarks: HashMap::new(),
            domain_data: HashMap::new(),
            domain_versions: HashMap::new(),
            claim_budgets: HashMap::new(),
            claims: HashMap::new(),
        }
    }

    #[allow(dead_code)] // Available for future dump/debug helpers.
    fn events_sorted_newest_first(&self) -> Vec<&StoredEvent> {
        let mut v: Vec<&StoredEvent> = self.events.values().collect();
        v.sort_by(|a, b| {
            b.raw.created_at.cmp(&a.raw.created_at)
                .then(a.raw.id.cmp(&b.raw.id))
        });
        v
    }
}

// ─── MemEventStore ────────────────────────────────────────────────────────────

/// Fully in-memory `EventStore` implementation.
pub struct MemEventStore {
    state: Mutex<MemState>,
}

impl MemEventStore {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(MemState::new()),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, MemState>, StoreError> {
        self.state.lock().map_err(|e| StoreError::Io(e.to_string()))
    }
}

impl Default for MemEventStore {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Provenance helpers ───────────────────────────────────────────────────────

fn sort_provenance(entries: &mut [ProvenanceEntry]) {
    entries.sort_by(|a, b| {
        a.first_seen_ms.cmp(&b.first_seen_ms)
            .then(a.relay_url.cmp(&b.relay_url))
    });
    for (i, e) in entries.iter_mut().enumerate() {
        e.primary = i == 0;
    }
}

fn upsert_provenance(
    entries: &mut Vec<ProvenanceEntry>,
    relay_url: RelayUrl,
    received_at_ms: u64,
) {
    // Update existing entry if present.
    if let Some(e) = entries.iter_mut().find(|e| e.relay_url == relay_url) {
        if received_at_ms < e.first_seen_ms {
            e.first_seen_ms = received_at_ms;
        }
        if received_at_ms > e.last_seen_ms {
            e.last_seen_ms = received_at_ms;
        }
        sort_provenance(entries);
        return;
    }

    // If at capacity, overwrite the oldest non-primary entry.
    if entries.len() >= MAX_PROVENANCE_ENTRIES {
        // Primary is entries[0] after sort; replace oldest non-primary by last_seen_ms.
        if let Some(oldest) = entries.iter_mut().skip(1)
            .min_by_key(|e| e.last_seen_ms)
        {
            *oldest = ProvenanceEntry {
                relay_url,
                first_seen_ms: received_at_ms,
                last_seen_ms: received_at_ms,
                primary: false,
            };
            sort_provenance(entries);
            return;
        }
    }

    entries.push(ProvenanceEntry {
        relay_url,
        first_seen_ms: received_at_ms,
        last_seen_ms: received_at_ms,
        primary: false,
    });
    sort_provenance(entries);
}

// ─── EventStore impl ─────────────────────────────────────────────────────────

impl EventStore for MemEventStore {
    // ─── Reads ───────────────────────────────────────────────────────────────

    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError> {
        let hex = bytes_to_hex(id);
        let st = self.lock()?;
        Ok(st.events.get(&hex).cloned())
    }

    fn scan_by_author_kind<'a>(
        &'a self,
        author: &PubKey,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        let author_hex = bytes_to_hex(author);
        let st = self.lock()?;
        let mut results: Vec<StoredEvent> = st.events.values()
            .filter(|ev| {
                ev.raw.pubkey == author_hex
                    && kinds.contains(&ev.raw.kind)
                    && since.is_none_or(|s| ev.raw.created_at >= s)
                    && until.is_none_or(|u| ev.raw.created_at <= u)
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| {
            b.raw.created_at.cmp(&a.raw.created_at).then(a.raw.id.cmp(&b.raw.id))
        });
        results.truncate(limit);
        Ok(Box::new(results.into_iter().map(Ok)))
    }

    fn get_param_replaceable(
        &self,
        pubkey: &PubKey,
        kind: u32,
        d_tag: &[u8],
    ) -> Result<Option<StoredEvent>, StoreError> {
        let pubkey_hex = bytes_to_hex(pubkey);
        let d_str = String::from_utf8_lossy(d_tag).into_owned();
        let st = self.lock()?;
        let winner = st.events.values()
            .filter(|ev| {
                ev.raw.pubkey == pubkey_hex
                    && ev.raw.kind == kind
                    && ev.raw.d_tag()
                        .map(|d| String::from_utf8_lossy(&d).into_owned() == d_str)
                        .unwrap_or(false)
            })
            .max_by(|a, b| {
                a.raw.created_at.cmp(&b.raw.created_at)
                    .then(b.raw.id.cmp(&a.raw.id))
            })
            .cloned();
        Ok(winner)
    }

    fn scan_by_kind_dtag<'a>(
        &'a self,
        kind: u32,
        d_tag: &[u8],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        let d_str = String::from_utf8_lossy(d_tag).into_owned();
        let st = self.lock()?;
        let mut results: Vec<StoredEvent> = st.events.values()
            .filter(|ev| {
                ev.raw.kind == kind
                    && ev.raw.d_tag()
                        .map(|d| String::from_utf8_lossy(&d).into_owned() == d_str)
                        .unwrap_or(false)
                    && since.is_none_or(|s| ev.raw.created_at >= s)
                    && until.is_none_or(|u| ev.raw.created_at <= u)
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| {
            b.raw.created_at.cmp(&a.raw.created_at).then(a.raw.id.cmp(&b.raw.id))
        });
        results.truncate(limit);
        Ok(Box::new(results.into_iter().map(Ok)))
    }

    fn scan_by_etag<'a>(
        &'a self,
        target: &EventId,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        let target_hex = bytes_to_hex(target);
        let st = self.lock()?;
        let mut results: Vec<StoredEvent> = st.events.values()
            .filter(|ev| {
                kinds.contains(&ev.raw.kind)
                    && ev.raw.e_tags().contains(&target_hex)
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| {
            b.raw.created_at.cmp(&a.raw.created_at).then(a.raw.id.cmp(&b.raw.id))
        });
        results.truncate(limit);
        Ok(Box::new(results.into_iter().map(Ok)))
    }

    fn scan_by_ptag<'a>(
        &'a self,
        target: &PubKey,
        kinds: &[u32],
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        let target_hex = bytes_to_hex(target);
        let st = self.lock()?;
        let mut results: Vec<StoredEvent> = st.events.values()
            .filter(|ev| {
                kinds.contains(&ev.raw.kind)
                    && ev.raw.p_tags().contains(&target_hex)
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| {
            b.raw.created_at.cmp(&a.raw.created_at).then(a.raw.id.cmp(&b.raw.id))
        });
        results.truncate(limit);
        Ok(Box::new(results.into_iter().map(Ok)))
    }

    fn scan_by_kind_time<'a>(
        &'a self,
        kinds: &[u32],
        since: Option<u64>,
        until: Option<u64>,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        let st = self.lock()?;
        let mut results: Vec<StoredEvent> = st.events.values()
            .filter(|ev| {
                (kinds.is_empty() || kinds.contains(&ev.raw.kind))
                    && since.is_none_or(|s| ev.raw.created_at >= s)
                    && until.is_none_or(|u| ev.raw.created_at <= u)
            })
            .cloned()
            .collect();
        results.sort_by(|a, b| {
            b.raw.created_at.cmp(&a.raw.created_at).then(a.raw.id.cmp(&b.raw.id))
        });
        results.truncate(limit);
        Ok(Box::new(results.into_iter().map(Ok)))
    }

    fn scan_expiring_before<'a>(
        &'a self,
        unix_seconds: u64,
        limit: usize,
    ) -> Result<Box<dyn EventIter + 'a>, StoreError> {
        let st = self.lock()?;
        // Ascending by expiration.
        let mut pairs: Vec<(u64, StoredEvent)> = st.events.values()
            .filter_map(|ev| {
                ev.raw.expiration()
                    .filter(|&exp| exp < unix_seconds)
                    .map(|exp| (exp, ev.clone()))
            })
            .collect();
        pairs.sort_by_key(|(exp, _)| *exp);
        pairs.truncate(limit);
        Ok(Box::new(pairs.into_iter().map(|(_, ev)| Ok(ev))))
    }

    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError> {
        let hex = bytes_to_hex(target);
        let st = self.lock()?;
        Ok(st.tombstones.get(&hex).cloned().into_iter().collect())
    }

    fn list_tombstones<'a>(
        &'a self,
    ) -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>
    {
        let st = self.lock()?;
        let rows: Vec<TombstoneRow> = st.tombstones.values().cloned().collect();
        Ok(Box::new(rows.into_iter().map(Ok)))
    }

    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError> {
        let hex = bytes_to_hex(id);
        let st = self.lock()?;
        Ok(st.provenance.get(&hex).cloned().unwrap_or_default())
    }

    // ─── Writes ──────────────────────────────────────────────────────────────

    fn insert(
        &self,
        event: RawEvent,
        source: &RelayUrl,
        received_at_ms: u64,
    ) -> Result<InsertOutcome, StoreError> {
        // 1. Structural validation (sig check deferred to nostr crate wiring).
        if !event.is_structurally_valid() {
            return Ok(InsertOutcome::Rejected {
                id: event.id_bytes(),
                reason: RejectReason::Malformed("invalid id/pubkey/sig length".into()),
            });
        }

        // 2. Ephemeral: deliver to live consumers, do not store.
        if event.is_ephemeral() {
            return Ok(InsertOutcome::Ephemeral { id: event.id_bytes() });
        }

        // 3. Check NIP-40 expiration on arrival.
        if let Some(exp) = event.expiration() {
            let now_secs = received_at_ms / 1000;
            if exp <= now_secs {
                return Ok(InsertOutcome::Rejected {
                    id: event.id_bytes(),
                    reason: RejectReason::ExpiredOnArrival,
                });
            }
        }

        let id_bytes = event.id_bytes();
        let id_hex = event.id.clone();
        let mut st = self.lock()?;

        // 4. Check tombstone (per-id).
        if let Some(tomb) = st.tombstones.get(&id_hex) {
            return Ok(InsertOutcome::Tombstoned {
                id: id_bytes,
                kind5_event_id: tomb.kind5_event_id,
                origin: tomb.origin,
            });
        }

        // 5. Check address tombstone for parameterized replaceables.
        if event.is_param_replaceable() {
            if let Some(d) = event.d_tag() {
                let d_str = String::from_utf8_lossy(&d).into_owned();
                let addr_key = format!("{}:{}:{}", event.kind, event.pubkey, d_str);
                if let Some(tomb) = st.addr_tombstones.get(&addr_key) {
                    // Only suppress if the kind:5 is newer than (or equal to) this event.
                    if tomb.deleted_at >= event.created_at {
                        return Ok(InsertOutcome::Tombstoned {
                            id: id_bytes,
                            kind5_event_id: tomb.kind5_event_id,
                            origin: tomb.origin,
                        });
                    }
                }
            }
        }

        // 6. Kind:5 handling (self-deletes only — foreign kind:5 is stored but ignored).
        if event.kind == 5 {
            return handle_kind5_insert(&mut st, event, source, received_at_ms);
        }

        // 7. Replaceable supersession.
        if event.is_replaceable() {
            return handle_replaceable_insert(&mut st, event, source, received_at_ms);
        }

        // 8. Parameterized replaceable.
        if event.is_param_replaceable() {
            return handle_param_replaceable_insert(&mut st, event, source, received_at_ms);
        }

        // 9. Normal insert / duplicate.
        handle_normal_insert(&mut st, event, source, received_at_ms)
    }

    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError> {
        let mut st = self.lock()?;
        let ids_to_remove: Vec<String> = match &filter {
            DeleteFilter::ByRelayOnly(relay) => {
                // Remove events where the only provenance source is this relay.
                st.events.keys()
                    .filter(|id| {
                        st.provenance.get(*id)
                            .map(|p| p.len() == 1 && p[0].relay_url == *relay)
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect()
            }
            DeleteFilter::ByAuthor(pk) => {
                let pk_hex = bytes_to_hex(pk);
                st.events.iter()
                    .filter(|(_, ev)| ev.raw.pubkey == pk_hex)
                    .map(|(id, _)| id.clone())
                    .collect()
            }
            DeleteFilter::ByIds(ids) => {
                ids.iter().map(|id| bytes_to_hex(id)).filter(|h| st.events.contains_key(h)).collect()
            }
            DeleteFilter::ByKindRange { lo, hi } => {
                st.events.iter()
                    .filter(|(_, ev)| ev.raw.kind >= *lo && ev.raw.kind <= *hi)
                    .map(|(id, _)| id.clone())
                    .collect()
            }
        };
        let count = ids_to_remove.len();
        for id in ids_to_remove {
            st.events.remove(&id);
            st.provenance.remove(&id);
        }
        Ok(count)
    }

    // ─── Watermarks ──────────────────────────────────────────────────────────

    fn read_watermark(&self, key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError> {
        let st = self.lock()?;
        let wm_key = (bytes_to_hex(&key.filter_hash), key.relay_url.clone());
        Ok(st.watermarks.get(&wm_key).cloned())
    }

    fn write_watermark(&self, row: WatermarkRow) -> Result<(), StoreError> {
        let mut st = self.lock()?;
        let wm_key = (bytes_to_hex(&row.key.filter_hash), row.key.relay_url.clone());
        st.watermarks.insert(wm_key, row);
        Ok(())
    }

    fn coverage(&self, key: &WatermarkKey) -> Result<Coverage, StoreError> {
        let row = self.read_watermark(key)?;
        let Some(row) = row else {
            return Ok(Coverage::Unknown);
        };
        // Default staleness window: 300 seconds.
        let staleness_window = 300u64;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let age = now.saturating_sub(row.updated_at);
        if age <= staleness_window {
            Ok(Coverage::CompleteAsOf(row.synced_up_to))
        } else {
            Ok(Coverage::PartialUpTo(row.synced_up_to))
        }
    }

    fn list_watermarks_for_relay<'a>(
        &'a self,
        relay_url: &str,
    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>
    {
        let st = self.lock()?;
        let rows: Vec<WatermarkRow> = st.watermarks.values()
            .filter(|r| r.key.relay_url == relay_url)
            .cloned()
            .collect();
        Ok(Box::new(rows.into_iter().map(Ok)))
    }

    // ─── Hot-set / claims ────────────────────────────────────────────────────

    fn register_view_cover(
        &self,
        claimer: ClaimerId,
        cover_budget: usize,
    ) -> Result<(), StoreError> {
        let mut st = self.lock()?;
        st.claim_budgets.insert(claimer, cover_budget);
        Ok(())
    }

    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError> {
        let mut st = self.lock()?;
        let ceiling = *st.claim_budgets.get(&claimer).unwrap_or(&DEFAULT_VIEW_CEILING);
        let current = st.claims.get(&claimer).map(|v| v.len()).unwrap_or(0);
        let requested = current + ids.len();
        if requested > ceiling {
            return Err(StoreError::OverPinned { claimer, requested, ceiling });
        }
        let entry = st.claims.entry(claimer).or_default();
        for id in ids {
            let hex = bytes_to_hex(id);
            if !entry.contains(&hex) {
                entry.push(hex);
            }
        }
        Ok(())
    }

    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError> {
        let mut st = self.lock()?;
        st.claims.remove(&claimer);
        Ok(())
    }

    fn hot_set_hint(&self, _ids: &[EventId]) -> Result<(), StoreError> {
        // Memory backend has no LRU — all events are equally hot. No-op.
        Ok(())
    }

    fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError> {
        let start = std::time::Instant::now();
        let mut st = self.lock()?;
        let mut report = GcReport::default();

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let now_secs = now_ms / 1000;

        // Reap NIP-40 expired events.
        let expired_ids: Vec<String> = st.events.iter()
            .filter(|(_, ev)| ev.raw.expiration().is_some_and(|exp| exp <= now_secs))
            .map(|(id, _)| id.clone())
            .take(budget.max_events_per_step)
            .collect();

        for id_hex in &expired_ids {
            if let Some(ev) = st.events.remove(id_hex) {
                st.provenance.remove(id_hex);
                st.tombstones.insert(id_hex.clone(), TombstoneRow {
                    target_id: ev.raw.id_bytes(),
                    kind5_event_id: None,
                    deleter_pubkey: None,
                    deleted_at: now_secs,
                    sources: vec![],
                    origin: TombstoneOrigin::NIP40Expiry,
                });
                report.expired_reaped += 1;
            }
            if start.elapsed().as_millis() as u32 >= budget.max_duration_ms {
                break;
            }
        }

        // Purge tombstones older than TOMBSTONE_MAX_AGE_SECS.
        let stale_tombstones: Vec<String> = st.tombstones.iter()
            .filter(|(_, t)| now_secs.saturating_sub(t.deleted_at) > TOMBSTONE_MAX_AGE_SECS)
            .map(|(k, _)| k.clone())
            .collect();
        report.tombstones_purged = stale_tombstones.len();
        for k in stale_tombstones {
            st.tombstones.remove(&k);
        }

        report.duration_ms = start.elapsed().as_millis() as u32;
        Ok(report)
    }

    // ─── Domain rows ─────────────────────────────────────────────────────────

    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle, StoreError> {
        let mut st = self.lock()?;
        let data = st.domain_data
            .entry(namespace)
            .or_insert_with(|| Arc::new(Mutex::new(HashMap::new())))
            .clone();
        Ok(DomainHandle {
            inner: DomainHandleInner::Mem { namespace, data },
        })
    }

    fn run_migrations(
        &self,
        namespace: &'static str,
        target_version: u32,
        migrations: &[DomainMigration],
    ) -> Result<(), StoreError> {
        let mut st = self.lock()?;
        let current = *st.domain_versions.get(namespace).unwrap_or(&0);

        if current > target_version {
            return Err(StoreError::SchemaTooNew {
                namespace: namespace.to_string(),
                on_disk: current,
                expected: target_version,
            });
        }

        if current == target_version {
            return Ok(());
        }

        // Get or create domain data arc.
        let data_arc = st.domain_data
            .entry(namespace)
            .or_insert_with(|| Arc::new(Mutex::new(HashMap::new())))
            .clone();

        // Apply migrations in order.
        for m in migrations {
            if m.from_version < current || m.from_version >= target_version {
                continue;
            }
            let mut tx = crate::substrate::MigrationTx::default();
            (m.apply)(&mut tx).map_err(|reason| StoreError::MigrationFailed {
                namespace: namespace.to_string(),
                from: m.from_version,
                to: m.to_version,
                reason,
            })?;
            let mut data = data_arc.lock().map_err(|e| StoreError::Io(e.to_string()))?;
            for (k, v) in tx.writes() {
                data.insert(k.clone(), v.clone());
            }
        }

        st.domain_versions.insert(namespace, target_version);
        Ok(())
    }

    // ─── Export ──────────────────────────────────────────────────────────────

    fn dump(
        &self,
        out: &mut dyn std::io::Write,
        format: DumpFormat,
    ) -> Result<DumpStats, StoreError> {
        if !matches!(format, DumpFormat::Jsonl) {
            return Err(StoreError::Io("CBOR dump not yet implemented".into()));
        }

        let st = self.lock()?;
        let mut stats = DumpStats::default();

        // Dump events in deterministic order (ascending hex id).
        let mut event_ids: Vec<&String> = st.events.keys().collect();
        event_ids.sort();
        for id in event_ids {
            let ev = &st.events[id];
            let line = serde_json::json!({
                "type": "event",
                "event": *ev.raw,
                "received_at_ms": ev.received_at_ms,
            })
            .to_string();
            let bytes = (line + "\n").into_bytes();
            stats.bytes_written += bytes.len() as u64;
            out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
            stats.events += 1;
        }

        // Dump tombstones in deterministic order.
        let mut tomb_ids: Vec<&String> = st.tombstones.keys().collect();
        tomb_ids.sort();
        for id in tomb_ids {
            let t = &st.tombstones[id];
            let line = serde_json::json!({
                "type": "tombstone",
                "target_id": bytes_to_hex(&t.target_id),
                "deleted_at": t.deleted_at,
                "origin": format!("{:?}", t.origin),
            })
            .to_string();
            let bytes = (line + "\n").into_bytes();
            stats.bytes_written += bytes.len() as u64;
            out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
            stats.tombstones += 1;
        }

        // Dump watermarks in deterministic order.
        let mut wm_keys: Vec<&(String, String)> = st.watermarks.keys().collect();
        wm_keys.sort();
        for k in wm_keys {
            let r = &st.watermarks[k];
            let line = serde_json::json!({
                "type": "watermark",
                "filter_hash": &r.key.filter_hash.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                "relay_url": &r.key.relay_url,
                "synced_up_to": r.synced_up_to,
            })
            .to_string();
            let bytes = (line + "\n").into_bytes();
            stats.bytes_written += bytes.len() as u64;
            out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
            stats.watermarks += 1;
        }

        // Dump domain rows in deterministic order (namespace, key).
        let mut ns_list: Vec<&&'static str> = st.domain_data.keys().collect();
        ns_list.sort();
        for ns in ns_list {
            let data = st.domain_data[ns].lock().map_err(|e| StoreError::Io(e.to_string()))?;
            let mut pairs: Vec<(&Vec<u8>, &Vec<u8>)> = data.iter().collect();
            pairs.sort_by_key(|(k, _)| *k);
            for (k, v) in pairs {
                let line = serde_json::json!({
                    "type": "domain",
                    "namespace": ns,
                    "key": k,
                    "value": v,
                })
                .to_string();
                let bytes = (line + "\n").into_bytes();
                stats.bytes_written += bytes.len() as u64;
                out.write_all(&bytes).map_err(|e| StoreError::Io(e.to_string()))?;
                stats.domain_rows += 1;
            }
        }

        Ok(stats)
    }
}

// ─── Insert helpers ───────────────────────────────────────────────────────────

fn handle_normal_insert(
    st: &mut MemState,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
) -> Result<InsertOutcome, StoreError> {
    let id_bytes = event.id_bytes();
    let id_hex = event.id.clone();

    if let Some(_existing) = st.events.get(&id_hex) {
        // Duplicate: merge provenance only.
        let p = st.provenance.entry(id_hex.clone()).or_default();
        upsert_provenance(p, source.clone(), received_at_ms);
        let sources_after = p.len() as u32;
        return Ok(InsertOutcome::Duplicate { id: id_bytes, sources_after });
    }

    let stored = StoredEvent {
        raw: Arc::new(event),
        received_at_ms,
    };
    st.events.insert(id_hex.clone(), stored);

    let p = st.provenance.entry(id_hex).or_default();
    upsert_provenance(p, source.clone(), received_at_ms);
    let sources_after = p.len() as u32;

    Ok(InsertOutcome::Inserted { id: id_bytes, sources_after })
}

fn handle_replaceable_insert(
    st: &mut MemState,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
) -> Result<InsertOutcome, StoreError> {
    let id_bytes = event.id_bytes();
    let id_hex = event.id.clone();
    let pubkey_hex = event.pubkey.clone();
    let kind = event.kind;

    // Find existing replaceable for this (pubkey, kind).
    let existing_id: Option<String> = st.events.iter()
        .filter(|(_, ev)| ev.raw.pubkey == pubkey_hex && ev.raw.kind == kind)
        .max_by(|(_, a), (_, b)| {
            a.raw.created_at.cmp(&b.raw.created_at)
                .then(b.raw.id.cmp(&a.raw.id))
        })
        .map(|(id, _)| id.clone());

    if let Some(ref existing_hex) = existing_id {
        let existing_ev = &st.events[existing_hex];
        let existing_time = existing_ev.raw.created_at;
        let existing_id_str = existing_ev.raw.id.clone();

        // Determine winner: newer created_at wins; tie → smaller id wins.
        let incoming_wins = event.created_at > existing_time
            || (event.created_at == existing_time && event.id < existing_id_str);

        if incoming_wins {
            // Remove old event.
            let replaced_id = hex_to_bytes32_owned(existing_hex);
            st.events.remove(existing_hex);
            st.provenance.remove(existing_hex);

            // Insert new.
            let new_id = id_bytes;
            let stored = StoredEvent { raw: Arc::new(event), received_at_ms };
            st.events.insert(id_hex.clone(), stored);
            let p = st.provenance.entry(id_hex).or_default();
            upsert_provenance(p, source.clone(), received_at_ms);

            Ok(InsertOutcome::Replaced { new_id, replaced_id })
        } else {
            // Incoming is older — drop it.
            let current_id = hex_to_bytes32_owned(existing_hex);
            Ok(InsertOutcome::Superseded { id: id_bytes, current_id })
        }
    } else {
        // No existing — fresh insert.
        let stored = StoredEvent { raw: Arc::new(event), received_at_ms };
        st.events.insert(id_hex.clone(), stored);
        let p = st.provenance.entry(id_hex).or_default();
        upsert_provenance(p, source.clone(), received_at_ms);
        let sources_after = p.len() as u32;
        Ok(InsertOutcome::Inserted { id: id_bytes, sources_after })
    }
}

fn handle_param_replaceable_insert(
    st: &mut MemState,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
) -> Result<InsertOutcome, StoreError> {
    let id_bytes = event.id_bytes();
    let id_hex = event.id.clone();
    let pubkey_hex = event.pubkey.clone();
    let kind = event.kind;
    let d_tag = event.d_tag().unwrap_or_default();
    let d_str = String::from_utf8_lossy(&d_tag).into_owned();

    // Find existing parameterized replaceable for (pubkey, kind, d_tag).
    let existing_id: Option<String> = st.events.iter()
        .filter(|(_, ev)| {
            ev.raw.pubkey == pubkey_hex
                && ev.raw.kind == kind
                && ev.raw.d_tag()
                    .map(|d| String::from_utf8_lossy(&d).into_owned() == d_str)
                    .unwrap_or(false)
        })
        .max_by(|(_, a), (_, b)| {
            a.raw.created_at.cmp(&b.raw.created_at)
                .then(b.raw.id.cmp(&a.raw.id))
        })
        .map(|(id, _)| id.clone());

    if let Some(ref existing_hex) = existing_id {
        let existing_ev = &st.events[existing_hex];
        let existing_time = existing_ev.raw.created_at;
        let existing_id_str = existing_ev.raw.id.clone();

        let incoming_wins = event.created_at > existing_time
            || (event.created_at == existing_time && event.id < existing_id_str);

        if incoming_wins {
            let replaced_id = hex_to_bytes32_owned(existing_hex);
            st.events.remove(existing_hex);
            st.provenance.remove(existing_hex);

            let new_id = id_bytes;
            let stored = StoredEvent { raw: Arc::new(event), received_at_ms };
            st.events.insert(id_hex.clone(), stored);
            let p = st.provenance.entry(id_hex).or_default();
            upsert_provenance(p, source.clone(), received_at_ms);

            Ok(InsertOutcome::Replaced { new_id, replaced_id })
        } else {
            let current_id = hex_to_bytes32_owned(existing_hex);
            Ok(InsertOutcome::Superseded { id: id_bytes, current_id })
        }
    } else {
        let stored = StoredEvent { raw: Arc::new(event), received_at_ms };
        st.events.insert(id_hex.clone(), stored);
        let p = st.provenance.entry(id_hex).or_default();
        upsert_provenance(p, source.clone(), received_at_ms);
        let sources_after = p.len() as u32;
        Ok(InsertOutcome::Inserted { id: id_bytes, sources_after })
    }
}

fn handle_kind5_insert(
    st: &mut MemState,
    event: RawEvent,
    source: &RelayUrl,
    received_at_ms: u64,
) -> Result<InsertOutcome, StoreError> {
    let kind5_id_bytes = event.id_bytes();
    let kind5_id_hex = event.id.clone();
    let kind5_pubkey = event.pubkey.clone();
    let kind5_created_at = event.created_at;

    // Process `e`-tag deletes.
    for target_hex in event.e_tags() {
        if let Some(existing) = st.events.get(&target_hex) {
            // Only self-deletes: deleter must own the target.
            if existing.raw.pubkey != kind5_pubkey {
                continue;
            }
            let target_id = existing.raw.id_bytes();
            st.events.remove(&target_hex);
            st.provenance.remove(&target_hex);
            st.tombstones.insert(target_hex, TombstoneRow {
                target_id,
                kind5_event_id: Some(kind5_id_bytes),
                deleter_pubkey: Some(hex_to_bytes32_owned(&kind5_pubkey)),
                deleted_at: kind5_created_at,
                sources: vec![source.clone()],
                origin: TombstoneOrigin::Kind5,
            });
        } else {
            // Target doesn't exist yet — write a pre-emptive tombstone.
            let target_bytes = hex_to_bytes32_owned(&target_hex);
            st.tombstones.entry(target_hex).or_insert(TombstoneRow {
                target_id: target_bytes,
                kind5_event_id: Some(kind5_id_bytes),
                deleter_pubkey: Some(hex_to_bytes32_owned(&kind5_pubkey)),
                deleted_at: kind5_created_at,
                sources: vec![source.clone()],
                origin: TombstoneOrigin::Kind5,
            });
        }
    }

    // Process `a`-tag deletes (parameterized replaceables).
    for addr in event.a_tags() {
        // addr format: "kind:pubkey:dtag"
        let parts: Vec<&str> = addr.splitn(3, ':').collect();
        if parts.len() < 3 { continue; }
        let target_kind_str = parts[0];
        let target_pubkey = parts[1];
        let target_dtag = parts[2];

        // Only self-deletes.
        if target_pubkey != kind5_pubkey { continue; }

        let addr_key = format!("{}:{}:{}", target_kind_str, target_pubkey, target_dtag);
        let Ok(target_kind) = target_kind_str.parse::<u32>() else { continue };

        // Delete any existing matching parameterized replaceable.
        let to_delete: Vec<String> = st.events.iter()
            .filter(|(_, ev)| {
                ev.raw.pubkey == target_pubkey
                    && ev.raw.kind == target_kind
                    && ev.raw.d_tag()
                        .map(|d| String::from_utf8_lossy(&d).into_owned() == target_dtag)
                        .unwrap_or(false)
                    && ev.raw.created_at <= kind5_created_at
            })
            .map(|(id, _)| id.clone())
            .collect();

        for target_hex in to_delete {
            if let Some(existing) = st.events.remove(&target_hex) {
                st.provenance.remove(&target_hex);
                st.tombstones.insert(target_hex, TombstoneRow {
                    target_id: existing.raw.id_bytes(),
                    kind5_event_id: Some(kind5_id_bytes),
                    deleter_pubkey: Some(hex_to_bytes32_owned(&kind5_pubkey)),
                    deleted_at: kind5_created_at,
                    sources: vec![source.clone()],
                    origin: TombstoneOrigin::Kind5,
                });
            }
        }

        // Write address tombstone for events arriving later.
        st.addr_tombstones.entry(addr_key).or_insert(TombstoneRow {
            target_id: [0u8; 32], // address tombstone has no specific target id
            kind5_event_id: Some(kind5_id_bytes),
            deleter_pubkey: Some(hex_to_bytes32_owned(&kind5_pubkey)),
            deleted_at: kind5_created_at,
            sources: vec![source.clone()],
            origin: TombstoneOrigin::Kind5,
        });
    }

    // Store the kind:5 event itself.
    let stored = StoredEvent { raw: Arc::new(event), received_at_ms };
    st.events.insert(kind5_id_hex.clone(), stored);
    let p = st.provenance.entry(kind5_id_hex).or_default();
    upsert_provenance(p, source.clone(), received_at_ms);
    let sources_after = p.len() as u32;

    Ok(InsertOutcome::Inserted { id: kind5_id_bytes, sources_after })
}

// ─── Utilities ────────────────────────────────────────────────────────────────

fn bytes_to_hex(b: &[u8]) -> String {
    b.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hex_to_bytes32_owned(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    if s.len() != 64 { return out; }
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        if i >= 32 { break; }
        if let (Some(&hi), Some(&lo)) = (chunk.first(), chunk.get(1)) {
            out[i] = (hex_nibble(hi) << 4) | hex_nibble(lo);
        }
    }
    out
}

fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}
