Reading additional input from stdin...
2026-05-17T22:45:39.677433Z ERROR codex_core::session: failed to load skill /Users/pablofernandez/.agents/skills/voice-capture-sheet/SKILL.md: invalid YAML: mapping values are not allowed in this context at line 2 column 116
OpenAI Codex v0.129.0 (research preview)
--------
workdir: /Users/pablofernandez/Work/nostr-multi-platform
model: gpt-5.5
provider: openai
approval: never
sandbox: workspace-write [workdir, /tmp, $TMPDIR, /Users/pablofernandez/.codex/memories]
reasoning effort: xhigh
reasoning summaries: none
session id: 019e381d-d4df-7121-8616-c362ec3bdeae
--------
user
You are reviewing merge 9fead0e (M3 LMDB schema + EventStore trait + GC policy design) on master in nostr-multi-platform. Doctrine D0-D5. File size: 300 LOC soft, 500 hard.

M3 goal (per docs/plan.md §M3): swap in-memory EventStore for LMDB; implement full insert invariants (replaceable kinds, kind:5 delete, NIP-40 expiration, dedup w/ provenance merge); claim-based GC; watermark table populated in M4.

Merge:

=== M3 design merge: 9fead0e ===
 docs/design/lmdb-schema.md     | 234 +++++++++++++++++++++++++++++++
 docs/design/lmdb/gc.md         | 195 ++++++++++++++++++++++++++
 docs/design/lmdb/keys.md       | 150 ++++++++++++++++++++
 docs/design/lmdb/tests.md      | 223 +++++++++++++++++++++++++++++
 docs/design/lmdb/trait.md      | 312 +++++++++++++++++++++++++++++++++++++++++
 docs/design/lmdb/watermarks.md | 191 +++++++++++++++++++++++++
 6 files changed, 1305 insertions(+)

9fead0e design(m3): LMDB schema + EventStore trait + GC policy
Adds the M3 design: docs/design/lmdb-schema.md (master decision +
perf budget + open questions) and split sub-docs under
docs/design/lmdb/ for the trait surface, key encodings, GC policy,
watermarks/provenance/migrations, and the test plan mapping every
§7.1 insert invariant to a concrete test file.

Decision: adopt nostr-lmdb as the on-disk byte store, wrap behind a
NMP-owned EventStore trait, add NMP sub-databases for the rows
nostr-lmdb does not model (claim-pinning, watermarks, projection
caches, domain-module rows, secondary indexes for kernel-side queries).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>

---

diff --git a/docs/design/lmdb-schema.md b/docs/design/lmdb-schema.md
new file mode 100644
index 0000000..6b84216
--- /dev/null
+++ b/docs/design/lmdb-schema.md
@@ -0,0 +1,234 @@
+# Design: LMDB schema + EventStore trait + GC policy (M3)
+
+> **Audience:** kernel implementers landing M3 (persistence).
+> **Status:** rev 0 — proposed; opens ADR slot for any open-question resolution.
+> **Companion docs:** [`lmdb/trait.md`](lmdb/trait.md), [`lmdb/keys.md`](lmdb/keys.md), [`lmdb/gc.md`](lmdb/gc.md), [`lmdb/watermarks.md`](lmdb/watermarks.md), [`lmdb/tests.md`](lmdb/tests.md).
+> **Prerequisites:** `docs/product-spec/subsystems.md` §7.1 (insert invariants), `docs/decisions/0003-working-set-memory.md` (GC policy intent), `docs/decisions/0009-app-extension-kernel-boundary.md` (DomainModule storage), `docs/design/kernel-substrate.md` §2 (DomainModule trait).
+> **Plan reference:** [`docs/plan.md`](../plan.md) §M3.
+
+---
+
+## 1. Decision: wrap `nostr-lmdb` behind our own `EventStore` trait
+
+**Adopt `nostr-lmdb` as the on-disk byte store. Wrap it behind the NMP `EventStore` trait. Add NMP-owned LMDB sub-databases for the rows `nostr-lmdb` does not model.**
+
+The competing options were (1) use `nostr-lmdb` directly via its concrete `NostrLMDB` type (or via `nostr_database::NostrEventsDatabase`), (2) wrap behind our own trait, or (3) hand-roll an LMDB layer.
+
+**`nostr-lmdb` gives us** (per `docs.rs/nostr-lmdb`): `save_event(&Event)`, `event_by_id(&EventId)`, `check_id(&EventId) -> DatabaseEventStatus`, `query(Filter) -> Events`, `count(Filter)`, `delete(Filter)`, `wipe()`, `negentropy_items(Filter) -> Vec<(EventId, Timestamp)>`. It owns the page allocator, the LMDB environment, primary by-id store, indexes derived from `Filter`, replaceable / parameterized-replaceable supersession, and NIP-09 delete handling. It is the only mature Rust LMDB store for Nostr events with proven NIP-77 integration; reinventing it is a year of work that we will not recoup.
+
+**What `nostr-lmdb` does *not* model** (the gap that justifies a wrapper):
+
+| Concern | Why `nostr-lmdb` doesn't cover it | Where NMP needs it |
+|---|---|---|
+| Per-relay provenance (which relays delivered each event; first seen / last seen) | Out of scope; the crate models events, not their wire history | `subsystems.md` §7.1 "Duplicate id → merge relay provenance set"; ADR-0007 diagnostics; outbox routing scoring in M2+ |
+| Sync watermarks `(filter_hash, relay) → synced_up_to` | Out of scope; the crate does not know about logical filters or relay identity | `subsystems.md` §7.1 + §7.8; M4 NIP-77 engine needs them to be authoritative |
+| Claim register / release for view-driven GC | Out of scope; the crate has no concept of an "open view" | ADR-0003; current in-memory analogue in `kernel/mod.rs:315` `profile_claims: HashMap<String, BTreeSet<String>>` |
+| Working-set hot/cold split with eviction LRU | LMDB is OS-paged; the crate trusts the kernel page cache | ADR-0003 numeric gate (≤ 100 MB at 100 views / 10k hot) |
+| Kernel-side secondary indexes for query shapes our planner uses (e.g. `(p-tag, timestamp)` desc scan, `(e-tag, timestamp)` desc scan, `(expires_at, event_id)` for NIP-40 wakeups) | The `Filter` API recomputes per call; not optimal for our planner's repeat shapes | Planner cache-coverage queries (§7.2); NIP-40 expiration scheduling (§7.1) |
+| Tombstone-as-row that survives independent of the deleted event | The crate suppresses re-insert via its own delete index; we want it exposed for export / restoring across re-syncs | `subsystems.md` §7.1 kind:5 row "persisted as tombstone so later re-insertion is suppressed" |
+| `DomainModule` rows (drafts, settings, action ledger, projection caches) | Entirely out of scope; the crate stores Nostr `Event` only | ADR-0009 + `kernel-substrate.md` §2 — kernel hosts non-Nostr typed rows |
+| Migrations versioned per namespace | Out of scope | `kernel-substrate.md` §2: `DomainModule::migrations() -> Vec<DomainMigration>` |
+| `nmp dump` deterministic export | Out of scope | M3 exit criteria; M11 cross-app proof |
+
+**Therefore.** `EventStore` is a NMP-owned trait, with one production impl `LmdbEventStore` that holds (a) a `NostrLMDB` for the canonical event store and Nostr-shaped queries, and (b) NMP-owned secondary LMDB sub-databases under the same `lmdb::Environment` for the gap rows. The in-memory backend (`MemEventStore`) remains, both for tests and as the web-pre-M15 fallback. See [`lmdb/trait.md`](lmdb/trait.md) for the exact trait shape and the relayed-vs-owned method split.
+
+**Rejected alternatives.**
+
+- *Use `NostrLMDB` directly, no wrapper.* Loses every gap row above. Forces the kernel actor to know about LMDB transactions and a non-NMP concrete type, breaking the `Box<dyn EventStore>` substitutability M3 requires.
+- *Roll our own.* Reinvents NIP-09 / replaceable handling that `nostr-lmdb` already gets right. ~2,000 LOC of avoidable code with a worse bug surface than upstream.
+- *SQLite-backed `nostr-sdk` store.* Larger memory footprint at our 10k-event hot working set; iOS-disk-WAL fsync cost not justified for this access pattern. Held in reserve for the web port (M15) if IndexedDB OPFS proves unworkable.
+
+## 2. Subsystem ownership map
+
+```
+crates/nmp-core/src/store/
+  mod.rs                — trait re-exports + factory
+  trait.rs              — `EventStore` (see lmdb/trait.md)
+  mem.rs                — in-memory backend (preserved from kernel/mod.rs current state)
+  lmdb/
+    mod.rs              — `LmdbEventStore` orchestrator
+    env.rs              — `lmdb::Environment` + sub-db handles + open()
+    events.rs           — wraps `nostr_lmdb::NostrLMDB`; primary-by-id, replaceable rules, kind:5 handling
+    secondary.rs        — NMP-owned secondary indexes (see lmdb/keys.md §3)
+    provenance.rs       — provenance sub-db (see lmdb/watermarks.md §2)
+    watermarks.rs       — watermark sub-db (see lmdb/watermarks.md §1)
+    claims.rs           — claim register / release + hot-set hints (see lmdb/gc.md §2)
+    gc.rs               — `gc_step()` algorithm (see lmdb/gc.md §3)
+    domain.rs           — per-DomainModule sub-db namespacing + migration runner
+    dump.rs             — `nmp dump` (see §9 below)
+```
+
+Each file is bounded ≤ 300 LOC per AGENTS.md. The trait module is read by the actor; backend modules are read only by the orchestrator.
+
+## 3. EventStore trait
+
+See [`lmdb/trait.md`](lmdb/trait.md) for the exact `pub trait EventStore` signature with all required methods, return types, and the `StoreError` enum. Summary:
+
+- **Reads:** `get_by_id`, `scan_by_author_kind`, `scan_by_kind_dtag`, `scan_by_etag`, `scan_by_ptag`, `scan_by_kind_time`, `scan_expiring_before`. All `scan_*` methods return a streaming `EventIter` so the planner pages without materialising. Cache-coverage queries take a `WatermarkKey` and answer authoritatively.
+- **Writes:** `insert(event, RelayUrl)` returns `InsertOutcome` matching §7.1's table. `delete_by_filter` for foreign-relay cleanups. `tombstones_for` for replay.
+- **Watermarks / sync:** `read_watermark`, `write_watermark`, `list_watermarks_for_relay`.
+- **GC:** `claim(ClaimerId, &[EventId])`, `release(ClaimerId)`, `hot_set_hint(&[EventId])`, `gc_step(GcBudget) -> GcReport`.
+- **Domain rows:** `domain_open(namespace) -> DomainHandle` returns a typed handle; `DomainHandle::put/get/scan_index` is the per-DomainModule API.
+- **Migration:** `run_migrations(&[DomainMigration])` runs at startup, transactional per migration.
+- **Export:** `dump(out: &mut dyn Write, format: DumpFormat) -> Result<DumpStats>`.
+
+**Error semantics.** All methods return `Result<T, StoreError>`. Per doctrine D3, store errors do **not** cross FFI — the actor maps every variant to either (a) a typed `Effect` (e.g. `StoreCorrupt`, surfaces via diagnostics + toast), (b) a `tracing::warn!` log + degraded state, or (c) a panic at startup if the LMDB environment refuses to open. The trait itself uses `Result` since it is internal to the actor process.
+
+## 4. Key encoding
+
+Full byte layout for primary + every secondary in [`lmdb/keys.md`](lmdb/keys.md). At a glance:
+
+- Primary `events`: `event_id[32]` → `Event` (CBOR via `nostr` crate's serialization). Owned by `nostr-lmdb`.
+- Secondary `idx_author_kind`: `pubkey[32] || kind_be[4] || created_at_be[8] || event_id[32]` → empty. NMP-owned.
+- Secondary `idx_kind_dtag`: `kind_be[4] || dtag_len_be[2] || dtag_bytes || pubkey[32]` → `event_id[32]`. NMP-owned. Parameterized replaceable address lookup.
+- Secondary `idx_etag_time`, `idx_ptag_time`: `tag_value[32] || created_at_desc_be[8] || event_id[32]` → empty. NMP-owned. `created_at_desc = u64::MAX - created_at` so a forward LMDB scan is newest-first.
+- Secondary `idx_kind_time`: `kind_be[4] || created_at_desc_be[8] || event_id[32]` → empty.
+- Secondary `idx_expires`: `expires_at_be[8] || event_id[32]` → empty. Scanned by the NIP-40 reaper.
+- `tombstones`: `target_id[32]` → `TombstoneRow { kind5_event_id, deleter_pubkey, deleted_at, sources: Vec<RelayUrl> }` (CBOR).
+
+`created_at_be` is big-endian so byte order matches numeric order; `created_at_desc_be = u64::MAX - created_at` then big-endian for newest-first scans without `MDB_LAST + MDB_PREV`.
+
+All secondaries are maintained inside the same `RwTxn` as the primary write — atomicity is achieved by LMDB transactionality, not by post-hoc reconciliation.
+
+## 5. Watermark table
+
+See [`lmdb/watermarks.md`](lmdb/watermarks.md) for full layout. Row shape (CBOR):
+
+```rust
+struct WatermarkRow {
+  filter_hash: [u8; 32],     // BLAKE3 of canonicalised filter (see watermarks.md §3)
+  relay_url: String,
+  synced_up_to: u64,         // unix seconds
+  last_sync_method: SyncMethod, // Negentropy | ReqScan | Manual
+  last_negentropy_state: Option<Vec<u8>>, // engine-opaque resume blob
+  bytes_saved_vs_req: u64,
+  updated_at: u64,
+}
+```
+
+Key: `filter_hash[32] || relay_url_bytes` (no length prefix needed — relay URL is the variable suffix; lookup uses exact key). Populated by M4 (NIP-77) and consulted by M2's planner (cache-coverage check before issuing backfill REQ). Survives restarts; loaded into the actor on startup as a `HashMap<(filter_hash, relay_url), WatermarkRow>` for hot lookups, with all writes going through `EventStore` for durability.
+
+## 6. Migration plumbing
+
+See [`lmdb/watermarks.md`](lmdb/watermarks.md) §4. A `DomainModule` (per `kernel-substrate.md` §2) declares `const NAMESPACE: &'static str` and `const SCHEMA_VERSION: u32` plus `fn migrations() -> Vec<DomainMigration>`. The store assigns one LMDB sub-database per `(namespace, "data")`, plus one per `(namespace, index_name)` for each declared index. A `_meta` sub-database tracks `(namespace, current_version)`.
+
+The current `ModuleRegistry` (`crates/nmp-core/src/substrate/mod.rs:41`) discards the concrete `M: DomainModule` type after `register_domain::<M>()` returns — only the `ModuleDescriptor` is retained. The store cannot get from a namespace string back to `M::SCHEMA_VERSION` or `M::migrations()` at runtime. M3 adds a `DomainFactories { schema_version: fn() -> u32, migrations: fn() -> Vec<DomainMigration>, indexes: fn() -> Vec<DomainIndex> }` struct attached per descriptor, populated by capturing the `M::*` consts and fns in `fn`-pointer closures at register time. This matches the existing `key_fn: fn(&[u8]) -> Option<Vec<u8>>` pattern in `DomainIndex` (`crates/nmp-core/src/substrate/domain.rs:18`) — no `Box<dyn DomainModule>` and no new trait object-safety constraints on `DomainModule`. The change is additive to the substrate module surface. See [`lmdb/watermarks.md`](lmdb/watermarks.md) §4.1 for the registry-side code shape.
+
+On startup:
+
+1. For every registered `DomainModule`, read its row from `_meta`.
+2. If absent, treat current as 0 and run all migrations from 0 to `SCHEMA_VERSION` in one `RwTxn` per step.
+3. If present and less than `SCHEMA_VERSION`, run the missing steps.
+4. If greater, refuse to start (downgrade not supported); surface as `Effect::DomainSchemaTooNew { namespace }`.
+
+Each `DomainMigration::apply` receives a `MigrationTx` with put/get/delete + index rebuild helpers. Rollback semantics: each migration step is its own LMDB write transaction; failure aborts the step cleanly. If migration N succeeds and N+1 fails, the store stays at version N — the actor refuses to start the affected module and the rest of the kernel runs in degraded mode (the module's actions return `ActionRejection::ModuleUnavailable`).
+
+## 7. GC working-set policy
+
+See [`lmdb/gc.md`](lmdb/gc.md) for the eviction algorithm. Formal statement (matches ADR-0003):
+
+```
+hot_resident = {e | e is in claim_pinned}
+             ∪ {e | e is in open_view_cover}
+             ∪ {e | e is among the ≤10k most-recently-touched events}
+
+cold = stored_events \ hot_resident
+```
+
+`hot_resident` lives in a `lru::LruCache<EventId, Arc<Event>>` capped at the configured hot ceiling (default 10,000) plus an unbounded pinned overlay holding events with non-zero claim count. `cold` lives only on disk; lookup pays one LMDB `get` (memory-mapped — typically already in OS page cache for recently-evicted items).
+
+**Eviction algorithm.** On any insert that pushes the LRU over its ceiling, the oldest non-pinned entry is dropped. `gc_step()` is called periodically by the actor (default every 60 s and on memory pressure callbacks from `MemoryWarningCapability`): it (a) reaps NIP-40 expired events using `idx_expires`, (b) trims the LRU to `target_hot_size`, (c) deletes tombstones older than `tombstone_retention` (default 90 days) whose target event is absent from the store, (d) returns a `GcReport` for diagnostics.
+
+ADR-0003's numbers are preserved as the M3 exit gate (§11 below): ≤ 100 MB working-set at 100 active views / 10k hot events / 1M cached on disk.
+
+## 8. Replaceable + tombstone semantics
+
+The `insert()` path implements exactly the §7.1 invariants:
+
+- **Replaceable (kinds 0, 3, 10000–19999).** Look up the existing event for `(pubkey, kind)` in `idx_author_kind` (most recent suffix). If incoming `created_at` is newer, replace; if equal, keep lexicographically smallest `id`; else drop. Replacement deletes the old primary row and all secondary entries in the same `RwTxn`.
+- **Parameterized replaceable (30000–39999).** Same algorithm keyed on `(pubkey, kind, d-tag)` via `idx_kind_dtag` (which holds `event_id` as value so we don't need a separate `idx_author_kind_dtag`; the dtag prefix is unique per author by Nostr semantics — see [`lmdb/keys.md`](lmdb/keys.md) §3.2 for the per-author scoping note).
+- **Kind:5 self-delete.** Verify signature, scan referenced `e` and `a` tags, for each target `e_id` that is authored by the deleter or whose `a` address matches `(deleter_pubkey, kind, d-tag)`: delete the primary + all secondaries + write the tombstone row. Tombstone timestamp = `max(existing.deleted_at, kind5.created_at)`. Re-insert of the deleted event id is suppressed at insert time by a `tombstones.contains(event_id)` check.
+- **Foreign kind:5.** A kind:5 referencing events not authored by the kind:5's `pubkey` is ignored (per spec) — the event is *still stored* as a kind:5 (so other clients can render it / dedup it), but it has no side effect on the targets. The tombstone row is **not** written.
+- **NIP-40 expiration.** On insert, parse `expiration` tag; if present, write `idx_expires`. On `gc_step()`, scan `idx_expires` for keys with `expires_at_be ≤ now`, delete them like kind:5 (full primary + secondaries + tombstone marker noting `kind: Expired`).
+
+The tombstone schema is in [`lmdb/keys.md`](lmdb/keys.md) §4.
+
+## 9. Provenance: per-row sidecar sub-database
+
+**Decision: separate `provenance` sub-database keyed by `event_id[32]`.** Value is CBOR `ProvenanceRow { sources: Vec<ProvenanceEntry> }` where `ProvenanceEntry = { relay_url, first_seen_ms, last_seen_ms, primary: bool }`.
+
+Rejected: stuffing provenance into the `Event` row. That requires re-serializing the full `Event` on every relay redelivery (high write amplification — popular events arrive 5–20× from the relay fan-out) and forks the `nostr-lmdb` row format, which we explicitly want to keep upstream-compatible. The sidecar is appended cheaply with a single CBOR re-encode of the (typically small) `sources` vector.
+
+On duplicate-id insert (§7.1 row 2), `insert()` does not touch the primary; it only updates the provenance sidecar (`last_seen_ms` bump on the matching `ProvenanceEntry`, or append). The "primary relay" — for outbox-routing scoring (M2) and ADR-0007 diagnostics — is deterministically the first relay observed (`sources[0]` after sort by `first_seen_ms`).
+
+The export format (§ next) includes the provenance row alongside each event so a `nmp dump` round-trip restores it.
+
+## 10. Backup / export format
+
+`nmp dump` writes line-delimited JSON to stdout (or a file). Each line is a single tagged record:
+
+```json
+{"type":"event","event": {...nostr event...},"provenance":[{"relay_url":"wss://relay.primal.net","first_seen_ms":1747000000000,"last_seen_ms":1747001234567,"primary":true}]}
+{"type":"tombstone","target_id":"abc...","kind5_event_id":"def...","deleter_pubkey":"...","deleted_at":1747000000,"sources":["wss://..."]}
+{"type":"watermark","filter_hash":"hex32","relay_url":"wss://...","synced_up_to":1747000000,"last_sync_method":"Negentropy","bytes_saved_vs_req":12345,"updated_at":1747000123}
+{"type":"domain","namespace":"twitter.drafts","schema_version":1,"key_hex":"...","value_b64":"..."}
+```
+
+JSONL is the chosen format because (a) it streams (no holding the full dump in memory; cold-events page in as scanned), (b) it diffs cleanly (one record per line), (c) any line is independently parsable for partial recovery, (d) `jq` works out of the box. Binary CBOR is faster but loses ad-hoc inspectability — JSONL is the right tradeoff for an export format.
+
+`nmp restore` is symmetric: read JSONL, replay through `insert()` for events (so all secondaries are rebuilt from scratch — provenance is restored separately by writing the sidecar row directly after each event), `write_watermark` for watermarks, `DomainHandle::put` for domain rows. Restore is idempotent: replaying the same dump twice produces the same store.
+
+## 11. Test plan
+
+See [`lmdb/tests.md`](lmdb/tests.md) for the full mapping of every spec §7.1 invariant to a concrete test file under `crates/nmp-testing/tests/`. Highlights:
+
+| Invariant (§7.1) | Test file |
+|---|---|
+| Insert API single path | `crates/nmp-testing/tests/store_insert_path.rs` |
+| Signature verification before persist | `crates/nmp-testing/tests/store_invalid_sig.rs` |
+| Duplicate id → merge provenance, keep earliest received_at | `crates/nmp-testing/tests/store_provenance_merge.rs` |
+| Replaceable supersession | `crates/nmp-testing/tests/store_replaceable.rs` |
+| Parameterized replaceable supersession | `crates/nmp-testing/tests/store_param_replaceable.rs` |
+| Kind:5 self-delete persists as tombstone | `crates/nmp-testing/tests/store_kind5_tombstone.rs` |
+| Foreign kind:5 ignored | `crates/nmp-testing/tests/store_kind5_foreign.rs` |
+| NIP-40 expiration scheduled + reaped | `crates/nmp-testing/tests/store_nip40_expiration.rs` |
+| Watermarks survive restart, authoritative cache-miss | `crates/nmp-testing/tests/store_watermarks.rs` |
+| Claim register / release; GC drops un-claimed cold | `crates/nmp-testing/tests/store_gc_claims.rs` |
+| `nmp dump` round-trip is byte-identical for second dump | `crates/nmp-testing/tests/store_dump_roundtrip.rs` |
+| Migration v0→v1 success; rollback on N+1 failure | `crates/nmp-testing/tests/store_domain_migration.rs` |
+| Domain isolation: module A cannot read module B's sub-db | `crates/nmp-testing/tests/store_domain_isolation.rs` |
+| Working-set ≤ 100 MB at 100 views / 10k hot / 1M cached | `crates/nmp-testing/bin/reactivity-bench` (extended scenario) |
+
+## 12. Performance budget
+
+| Gate | Budget | Measurement |
+|---|---|---|
+| Cold-start time-to-first-painted-timeline on iPhone 12 (primed LMDB, last session's events on disk) | ≤ 1.5 s p99 | `firehose-bench live cold_start --device iphone12` |
+| Cold-start time-to-first-painted-timeline on simulator | ≤ 800 ms p99 (looser than device because no thermal envelope) | same harness, simulator scenario |
+| Working-set memory at 100 active views / 10k hot / 1M on disk | ≤ 100 MB resident | Instruments Allocations + `reactivity-bench` working-set scenario |
+| Single `insert()` for an unseen kind:1 with 4 secondaries | ≤ 250 µs p99 on iPhone 12 | criterion bench in `crates/nmp-testing/benches/store_insert.rs` |
+| `scan_by_author_kind` returning 200 newest events | ≤ 5 ms p99 | criterion bench in `crates/nmp-testing/benches/store_scan.rs` |
+| `gc_step()` work-batch ceiling (single call) | ≤ 50 ms total wall time | bounded by `GcBudget { max_events, max_duration_ms }` |
+| `nmp dump` of 1M events | sustained ≥ 50k events/sec on M-series Mac | wall-clock measurement in dump-roundtrip test |
+
+Each gate is measurable; any miss revises the design via an ADR before M3 is declared complete (per `plan.md` §1.6 "no silent endings").
+
+## 13. Open questions for ADR after review
+
+1. **`nostr-lmdb` LMDB environment sharing.** Can we open the same `lmdb::Environment` for both `NostrLMDB`'s sub-databases and our own NMP sub-databases (provenance, watermarks, claims, domain rows)? If yes, we get atomic cross-sub-db transactions for free (a single `RwTxn` covers event + provenance + secondary indexes). If `nostr-lmdb` insists on opening its own `Environment`, we lose that and the insert path needs a two-phase write with crash-recovery logic. Investigate before implementation — may require an upstream PR exposing `Environment` access.
+2. **Watermark `filter_hash` canonicalisation.** Two `Filter`s that are semantically identical but field-ordered differently must hash the same. The canonicalisation rule (likely: sort all tag-value arrays, sort kinds, sort authors, lexicographic field order before BLAKE3) needs to be specified once and shared with the planner so cache-coverage lookups hit. Candidate: a single `fn canonical_filter_hash(&Filter) -> [u8; 32]` in `nmp-core::store::watermarks`.
+3. **Projection cache durability.** Currently in-memory in the existing kernel (`kernel/mod.rs:293` `profiles: HashMap`). Do we persist projection caches as a `DomainModule` or rebuild from events at cold-start? Rebuild is simpler and avoids cache-staleness bugs but adds startup cost; persistence is faster but requires invalidation logic on kind:0 replacement. Recommended default: rebuild on cold-start, measure, decide whether to add the persistence layer in M3.x or M4.
+4. **Domain-module per-record encoding.** CBOR via `serde_cbor` vs serde-json vs bincode. CBOR is upstream-compatible (matches `nostr` crate); bincode is faster but stratifies the format. Default: CBOR for cross-language readability; revisit if benchmarks show >5% insert-time cost.
+5. **iOS keychain-stored encryption-at-rest key for LMDB.** Out of scope for M3 (mentioned for M6 keychain work) but the schema must not assume cleartext-on-disk forever; reserve a `meta` row for `encryption_version: u32` so a future migration can wrap pages.
+6. **`ModuleRegistry::register_domain` API stability.** Adding `DomainFactories` to `ModuleDescriptor` is a non-breaking additive change to the public substrate API (existing callers using only the generic `register_domain::<M>()` continue to compile), but it commits us to keeping `DomainModule::SCHEMA_VERSION` and `DomainModule::migrations` as compile-time-resolvable items rather than object-safe methods. Confirm this with the substrate maintainer before M3 lands — if `DomainModule` is expected to support runtime composition (e.g., plugin loading), we need option (c): the actor passes the live `&[Box<dyn DomainModule>]` to `EventStore::open` instead. Recommended default: stay with `fn`-pointer factories; revisit if a plugin-loading use case appears.
+
+## 14. Citations to current code
+
+- In-memory event store: `crates/nmp-core/src/kernel/mod.rs:294` (`events: HashMap<String, StoredEvent>`), `kernel/mod.rs:46` (`StoredEvent` struct).
+- Insert path under wrap: `crates/nmp-core/src/kernel/ingest.rs:166` (`ingest_profile`), `ingest.rs:235` (`ingest_timeline_event`), `ingest.rs:209` (`ingest_relay_list`).
+- Replaceable supersession (current scattered logic to be centralised in `EventStore::insert`): `kernel/ingest.rs:166-185` (profile replace by `(pubkey, kind)`), `ingest.rs:218-233` (NIP-65 list replace by `(pubkey, 10002)`).
+- Profile claim refcounting (current in-memory analogue of `EventStore::claim/release`): `kernel/mod.rs:315` (`profile_claims: HashMap<String, BTreeSet<String>>`), `kernel/requests.rs:202` (`claim_profile`), `requests.rs:239` (`release_profile`).
+- Substrate `DomainModule` trait the store backs: `crates/nmp-core/src/substrate/domain.rs:1` (current shape, lines 1–49).
+- Module registry the store consumes at startup: `crates/nmp-core/src/substrate/mod.rs:41` (`ModuleRegistry::register_domain`).
+
+The M3 implementation deletes none of the existing files outright — the kernel's `events: HashMap` and `profiles: HashMap` are replaced by calls to `Box<dyn EventStore>` held inside the `Kernel` struct, and the per-field tests (`kernel/tests.rs`) shift to the new trait via `MemEventStore`. No public FFI surface changes.
diff --git a/docs/design/lmdb/gc.md b/docs/design/lmdb/gc.md
new file mode 100644
index 0000000..ce885c3
--- /dev/null
+++ b/docs/design/lmdb/gc.md
@@ -0,0 +1,195 @@
+# LMDB sub-design: GC working-set policy
+
+> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Formalises the hot-resident / cold-on-disk split required by ADR-0003 (`docs/decisions/0003-working-set-memory.md`).
+
+## 1. Definitions
+
+```
+stored_events = every event currently in `events` (primary), not tombstoned
+
+claim_pinned  = ⋃ { ids | ids ∈ claims[claimer] for each registered claimer }
+                where each `claimer` is an open ViewHandle / open ActionHandle
+
+open_view_cover = ⋃ { dependency_target_ids(spec)
+                       | (view_id, spec) ∈ open_views }
+                  computed from the composite reverse-index per ADR-0001
+
+recently_touched = top-N by `last_touched_ms` (default N = 10,000)
+
+hot_resident = claim_pinned ∪ open_view_cover ∪ recently_touched
+cold         = stored_events \ hot_resident
+```
+
+`last_touched_ms` is bumped on every `get_by_id`, on every secondary scan that *materialises* the event body, and on `insert` for a fresh row. Scans that only return ids/timestamps (e.g., the early-filter pass in a view's planner) do **not** bump it — only the construction of a `Delta` payload that needs the body does.
+
+`hot_resident` is stored in memory; `cold` lives only on disk. The store still **knows** about every cold event via secondaries — the reverse index covers both per ADR-0003: "The reverse index indexes both hot and cold events. Lookup returns view ids immediately; event bodies for delta construction load lazily and synchronously via the storage backend."
+
+## 2. Hot data structure
+
+```rust
+pub(crate) struct HotSet {
+    // LRU bounded by `target_hot_size` (default 10,000), evicts non-pinned.
+    lru: lru::LruCache<EventId, Arc<nostr::Event>>,
+    // Strong-pin overlay; refcounted by ClaimerId.
+    pinned: HashMap<EventId, u32>,                   // event_id → refcount
+    // Reverse map for cheap release().
+    by_claimer: HashMap<ClaimerId, SmallVec<[EventId; 8]>>,
+    target_hot_size: usize,
+}
+
+impl HotSet {
+    pub fn claim(&mut self, c: ClaimerId, ids: &[EventId]) {
+        for id in ids {
+            *self.pinned.entry(*id).or_insert(0) += 1;
+        }
+        self.by_claimer.entry(c).or_default().extend_from_slice(ids);
+    }
+
+    pub fn release(&mut self, c: ClaimerId) {
+        if let Some(ids) = self.by_claimer.remove(&c) {
+            for id in ids {
+                if let Some(rc) = self.pinned.get_mut(&id) {
+                    *rc = rc.saturating_sub(1);
+                    if *rc == 0 { self.pinned.remove(&id); }
+                }
+            }
+        }
+    }
+
+    pub fn touch(&mut self, id: EventId, e: Arc<nostr::Event>) {
+        self.lru.put(id, e);                          // bumps LRU
+        self.trim();
+    }
+
+    fn trim(&mut self) {
+        while self.lru.len() > self.target_hot_size {
+            // pop_lru returns oldest; skip pinned ones until we find an evictable.
+            // (LruCache::pop_lru doesn't take a predicate; we rotate.)
+            let mut skipped: SmallVec<[(EventId, Arc<nostr::Event>); 8]> = SmallVec::new();
+            let evicted = loop {
+                match self.lru.pop_lru() {
+                    Some((id, e)) if self.pinned.contains_key(&id) => skipped.push((id, e)),
+                    Some(pair) => break Some(pair),
+                    None => break None,
+                }
+            };
+            for (id, e) in skipped.drain(..) { self.lru.put(id, e); }
+            if evicted.is_none() { break; }           // every entry is pinned
+        }
+    }
+}
+```
+
+`target_hot_size` is set from `AppConfig::hot_event_ceiling` (default 10,000) and may be lowered by `MemoryWarningCapability` events (iOS app suspend or low-memory warning → halve the ceiling, run `gc_step()` once, restore after the warning clears).
+
+## 3. `gc_step()` algorithm
+
+```rust
+pub fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError> {
+    let start = Instant::now();
+    let now_s = unix_now();
+    let mut report = GcReport::default();
+
+    // 3.1 — NIP-40 expired reaper.
+    let to_reap = self.scan_expiring_before(now_s, budget.max_events_per_step)?
+        .collect::<Result<Vec<_>, _>>()?;
+    for ev in to_reap {
+        if start.elapsed().as_millis() as u32 >= budget.max_duration_ms { break; }
+        self.reap_one(ev.raw.id.into(), TombstoneOrigin::NIP40Expiry, now_s)?;
+        report.expired_reaped += 1;
+    }
+
+    // 3.2 — Trim LRU back to target.
+    let lru_before = self.hot.lock().lru.len();
+    self.hot.lock().trim();
+    report.lru_evicted = lru_before.saturating_sub(self.hot.lock().lru.len());
+
+    // 3.3 — Purge old tombstones whose target event is absent.
+    let cutoff = now_s.saturating_sub(self.cfg.tombstone_retention_secs);
+    report.tombstones_purged = self.purge_old_tombstones(cutoff,
+        budget.max_events_per_step.saturating_sub(report.expired_reaped))?;
+
+    report.duration_ms = start.elapsed().as_millis() as u32;
+    Ok(report)
+}
+```
+
+Single `gc_step()` is bounded by `GcBudget { max_events_per_step, max_duration_ms }`. Defaults: `max_events_per_step = 2000`, `max_duration_ms = 50`. The actor calls `gc_step()`:
+
+- Every 60 seconds (cooperative; runs on the actor thread between mailbox messages).
+- On `MemoryWarningCapability::Pressure` (iOS / Android low-memory signals).
+- On any single `insert()` that observes `hot.lru.len() > 2 * target_hot_size` (safety net).
+
+`gc_step()` is **never** invoked from an FFI call path — it runs on the actor's own schedule so any latency it introduces is invisible to the platform.
+
+## 4. Claim / release wiring
+
+The kernel actor holds `view_claims: HashMap<ViewId, ClaimerId>`. On `open_view(spec)`:
+
+1. The view module's `dependencies(spec)` is consulted (per `kernel-substrate.md` §3).
+2. The composite reverse-index resolves the dependency set to a (small, bounded) set of currently-known event ids — the *view cover*.
+3. `store.claim(claimer_id, &cover_ids)` pins those events in hot.
+4. As events arrive matching the dependency, the actor calls `store.claim(claimer_id, &[new_id])` incrementally (claim is idempotent under increment).
+
+On `close_view(view_id)`:
+
+1. `store.release(claimer_id)` drops every pin in one call.
+2. The view module's `state` is dropped; its claim refcounts decay; the next `gc_step()` evicts any newly-unpinned cold from LRU.
+
+Restart recovery: `claims_meta` sub-db ([`keys.md`](keys.md) §1) holds the persisted per-`ClaimerId` pin set. On startup the actor rebuilds active views first (per the diagnostics replay sequence), then re-claims; entries in `claims_meta` whose `ClaimerId` is not associated with a re-opened view are dropped from the persisted map. This means the cold-start path always re-derives claims from open-view state, but the persistence is what lets the store survive an actor restart without losing hot-set protection mid-shutdown.
+
+## 5. Memory accounting (the ADR-0003 gate)
+
+The relevant figure for the M3 exit gate is **working-set RSS at the configuration described in ADR-0003 §Decision**: 100 active views, 10k hot events, 1M cached on disk, ≤ 100 MB.
+
+Components measured:
+
+| Source | Approx bytes | Notes |
+|---|---|---|
+| Hot LRU (10k × Arc<Event>) | ~30 MB | average kind:1 event with content ~800 B, profile/contacts can be 4–8 KB each; mix-weighted average ~3 KB; the `Arc` is shared with view module payloads so the same body isn't duplicated |
+| Claim refcount maps (10k entries) | ~0.5 MB | `HashMap<EventId, u32>` + reverse `by_claimer` |
+| Reverse index in-memory (composite keys for 100 views) | ~5 MB | from ADR-0001 — bounded by `~broad_axes_guardrail` per ADR-0001 |
+| Projection caches (author display, reaction counts) | ~10 MB | LRU-bounded by referenced-view count per ADR-0003 |
+| LMDB page cache (kernel-owned, *not* counted toward RSS budget) | 0 | OS-paged, evicted under pressure; counts against system memory but not app working set |
+| Watermarks (loaded as `HashMap` for hot lookups) | ~2 MB | M4 — assuming O(10k) watermarks (one per `(filter, relay)` pair) |
+| Tombstone bloom filter (if added — see open questions) | ~1 MB | accelerates the `tombstones.contains_key()` check on insert |
+| Action ledger in-flight rows | ~1 MB | bounded by spec §7.5 |
+| Slack / Rust allocator overhead | ~20 MB | empirical from reactivity-bench |
+| **Total target** | **~70 MB** | leaves ~30 MB headroom against the 100 MB gate |
+
+The 1M-events-on-disk dimension does **not** appear in the budget because LMDB does not page them into our heap; they exist in mmap'd pages the OS may evict at will. This is the design intent of ADR-0003.
+
+## 6. Failure modes and degraded behavior
+
+| Failure | Detection | Response |
+|---|---|---|
+| LMDB env out of space | LMDB `MDB_MAP_FULL` on a write | Run an emergency `gc_step()` with relaxed budget; if still full, surface `Effect::StoreOutOfSpace`, refuse new inserts, allow reads + deletes |
+| LRU evicted a still-pinned event (bug) | `trim()` would have skipped it; if observed, log + invariant violation | Pin reinstated from `claims_meta`; fire `tracing::error!`; flagged as critical bug class to investigate |
+| `gc_step()` over-budget | `start.elapsed() > max_duration_ms` mid-loop | Break out of current loop early; remaining work picked up next call (no state corruption — every reaped event is its own transaction) |
+| `release()` called for unknown `ClaimerId` | `by_claimer.remove` returns None | Silent no-op; logged at debug; not a bug (idempotent close) |
+| Memory warning during heavy insert burst | iOS `didReceiveMemoryWarning` → `MemoryWarningCapability` event | Actor lowers `target_hot_size` to 5k, runs `gc_step({max_events_per_step:5000, max_duration_ms:200})` once; restored after the warning clears |
+
+## 7. Diagnostics integration (ADR-0007)
+
+The store exposes a `StoreHealth` snapshot for the diagnostics bridge:
+
+```rust
+pub struct StoreHealth {
+    pub primary_event_count: u64,
+    pub tombstone_count: u64,
+    pub hot_lru_size: usize,
+    pub claim_pinned_count: usize,
+    pub watermark_count: usize,
+    pub on_disk_bytes: u64,
+    pub last_gc: Option<GcReport>,
+    pub last_gc_at_ms: Option<u128>,
+}
+```
+
+Surfaced in the diagnostics screen alongside relay status (ADR-0007 §1). The Phase 1a.7 proof app already has the rendering scaffold (`ios/NmpStress/NmpStress/DiagnosticsView.swift`); M3 adds the StoreHealth row to it.
+
+## 8. Why not a periodic full sweep?
+
+A full sweep is `O(stored_events)`. With 1M events on disk the LMDB scan alone is 100–500 ms wall-time on iPhone 12 NAND — well outside the actor's single-message budget. The bounded `gc_step()` with explicit budget is therefore the only correct shape; it composes with LMDB's natural mmap eviction model and never blocks the mailbox for long.
+
+A periodic vacuum/compact pass (LMDB's equivalent of `VACUUM`) **is** scheduled — once per app launch, at idle, after the first 30 seconds of quiescence. It is *not* part of `gc_step`'s budget envelope and runs as a separate low-priority actor message that yields between LMDB page boundaries.
diff --git a/docs/design/lmdb/keys.md b/docs/design/lmdb/keys.md
new file mode 100644
index 0000000..61ccd10
--- /dev/null
+++ b/docs/design/lmdb/keys.md
@@ -0,0 +1,150 @@
+# LMDB sub-design: key encoding
+
+> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Fixes the byte layout of every sub-database the NMP store opens. Primary event storage is owned by `nostr-lmdb`; everything below is NMP-owned and lives in NMP-managed sub-databases under the same `lmdb::Environment` (per open question 1 in the master doc).
+
+## 1. LMDB environment layout
+
+One `lmdb::Environment` per app data directory. Sub-databases:
+
+| Sub-db | Owner | Key shape | Value | Notes |
+|---|---|---|---|---|
+| (multiple) | `nostr-lmdb` | upstream | upstream | event primary, internal filter indexes, kind:5 suppression |
+| `idx_author_kind` | NMP | `pubkey[32] ‖ kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` | empty | newest-first scans for `(author, kinds[])` |
+| `idx_kind_dtag` | NMP | `kind_be[4] ‖ pubkey[32] ‖ dtag_len_be[2] ‖ dtag_bytes` | `event_id[32]` | parameterized replaceable address lookup |
+| `idx_etag_time` | NMP | `target_event_id[32] ‖ created_at_desc_be[8] ‖ event_id[32]` | `kind_be[4]` | reaction/reply/thread view scans |
+| `idx_ptag_time` | NMP | `target_pubkey[32] ‖ created_at_desc_be[8] ‖ event_id[32]` | `kind_be[4]` | mentions / notifications |
+| `idx_kind_time` | NMP | `kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` | empty | global-by-kind backfills |
+| `idx_expires` | NMP | `expires_at_be[8] ‖ event_id[32]` | empty | NIP-40 reaper |
+| `tombstones` | NMP | `target_event_id[32]` | CBOR `TombstoneRow` | persists past delete |
+| `provenance` | NMP | `event_id[32]` | CBOR `ProvenanceRow` | per-relay sidecar (master doc §9) |
+| `watermarks` | NMP | `filter_hash[32] ‖ relay_url_bytes` | CBOR `WatermarkRow` | M4 NIP-77 sync state |
+| `claims_meta` | NMP | `claimer_id_be[8]` | CBOR `Vec<EventId>` | pinned set per ClaimerId; rebuilt on restart from open views |
+| `domain_<ns>_data` | NMP, per `DomainModule` | module-defined | module-defined | one sub-db per registered namespace |
+| `domain_<ns>_idx_<name>` | NMP, per `DomainModule` index | `index_key ‖ primary_key` | empty | secondary indexes per `DomainIndex` |
+| `_meta` | NMP | string namespace | `{ schema_version: u32, opened_with_nmp_version: String }` | migration tracking |
+
+Sub-databases are opened lazily on first access and cached on the `LmdbEventStore`.
+
+## 2. Endian + ordering conventions
+
+- All integers in keys are **big-endian** so LMDB's byte-wise comparator matches numeric order.
+- `created_at_desc_be = (u64::MAX - created_at).to_be_bytes()` so a forward scan returns newest-first without `MDB_PREV` gymnastics.
+- All pubkeys / event ids are fixed-width 32 bytes; the `nostr` crate's `EventId` and `PublicKey` give us byte arrays directly.
+
+## 3. Secondary index details
+
+### 3.1 `idx_author_kind`
+
+Key: `pubkey[32] ‖ kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` → empty value.
+
+Scan recipes:
+
+- *Newest N events by author* — `range(pubkey ‖ 0u32_be ‖ ..)` (kind=0 lower bound) up to `pubkey ‖ u32::MAX_be ‖ ..`, take N.
+- *Newest N events by `(author, kind=1)`* — `range(pubkey ‖ 1u32_be ‖ ..)` up to `pubkey ‖ 1u32_be ‖ u64::MAX_be`, take N.
+- *All kind:0 for author* — `range(pubkey ‖ 0u32_be ‖ ..)`, take 1 (because the replaceable index ensures only one).
+
+Replaceable supersession (§7.1): on insert of a new kind in [0, 3, 10000–19999], find existing row via this index with `(pubkey, kind)` prefix, compare `created_at`, if incoming wins delete old + write new. Both deletes happen in the same `RwTxn` as the new write so there is no half-state visible to readers.
+
+### 3.2 `idx_kind_dtag` (parameterized replaceable)
+
+Key: `kind_be[4] ‖ pubkey[32] ‖ dtag_len_be[2] ‖ dtag_bytes` → `event_id[32]`.
+
+The d-tag bytes go last so two events with the same `(kind, pubkey)` but different `d` tags don't collide; the explicit length prefix avoids `d="foo"` vs `d="foob"` aliasing under prefix scans. Lookup is exact-key: `get_param_replaceable(pubkey, kind, d_tag)` builds the key and reads.
+
+The value is the `event_id`; the primary event itself lives in the `nostr-lmdb` events sub-db. On supersession, the old event-id is fetched from this row, both primary and old `idx_*` rows are deleted, and the value is overwritten with the new id.
+
+### 3.3 `idx_etag_time` and `idx_ptag_time`
+
+Key: `target[32] ‖ created_at_desc_be[8] ‖ event_id[32]` → `kind_be[4]`.
+
+The value holds the kind so a reactions view can filter `(kinds == 7)` during scan without a primary-row fetch per candidate. Bookmark / repost / thread views similarly avoid the `get_by_id` round trip until they need the body.
+
+On insert, the kernel walks the event's `tags`: every `e` tag value goes into `idx_etag_time` and every `p` tag value goes into `idx_ptag_time`. Tag values must be 32-byte hex (validated at insert time); non-conformant tags are silently skipped from indexing (they are still stored in the event body).
+
+### 3.4 `idx_kind_time`
+
+Key: `kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` → empty.
+
+Used by *global-by-kind* backfills (e.g. "recent kind:0 across all authors" during diagnostics). Heavy index — populated for **all** kinds by default but the implementation may skip kinds in a configurable deny-list to keep write amplification down (default deny-list: kind:1 if config flag `index_kind1_globally=false`, which it is by default; M2's planner does not need a global kind:1 scan).
+
+### 3.5 `idx_expires`
+
+Key: `expires_at_be[8] ‖ event_id[32]` → empty.
+
+Populated **only** for events that have an `expiration` tag at insert (NIP-40). `gc_step()` opens a read cursor at `expires_at = 0`, walks forward up to the configured budget, and reaps any keys whose `expires_at ≤ now_unix_seconds()`. Each reaped event triggers a tombstone-of-origin `NIP40Expiry` write so re-insertions (from a re-sync) don't resurrect it.
+
+## 4. Tombstones
+
+Key: `target_event_id[32]` → CBOR `TombstoneRow`:
+
+```rust
+#[derive(Serialize, Deserialize)]
+struct TombstoneRow {
+    target_id: [u8; 32],
+    origin: TombstoneOrigin,             // Kind5 | NIP40Expiry | AdminPurge
+    kind5_event_id: Option<[u8; 32]>,    // None for non-Kind5 origins
+    deleter_pubkey: Option<[u8; 32]>,    // None for NIP40Expiry / AdminPurge
+    deleted_at: u64,                     // max observed across kind:5 redeliveries
+    sources: Vec<String>,                // relay urls that delivered the kind:5
+}
+```
+
+Insert pre-check: before any new event hits the primary store, `tombstones.contains_key(event.id)` is consulted. A hit yields `InsertOutcome::Tombstoned { target_kind5_id }` and the event is dropped. This is the "later re-insertion is suppressed" behavior of §7.1.
+
+Foreign kind:5 (where the kind:5 author did not author all targets) is **stored** as an ordinary event (so other clients can render the delete intent) but **does not** write a `TombstoneRow` for any of its targets — per §7.1 "foreign kind:5 ignored". The kind:5 event itself goes through the normal insert path including secondaries.
+
+## 5. Watermarks
+
+Key: `filter_hash[32] ‖ relay_url_bytes` — variable-length, exact-key lookups only. `filter_hash` is BLAKE3 of the canonical filter encoding (see `lmdb/watermarks.md` §3 for the canonicalisation algorithm).
+
+Value: CBOR `WatermarkRow` (same shape as the trait type in [`trait.md`](trait.md) §2).
+
+## 6. Provenance
+
+Key: `event_id[32]` → CBOR `ProvenanceRow { sources: Vec<ProvenanceEntry> }`. On duplicate insert: read, mutate (append or bump `last_seen_ms`), write back. Bounded growth — the kernel caps `sources.len()` at 32 (the 33rd unique relay overwrites the oldest non-primary entry); for nearly all events this is non-binding. The `primary: bool` flag is deterministic: `sources[0]` after sorting by `(first_seen_ms, relay_url)`.
+
+## 7. Domain rows (per `DomainModule`)
+
+For each `DomainModule` with namespace `"foo.bar"`:
+
+- `domain_foo.bar_data` — primary data sub-db. Module owns key + value encoding.
+- `domain_foo.bar_idx_<index>` — one sub-db per `DomainIndex` (per `crates/nmp-core/src/substrate/domain.rs:16`). Key = `index_key_fn(data_value) ‖ primary_key`; value = empty. The index is rewritten on every put (delete-old, write-new).
+
+The actor exposes them only via `DomainHandle` (see [`trait.md`](trait.md) §4); modules never see the sub-db handles directly. Module isolation per `kernel-substrate.md` §8 is preserved: the handle factory checks the caller's registered namespace.
+
+## 8. `_meta` sub-database
+
+Key: namespace string (e.g. `"twitter.drafts"`, `"_kernel"`). Value: CBOR `{ schema_version: u32, opened_with_nmp_version: String, last_migration_at_ms: u64 }`. Read at startup by the migration runner; written after every successful migration step.
+
+The reserved `_kernel` namespace tracks the LMDB store's own schema version (currently 1). A bumped `_kernel` version triggers store-wide migrations (e.g. re-encoding all `ProvenanceRow` values when the format changes).
+
+## 9. Worked example: inserting a kind:1 from `pablof7z` arriving from `wss://relay.primal.net`
+
+```
+event_id   = a3f1...   (32 bytes)
+pubkey     = 0461...   (32 bytes)
+kind       = 1
+created_at = 1747000000
+tags       = [["e","b21c...","","root"], ["p","0488..."]]
+```
+
+Inside one `RwTxn`:
+
+1. `tombstones.get(&event_id)` → None ⇒ proceed.
+2. `nostr_lmdb.save_event(&event)` → SaveEventStatus::Success.
+3. `idx_author_kind.put(0461... ‖ 0x00000001 ‖ desc(1747000000) ‖ a3f1..., &[])`.
+4. `idx_kind_time.put(0x00000001 ‖ desc(1747000000) ‖ a3f1..., &[])` (only if `index_kind1_globally`; default off).
+5. For `e:b21c...` → `idx_etag_time.put(b21c... ‖ desc(1747000000) ‖ a3f1..., 1u32_be)`.
+6. For `p:0488...` → `idx_ptag_time.put(0488... ‖ desc(1747000000) ‖ a3f1..., 1u32_be)`.
+7. `provenance.put(a3f1..., cbor({sources:[{relay:"wss://relay.primal.net", first_seen_ms:T, last_seen_ms:T, primary:true}]}))`.
+
+Total LMDB writes: 1 primary (delegated to upstream) + 3 NMP secondaries + 1 provenance = ~5 page writes for a typical kind:1. Within the 250 µs p99 budget (master doc §12) on iPhone 12 NAND.
+
+A second arrival of the same id from `wss://nos.lol`:
+
+1. `tombstones.get(&a3f1...)` → None.
+2. `nostr_lmdb.save_event` → SaveEventStatus::Duplicate (we don't re-process).
+3. Skip steps 3–6 (secondaries unchanged).
+4. `provenance.get(a3f1...)` → existing row; append `{relay:"wss://nos.lol", first_seen_ms:T2, last_seen_ms:T2, primary:false}`; put back.
+
+One read + one write. Returns `InsertOutcome::Duplicate { sources_after: 2 }`.
diff --git a/docs/design/lmdb/tests.md b/docs/design/lmdb/tests.md
new file mode 100644
index 0000000..762b46b
--- /dev/null
+++ b/docs/design/lmdb/tests.md
@@ -0,0 +1,223 @@
+# LMDB sub-design: test plan
+
+> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Maps every insert invariant in `docs/product-spec/subsystems.md` §7.1 to a concrete test in `crates/nmp-testing/tests/`. Each test exists for both `MemEventStore` (always) and `LmdbEventStore` (under `#[cfg(feature = "lmdb-backend")]`).
+
+## 1. Test harness shape
+
+```rust
+// crates/nmp-testing/src/store_harness.rs
+pub struct StoreHarness {
+    pub store: Box<dyn EventStore>,
+    pub tmp: tempfile::TempDir,
+    pub keys: nostr::Keys,
+}
+
+impl StoreHarness {
+    pub fn mem() -> Self { /* MemEventStore */ }
+    pub fn lmdb() -> Self { /* LmdbEventStore in tmp dir */ }
+
+    pub fn insert(&self, builder: EventBuilder, source: &str) -> InsertOutcome { /* ... */ }
+    pub fn assert_present(&self, id: &EventId);
+    pub fn assert_tombstoned(&self, id: &EventId);
+    pub fn restart(&mut self);   // close + reopen the store; LMDB only
+}
+
+// Tests use a macro to run against both backends.
+macro_rules! for_each_backend {
+    ($name:ident, $body:expr) => {
+        #[test] fn $name() { let mut h = StoreHarness::mem(); $body(&mut h); }
+        #[cfg(feature = "lmdb-backend")]
+        #[test] fn paste::paste!([<$name _lmdb>])() {
+            let mut h = StoreHarness::lmdb(); $body(&mut h);
+        }
+    };
+}
+```
+
+The harness lives in `crates/nmp-testing/src/` so per-test files are short and declarative.
+
+## 2. Invariant → test mapping
+
+Every row of the §7.1 table:
+
+### 2.1 Insert API single path (§7.1 row "Insert API")
+
+File: `crates/nmp-testing/tests/store_insert_path.rs`
+
+```rust
+for_each_backend!(insert_returns_insert_outcome, |h: &mut StoreHarness| {
+    let event = h.signed(EventBuilder::text_note("hello", &[]));
+    let outcome = h.store.insert(event.clone(), &"wss://t/".into(), 0).unwrap();
+    assert!(matches!(outcome, InsertOutcome::Inserted { .. }));
+    assert!(h.store.get_by_id(&event.id.to_bytes()).unwrap().is_some());
+});
+```
+
+Plus a static-assertion-style test ensuring no other public function on `EventStore` writes to the primary store (compile-time check by inspecting trait method list via a build script — deferred to v1.x; v1 covers via review).
+
+### 2.2 Signature verification (§7.1 row "Signature/delegation validity")
+
+File: `crates/nmp-testing/tests/store_invalid_sig.rs`
+
+Builds an event, mutates the signature, inserts. Expects `InsertOutcome::Rejected { reason: RejectReason::BadSignature }` and no row in primary, secondaries, provenance, or tombstones. Also tests a malformed NIP-26 delegation tag (rejects with `BadDelegation`).
+
+### 2.3 Duplicate id → provenance merge (§7.1 row "Duplicate id")
+
+File: `crates/nmp-testing/tests/store_provenance_merge.rs`
+
+```rust
+for_each_backend!(duplicate_merges_provenance_keeps_earliest, |h| {
+    let ev = h.signed(EventBuilder::text_note("x", &[]));
+    let o1 = h.store.insert(ev.clone(), &"wss://a/".into(), 1000).unwrap();
+    let o2 = h.store.insert(ev.clone(), &"wss://b/".into(), 2000).unwrap();
+    assert!(matches!(o1, InsertOutcome::Inserted { .. }));
+    assert!(matches!(o2, InsertOutcome::Duplicate { sources_after: 2, .. }));
+    let p = h.store.provenance_for(&ev.id.to_bytes()).unwrap();
+    assert_eq!(p.len(), 2);
+    let primary = p.iter().find(|e| e.primary).unwrap();
+    assert_eq!(primary.relay_url, "wss://a/");
+    assert_eq!(primary.first_seen_ms, 1000); // earliest preserved
+});
+```
+
+### 2.4 Replaceable supersession (§7.1 row "Replaceable kinds")
+
+File: `crates/nmp-testing/tests/store_replaceable.rs`
+
+Inserts two kind:0 from same pubkey, second with later `created_at`. Asserts: `get_by_id(first_id)` returns None; `scan_by_author_kind(pk, &[0], None, None, 10)` returns one row; the row's id is the second. Tie-break test: two kind:0 with same `created_at` — keep the lexicographically smaller id.
+
+### 2.5 Parameterized replaceable (§7.1 row "Parameterized replaceable")
+
+File: `crates/nmp-testing/tests/store_param_replaceable.rs`
+
+Insert two kind:30023 with same `(pubkey, d=foo)`, second newer; assert only the second is returned by `get_param_replaceable(pk, 30023, b"foo")`. Insert a third with same kind+pubkey but `d=bar` — assert both `foo` and `bar` are independently retrievable. Assert that a kind:30024 with `d=foo` (different kind) does not collide with the kind:30023.
+
+### 2.6 Kind:5 self-delete + tombstone persistence (§7.1 row "Kind 5")
+
+File: `crates/nmp-testing/tests/store_kind5_tombstone.rs`
+
+- Insert kind:1 by Alice.
+- Insert kind:5 by Alice referencing the kind:1 via `e` tag.
+- Assert kind:1 gone from primary; tombstone row exists with `target_id == kind1.id`, `origin == Kind5`.
+- Insert the same kind:1 again — assert `InsertOutcome::Tombstoned`, no primary row created.
+- Restart store; repeat the re-insertion — assert tombstone persists across restart.
+
+### 2.7 Foreign kind:5 ignored (§7.1 row "Kind 5" — foreign clause)
+
+File: `crates/nmp-testing/tests/store_kind5_foreign.rs`
+
+- Insert kind:1 by Alice.
+- Insert kind:5 by Bob referencing Alice's kind:1.
+- Assert: kind:1 is still present in primary (Bob can't delete Alice's event); the kind:5 event itself is stored (so other clients can see it); no tombstone row was written.
+
+### 2.8 NIP-40 expiration scheduling (§7.1 row "NIP-40 expiration")
+
+File: `crates/nmp-testing/tests/store_nip40_expiration.rs`
+
+- Insert kind:1 with `expiration` tag at `now + 1 second`.
+- Assert `scan_expiring_before(now + 5, 10)` returns the event.
+- Call `gc_step(GcBudget { max_events_per_step: 10, max_duration_ms: 100 })` at `now + 2`.
+- Assert primary row gone; tombstone written with `origin == NIP40Expiry`.
+- Insert same event again — assert `InsertOutcome::Tombstoned`.
+- Insert an event with `expiration` already in the past — assert `InsertOutcome::Rejected { reason: ExpiredOnArrival }`.
+- Restart store; insert new event with `expiration` at `now + 1`; assert the reaper picks it up after restart (the `idx_expires` cursor scan is the source of truth — no separate timer needs to survive restart).
+
+### 2.9 Watermarks (§7.1 "Sync watermarks")
+
+File: `crates/nmp-testing/tests/store_watermarks.rs`
+
+- Write a watermark; read it back; assert equal.
+- Restart store; read again; assert preserved.
+- Test `coverage()`: row with `synced_up_to = now - 60s` → `Coverage::CompleteAsOf` (under default 300s staleness); row with `synced_up_to = now - 600s` → `Coverage::PartialUpTo`; missing row → `Coverage::Unknown`.
+- `list_watermarks_for_relay("wss://a/")` returns only rows for that relay.
+- Concurrent writes to the same key (simulated): last-writer-wins, no row corruption.
+
+### 2.10 Claims + GC (§7.1 "GC")
+
+File: `crates/nmp-testing/tests/store_gc_claims.rs`
+
+- Insert 100 events; all in hot LRU (under default 10k ceiling).
+- Claim 10 of them under `ClaimerId(1)`.
+- Configure `target_hot_size = 50`; insert another 50 events; call `gc_step`.
+- Assert: 10 claimed events still present in hot (`store.get_by_id` is a fast in-memory hit — measurable via a counter exposed for the test); 40 unclaimed events evicted from LRU but still readable from disk.
+- Release `ClaimerId(1)`; insert another 20 events; call `gc_step`.
+- Assert: previously claimed events now subject to LRU eviction.
+
+### 2.11 Dump round-trip (master doc §10)
+
+File: `crates/nmp-testing/tests/store_dump_roundtrip.rs`
+
+- Build a populated store: 1000 events, 50 tombstones, 100 watermarks, 200 domain rows across 3 namespaces.
+- `dump(&mut buf1, DumpFormat::Jsonl)`.
+- Open a fresh store; replay every line; `dump(&mut buf2, ...)`.
+- Assert `buf1 == buf2` byte-for-byte (sort by stable key first — the dump iterates sub-dbs in a deterministic order documented in the dump module).
+
+### 2.12 Domain migration success + failure (master doc §6)
+
+File: `crates/nmp-testing/tests/store_domain_migration.rs`
+
+- Register `TestModuleV1` with `SCHEMA_VERSION = 1` and no migrations; open store; assert `_meta.test_module.schema_version == 1`.
+- Close store; register `TestModuleV2` with `SCHEMA_VERSION = 2` and one migration v1→v2 that writes one key; open store; assert migration ran and key exists.
+- Close; register `TestModuleV3` with `SCHEMA_VERSION = 3` and a deliberately failing migration v2→v3; open store; assert `Effect::DomainSchemaTooNew { namespace: "test_module" }` (under degraded-mode rules) and `_meta` still at v2.
+- Close; remove the failing migration; reopen — assert successful catch-up to v3 (idempotent retry).
+
+### 2.13 Domain isolation (`kernel-substrate.md` §8)
+
+File: `crates/nmp-testing/tests/store_domain_isolation.rs`
+
+- Open `DomainHandle` for module A; write key `K`.
+- Open `DomainHandle` for module B; read key `K` — assert returns `None`.
+- Module B's `scan_prefix(b"")` returns only module B's rows.
+
+### 2.14 Cold-start performance (master doc §12)
+
+Scenario in `crates/nmp-testing/bin/firehose-bench/src/scenarios/cold_start.rs` (already exists in M1; extended here):
+
+- Pre-populate an LMDB store with a representative session (~20k events: 10k kind:1, 8k kind:0, 2k kind:3 / 10002).
+- Tar + ship the file with the test fixture.
+- Measure: open store, register modules, run the bootstrap sequence that the actor runs on app launch, until the first `AppUpdate::FullState` is emitted with non-empty timeline.
+- Gate: ≤ 1.5 s on iPhone 12 hardware; ≤ 800 ms on iPhone 16 Pro simulator.
+
+### 2.15 Working-set memory (ADR-0003)
+
+Scenario in `crates/nmp-testing/bin/reactivity-bench` — extended with a new `--scenario working_set_lmdb` mode:
+
+- Insert 1M synthetic events into the store.
+- Open 100 view subscriptions covering 10k events.
+- Run for 60 seconds with light churn (insert 10 events / sec).
+- Sample RSS every 5 seconds via `/proc/self/status` on Linux / `mach_task_basic_info` on iOS.
+- Gate: max RSS ≤ 100 MB over the run.
+
+### 2.16 Restart preserves replaceable semantics (`plan.md` §M3 exit gate)
+
+File: `crates/nmp-testing/tests/store_replaceable_restart.rs`
+
+- Write kind:0 v1; assert present.
+- Write kind:0 v2 (newer); assert v1 gone, v2 present.
+- Restart store; assert v2 still present, v1 still gone.
+- Write kind:0 v0 (older than v2); assert no change (`InsertOutcome::Superseded`).
+
+## 3. Property tests
+
+In `crates/nmp-testing/tests/store_props.rs` using `proptest`:
+
+- **Insert is total under random valid events.** Generate a vec of valid signed events, insert in any order, assert the store's `get_by_id` agrees with the model (a `HashMap` reference impl).
+- **Replaceable convergence.** For any sequence of replaceable inserts for the same `(pubkey, kind, [d])` key, the final stored event is the (max created_at, min id) winner regardless of insertion order.
+- **Provenance commutativity.** For any two relay sources `r1, r2` and identical event, the post-state of provenance is identical to inserting `r2` first then `r1`.
+- **`nmp dump` is a fixed point.** Round-trip equality after N random operations.
+
+## 4. Cross-test invariants (asserted in a `teardown` hook for every test)
+
+Every test ends with `harness.assert_invariants()`:
+
+1. Every event in the primary store has a `provenance` row with ≥ 1 entry.
+2. Every secondary index entry's `event_id` resolves to an existing primary row.
+3. Every tombstone's `target_id` does **not** exist in the primary store.
+4. The `_meta._kernel.schema_version` is at the latest version the binary knows.
+5. The hot LRU contains only events that exist in the primary store.
+
+Violation of any invariant fails the test with a precise diff of which sub-db is out of sync.
+
+## 5. CI integration
+
+`cargo test --workspace --features lmdb-backend` becomes part of the pre-merge gate from M3 onward (`plan.md` §6 will be updated). The criterion benches in `crates/nmp-testing/benches/store_*.rs` run nightly with regression checks against the previous week's median (>5% regression on any p99 fails the nightly).
diff --git a/docs/design/lmdb/trait.md b/docs/design/lmdb/trait.md
new file mode 100644
index 0000000..2bbdc60
--- /dev/null
+++ b/docs/design/lmdb/trait.md
@@ -0,0 +1,312 @@
+# LMDB sub-design: `EventStore` trait
+
+> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). This file fixes the trait surface; the master doc fixes the decision.
+
+## 1. Crate placement
+
+`crates/nmp-core/src/store/events.rs` (filename note: `trait` is a Rust keyword, so the file is named `events.rs` and exposes `pub trait EventStore`). Re-exported from `nmp_core::store::EventStore`. The actor (`crates/nmp-core/src/actor.rs`) holds the store as `store: Box<dyn EventStore>`; backends are constructed by the factory in `store/mod.rs::open_event_store(&AppConfig) -> Result<Box<dyn EventStore>, StoreError>`.
+
+## 2. Supporting types
+
+```rust
+use std::sync::Arc;
+
+pub type EventId = [u8; 32];
+pub type PubKey = [u8; 32];
+pub type RelayUrl = String;
+
+#[derive(Clone, Debug)]
+pub struct StoredEvent {
+    pub raw: Arc<nostr::Event>,         // upstream nostr crate type
+    pub received_at_ms: u64,            // wall-clock first arrival across all relays
+}
+
+#[derive(Clone, Debug)]
+pub struct ProvenanceEntry {
+    pub relay_url: RelayUrl,
+    pub first_seen_ms: u64,
+    pub last_seen_ms: u64,
+    pub primary: bool,                  // first observed relay (deterministic)
+}
+
+#[derive(Clone, Debug)]
+pub enum InsertOutcome {
+    /// Fresh insert; secondary indexes written.
+    Inserted { id: EventId, sources_after: u32 },
+    /// Duplicate id; provenance updated, primary untouched.
+    Duplicate { id: EventId, sources_after: u32 },
+    /// Replaceable supersession: this event replaced an older one.
+    Replaced { new_id: EventId, replaced_id: EventId },
+    /// Replaceable supersession: incoming was older, dropped.
+    Superseded { id: EventId, current_id: EventId },
+    /// Suppressed because target is tombstoned.
+    Tombstoned { id: EventId, target_kind5_id: EventId },
+    /// Signature / delegation / structural validity failed.
+    Rejected { id: EventId, reason: RejectReason },
+    /// Ephemeral kind: delivered to live consumers, not stored.
+    Ephemeral { id: EventId },
+}
+
+#[derive(Clone, Debug)]
+pub enum RejectReason {
+    BadSignature,
+    BadDelegation(String),
+    Malformed(String),
+    ExpiredOnArrival,                   // NIP-40 expiration already in the past
+}
+
+#[derive(Clone, Debug)]
+pub struct TombstoneRow {
+    pub target_id: EventId,
+    pub kind5_event_id: Option<EventId>, // None for NIP-40 expiry tombstones
+    pub deleter_pubkey: Option<PubKey>,
+    pub deleted_at: u64,                 // unix seconds
+    pub sources: Vec<RelayUrl>,
+    pub origin: TombstoneOrigin,
+}
+
+#[derive(Clone, Copy, Debug, Eq, PartialEq)]
+pub enum TombstoneOrigin { Kind5, NIP40Expiry, AdminPurge }
+
+#[derive(Clone, Debug)]
+pub struct WatermarkKey {
+    pub filter_hash: [u8; 32],
+    pub relay_url: RelayUrl,
+}
+
+#[derive(Clone, Debug)]
+pub struct WatermarkRow {
+    pub key: WatermarkKey,
+    pub synced_up_to: u64,               // unix seconds
+    pub last_sync_method: SyncMethod,
+    pub last_negentropy_state: Option<Vec<u8>>,
+    pub bytes_saved_vs_req: u64,
+    pub updated_at: u64,
+}
+
+#[derive(Clone, Copy, Debug, Eq, PartialEq)]
+pub enum SyncMethod { Negentropy, ReqScan, Manual }
+
+#[derive(Clone, Copy, Debug)]
+pub struct ClaimerId(pub u64);           // opaque view-handle id from the actor
+
+#[derive(Clone, Copy, Debug)]
+pub struct GcBudget {
+    pub max_events_per_step: usize,
+    pub max_duration_ms: u32,
+}
+
+#[derive(Clone, Debug, Default)]
+pub struct GcReport {
+    pub expired_reaped: usize,
+    pub lru_evicted: usize,
+    pub tombstones_purged: usize,
+    pub duration_ms: u32,
+}
+
+#[derive(Clone, Copy, Debug)]
+pub enum DumpFormat { Jsonl, Cbor }
+
+#[derive(Clone, Debug, Default)]
+pub struct DumpStats {
+    pub events: u64,
+    pub tombstones: u64,
+    pub watermarks: u64,
+    pub domain_rows: u64,
+    pub bytes_written: u64,
+}
+
+#[derive(Debug, thiserror::Error)]
+pub enum StoreError {
+    #[error("backend i/o: {0}")] Io(String),
+    #[error("backend corruption: {0}")] Corrupt(String),
+    #[error("encoding: {0}")] Encoding(String),
+    #[error("schema too new: {namespace} on-disk={on_disk} expected={expected}")]
+    SchemaTooNew { namespace: String, on_disk: u32, expected: u32 },
+    #[error("schema migration failed: {namespace} v{from}->v{to}: {reason}")]
+    MigrationFailed { namespace: String, from: u32, to: u32, reason: String },
+    #[error("unknown namespace: {0}")] UnknownNamespace(String),
+}
+```
+
+The store iterates lazily for scans:
+
+```rust
+pub trait EventIter: Iterator<Item = Result<StoredEvent, StoreError>> + Send {}
+impl<T: Iterator<Item = Result<StoredEvent, StoreError>> + Send> EventIter for T {}
+```
+
+`StoredEvent::raw` is `Arc<nostr::Event>` so the hot LRU can hold reference-counted copies without cloning the event body on each `get_by_id`.
+
+## 3. The trait
+
+```rust
+pub trait EventStore: Send + Sync {
+    // ─────── Reads ───────
+
+    /// Primary lookup. Returns Ok(None) if absent; tombstones do not count as "present".
+    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError>;
+
+    /// `idx_author_kind` scan, newest-first. `kinds` empty = any kind.
+    fn scan_by_author_kind<'a>(
+        &'a self,
+        author: &PubKey,
+        kinds: &[u32],
+        since: Option<u64>,
+        until: Option<u64>,
+        limit: usize,
+    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
+
+    /// `idx_kind_dtag` lookup. Returns the current authoritative parameterized
+    /// replaceable for `(pubkey, kind, d_tag)`, or Ok(None).
+    fn get_param_replaceable(
+        &self,
+        pubkey: &PubKey,
+        kind: u32,
+        d_tag: &[u8],
+    ) -> Result<Option<StoredEvent>, StoreError>;
+
+    /// `idx_etag_time` scan, newest-first. Used by reaction / repost / thread views.
+    fn scan_by_etag<'a>(
+        &'a self,
+        target: &EventId,
+        kinds: &[u32],
+        limit: usize,
+    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
+
+    /// `idx_ptag_time` scan, newest-first. Used by notifications / mention views.
+    fn scan_by_ptag<'a>(
+        &'a self,
+        target: &PubKey,
+        kinds: &[u32],
+        limit: usize,
+    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
+
+    /// `idx_kind_time` scan, newest-first. Used by timeline backfills.
+    /// `kinds` empty = any kind (parity with `scan_by_author_kind`).
+    fn scan_by_kind_time<'a>(
+        &'a self,
+        kinds: &[u32],
+        since: Option<u64>,
+        until: Option<u64>,
+        limit: usize,
+    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
+
+    /// `idx_expires` scan, ascending — used by the NIP-40 reaper.
+    fn scan_expiring_before<'a>(
+        &'a self,
+        unix_seconds: u64,
+        limit: usize,
+    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
+
+    /// Tombstones referencing a target id (typically one row).
+    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError>;
+
+    /// Iterate all tombstones (used by `nmp dump`).
+    fn list_tombstones<'a>(&'a self)
+        -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>;
+
+    /// Provenance sidecar for an event.
+    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError>;
+
+    // ─────── Writes ───────
+
+    /// The single insert path. `source` is the relay that delivered this copy.
+    /// Verifies signature/delegation, applies §7.1 invariants, updates secondaries
+    /// + provenance + tombstones atomically. Returns InsertOutcome per §7.1.
+    fn insert(&self, event: nostr::Event, source: &RelayUrl, received_at_ms: u64)
+        -> Result<InsertOutcome, StoreError>;
+
+    /// Delete by a NMP-internal filter — for admin / GC / kind:5 application.
+    /// Returns the number of primary rows removed.
+    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError>;
+
+    // ─────── Watermarks ───────
+
+    fn read_watermark(&self, key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError>;
+    fn write_watermark(&self, row: WatermarkRow) -> Result<(), StoreError>;
+    fn list_watermarks_for_relay<'a>(
+        &'a self,
+        relay_url: &str,
+    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>;
+
+    // ─────── Hot-set / claims (GC) ───────
+
+    /// Register a claim: caller pins `ids` against eviction until `release`.
+    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError>;
+    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError>;
+
+    /// Soft hint: keep these in hot LRU on a best-effort basis.
+    fn hot_set_hint(&self, ids: &[EventId]) -> Result<(), StoreError>;
+
+    /// One bounded GC pass — reap expired, trim LRU, purge old tombstones.
+    fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError>;
+
+    // ─────── Domain rows (per-DomainModule typed namespace) ───────
+
+    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle<'_>, StoreError>;
+    fn run_migrations(&self, namespace: &'static str, target_version: u32,
+                      migrations: &[crate::substrate::DomainMigration])
+        -> Result<(), StoreError>;
+
+    // ─────── Export ───────
+
+    fn dump(&self, out: &mut dyn std::io::Write, format: DumpFormat)
+        -> Result<DumpStats, StoreError>;
+}
+```
+
+`DeleteFilter` mirrors the limited subset of admin operations the kernel needs (by-relay-only events, by-author, by-id-list, by-kind range); it is **not** a pass-through to `nostr::Filter` — we intentionally do not expose arbitrary remote filters as a delete vector.
+
+## 4. `DomainHandle`
+
+```rust
+pub struct DomainHandle<'env> {
+    pub(crate) namespace: &'static str,
+    pub(crate) inner: DomainHandleInner<'env>,  // backend-specific
+}
+
+impl<'env> DomainHandle<'env> {
+    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError>;
+    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;
+    pub fn delete(&self, key: &[u8]) -> Result<bool, StoreError>;
+    pub fn scan_prefix<'a>(&'a self, prefix: &[u8])
+        -> Result<Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>, StoreError>;
+    pub fn scan_index<'a>(&'a self, index: &'static str, key_prefix: &[u8])
+        -> Result<Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>, StoreError>;
+}
+```
+
+A handle is module-scoped; the kernel does not give a `DraftsModule` handle to `SettingsModule` (per `kernel-substrate.md` §8 "Domain stores are isolated"). The handle is `'env`-bounded so it cannot outlive the LMDB environment.
+
+## 5. Error semantics (doctrine D3)
+
+The trait returns `Result<T, StoreError>`. The actor's wrapper functions map them as:
+
+- `Io / Corrupt` at startup → panic (we cannot run without a store; surfaces to platform shell as a process restart).
+- `Io / Corrupt` mid-run → `Effect::StoreDegraded { details }` published on the diagnostics bridge (ADR-0007); the affected operation returns the closest-fit graceful default (empty iterator, drop-write); the next gc_step retries.
+- `Encoding` → `tracing::error!` with the offending key/namespace; the action that triggered it fails with a `toast: Some("internal storage error; please restart")` per D3.
+- `SchemaTooNew` at startup → publish `Effect::DomainSchemaTooNew { namespace }`, the affected module starts in degraded mode (its actions reject with `ActionRejection::ModuleUnavailable`), rest of the kernel runs.
+- `MigrationFailed` → same as above, plus a one-time toast on first action attempt.
+- `UnknownNamespace` → programming error; assert in debug, log + drop in release.
+
+No `StoreError` ever crosses FFI. The `AppUpdate` carries only successful state + optional `toast: Option<String>`.
+
+## 6. Two backends in v1
+
+```rust
+// In-memory backend, kept for tests + web-pre-M15.
+pub struct MemEventStore { /* HashMap-backed; preserves the current kernel state */ }
+
+// Production backend on iOS / Android / Desktop.
+pub struct LmdbEventStore { /* wraps nostr_lmdb::NostrLMDB + NMP sub-dbs */ }
+
+pub fn open_event_store(cfg: &AppConfig) -> Result<Box<dyn EventStore>, StoreError> {
+    match cfg.storage_backend {
+        StorageBackend::Memory => Ok(Box::new(MemEventStore::new())),
+        StorageBackend::Lmdb { ref path } => Ok(Box::new(LmdbEventStore::open(path)?)),
+    }
+}
+```
+
+`MemEventStore` implements every method using `HashMap` / `BTreeMap`. The same test suite runs against both backends with `#[cfg(feature = "lmdb-backend")]` gating only the LMDB-specific edge tests (corruption recovery, oversized values).
diff --git a/docs/design/lmdb/watermarks.md b/docs/design/lmdb/watermarks.md
new file mode 100644
index 0000000..e6b4333
--- /dev/null
+++ b/docs/design/lmdb/watermarks.md
@@ -0,0 +1,191 @@
+# LMDB sub-design: watermarks, provenance, migrations
+
+> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Covers the three NMP-owned LMDB sub-databases that hold non-event durable state: `watermarks`, `provenance`, and the per-`DomainModule` sub-databases tracked by `_meta`.
+
+## 1. Watermark table
+
+Sub-db: `watermarks`. Populated by the M4 NIP-77 negentropy engine and consulted by the M2 subscription planner (per `docs/product-spec/subsystems.md` §7.2 "coverage-aware backfill").
+
+Key: `filter_hash[32] ‖ relay_url_bytes` — exact-key lookups only; no scans by `filter_hash` alone are needed (the planner always knows the relay it's about to query). The relay url is appended raw (no length prefix) because lookups are exact-key and the trailing bytes have no semantic role beyond identification.
+
+Value: CBOR `WatermarkRow`:
+
+```rust
+#[derive(Serialize, Deserialize, Clone, Debug)]
+pub struct WatermarkRow {
+    pub key: WatermarkKey,
+    pub synced_up_to: u64,                 // unix seconds
+    pub last_sync_method: SyncMethod,      // Negentropy | ReqScan | Manual
+    pub last_negentropy_state: Option<Vec<u8>>, // engine-opaque resume blob (M4)
+    pub bytes_saved_vs_req: u64,           // cumulative; for diagnostics
+    pub updated_at: u64,                   // unix seconds
+}
+```
+
+`last_negentropy_state` is an opaque byte blob written by the NIP-77 engine (M4) — the store does not interpret it. Empty for `ReqScan` / `Manual` rows.
+
+### 1.1 Authoritative cache-miss semantics
+
+Per §7.1 of the spec: "A cache-miss query against a fully-synced `(filter, relay)` pair is **authoritative**." The store implements this via the read path:
+
+```rust
+pub fn coverage(&self, key: &WatermarkKey, now_s: u64) -> Coverage {
+    match self.read_watermark(key) {
+        Ok(Some(row)) if row.synced_up_to >= now_s.saturating_sub(self.cfg.coverage_staleness_secs) =>
+            Coverage::CompleteAsOf(row.synced_up_to),
+        Ok(Some(row)) => Coverage::PartialUpTo(row.synced_up_to),
+        Ok(None) => Coverage::Unknown,
+        Err(_) => Coverage::Unknown,  // degraded; do not lie about completeness
+    }
+}
+```
+
+`coverage_staleness_secs` defaults to 300 s — a row that hasn't been re-confirmed in 5 minutes is treated as partial. The planner uses this signal to decide whether a missing-event query is "doesn't exist" (CompleteAsOf) or "need to fetch" (PartialUpTo / Unknown).
+
+### 1.2 Restart hydration
+
+On `LmdbEventStore::open()`, the store reads all `watermarks` rows and builds an in-memory `HashMap<WatermarkKey, WatermarkRow>` for hot lookups. Every `write_watermark` updates both the in-memory map and the LMDB row in a single `RwTxn`. Restart re-derives the map; we don't need a separate cache file.
+
+For installations with O(100k+) watermarks (an edge case — typical apps see O(100)–O(10k)), the in-memory map switches to a lazy-loaded variant that pages on demand. Threshold and switching logic deferred to M4 when the negentropy engine's real-world row count is measured.
+
+## 2. Provenance
+
+Sub-db: `provenance`. Per-event sidecar; the master doc §9 justifies the split-table choice.
+
+Key: `event_id[32]`. Value: CBOR `ProvenanceRow`:
+
+```rust
+#[derive(Serialize, Deserialize, Clone, Debug)]
+pub struct ProvenanceRow {
+    pub sources: SmallVec<[ProvenanceEntry; 4]>,    // bounded at 32; see master doc §9
+}
+
+#[derive(Serialize, Deserialize, Clone, Debug)]
+pub struct ProvenanceEntry {
+    pub relay_url: String,
+    pub first_seen_ms: u64,
+    pub last_seen_ms: u64,
+    pub primary: bool,
+}
+```
+
+The `primary: bool` is set deterministically: after every mutation, the `sources` vec is sorted by `(first_seen_ms, relay_url)` and the head element gets `primary = true`, all others `false`. This produces a stable "first observed relay" identifier regardless of the order in which redeliveries are processed.
+
+### 2.1 Mutation hot path
+
+For a duplicate-id insert, the per-event provenance write is the **only** LMDB write (per [`keys.md`](keys.md) §9 worked example). The store reads the existing row, mutates the matching entry's `last_seen_ms` (or appends), re-sorts + recomputes `primary`, and writes it back. Total cost: 1 read + 1 write on a 4-element CBOR row — well under 50 µs on iPhone 12 NAND.
+
+The bound of 32 distinct relays per event is empirical: in practice an event is delivered by 1–6 relays; events propagated to "everywhere" (popular kind:0 / kind:3) might hit 15–25. The 32nd entry overwrites the oldest non-primary slot, preserving the primary anchor.
+
+### 2.2 Reuse in outbox routing (M2)
+
+The M2 subscription planner consults `provenance_for(id)` to learn which relays have already delivered an event when scoring per-relay coverage in `LogicalInterestStatus::relay_urls`. This avoids re-fetching the same event from relays we already know carry it. The provenance sidecar is also part of the ADR-0007 diagnostics bridge — the diagnostics screen shows per-event source counts in the firehose tap view.
+
+## 3. Filter canonicalisation (for `filter_hash`)
+
+The `filter_hash` field in `WatermarkKey` is BLAKE3 of the canonical filter encoding. Canonicalisation rules:
+
+1. Within each tag-value array (`#e`, `#p`, `#a`, etc.), sort ascending bytewise.
+2. Sort the `kinds` array ascending numerically.
+3. Sort the `authors` array ascending bytewise.
+4. Sort the `ids` array ascending bytewise.
+5. Encode the filter as CBOR with map keys in this lexicographic order: `authors`, `ids`, `kinds`, `since`, `until`, `limit`, `search`, then `#<tag>` keys in ascending tag-letter order.
+6. BLAKE3-hash the resulting bytes.
+
+This produces a deterministic hash that is stable across `Filter` field-order variations and across Rust HashMap ordering randomness. The implementation lives at `crates/nmp-core/src/store/watermarks.rs::canonical_filter_hash(&Filter) -> [u8; 32]` and is the single source of truth for the planner + sync engine + dump format.
+
+A filter with `limit: Some(N)` produces a *different* hash than the same filter without `limit` — because their cache-coverage semantics genuinely differ. A planner that wants to share a watermark across "limit=200" and "limit=500" requests of the same shape strips `limit` before hashing (this is a planner-side optimisation, not a store-side one).
+
+## 4. Migration plumbing
+
+Per `kernel-substrate.md` §2: `DomainModule` declares `SCHEMA_VERSION` and `migrations()`. The store handles applying them at startup.
+
+### 4.1 Registry extension required
+
+The existing `ModuleRegistry` (`crates/nmp-core/src/substrate/mod.rs:36-79`) stores only `ModuleDescriptor { namespace, family, rust_type }` — the concrete `M: DomainModule` type is consumed by the generic `register_domain::<M>()` call and not retained, so the store has no runtime path from a namespace string back to `M::SCHEMA_VERSION` or `M::migrations()`. M3 extends `ModuleDescriptor` for the Domain family with two `fn`-pointer factories — matching the existing `DomainIndex::key_fn: fn(&[u8]) -> ...` pattern (`substrate/domain.rs:18`):
+
+```rust
+// Added in M3 — substrate/domain.rs
+pub struct DomainFactories {
+    pub schema_version: fn() -> u32,
+    pub migrations: fn() -> Vec<DomainMigration>,
+    pub indexes: fn() -> Vec<DomainIndex>,
+}
+
+// ModuleRegistry::register_domain becomes:
+pub fn register_domain<M: DomainModule>(&mut self) {
+    let factories = DomainFactories {
+        schema_version: || M::SCHEMA_VERSION,
+        migrations: M::migrations,
+        indexes: M::indexes,
+    };
+    self.push_domain::<M>(M::NAMESPACE, factories);
+}
+```
+
+The store reads these factories at open time. No `Box<dyn DomainModule>` is required, no trait object-safety constraints are imposed on `DomainModule`, and the change is additive to the existing trait.
+
+### 4.2 Startup sequence
+
+```rust
+pub fn open(path: &Path, modules: &ModuleRegistry) -> Result<Self, StoreError> {
+    let env = open_lmdb_environment(path)?;
+    let meta = env.open_db(Some("_meta"))?;
+    let mut store = Self::bootstrap(env)?;
+
+    // _kernel schema version
+    store.migrate_kernel_schema(&meta)?;
+
+    // each registered DomainModule
+    for (namespace, factories) in modules.domain_factories() {
+        let current = store.read_meta_schema_version(namespace)?;
+        let target = (factories.schema_version)();
+        let mut applied = current;
+        let mut steps = (factories.migrations)();
+        steps.retain(|m| m.from_version >= current && m.to_version <= target);
+        steps.sort_by_key(|m| m.from_version);
+        for step in steps {
+            store.run_migration_step(namespace, step)?;
+            applied = step.to_version;
+            store.write_meta_schema_version(namespace, applied)?;
+        }
+        if applied < target {
+            return Err(StoreError::MigrationFailed { /* missing step */ });
+        }
+        if applied > target {
+            return Err(StoreError::SchemaTooNew { /* downgrade */ });
+        }
+    }
+    Ok(store)
+}
+```
+
+Each `run_migration_step` opens its own `RwTxn`, calls `step.apply(&mut migration_tx)`, drains `migration_tx.writes()` into the relevant sub-db, and commits. Either the whole step lands atomically or LMDB rolls it back on commit failure.
+
+### 4.3 Rollback semantics
+
+LMDB does not support cross-process downgrade; once `_meta.<namespace>.schema_version` is bumped, there is no "undo." Therefore:
+
+- If migration step N fails: `_meta` is **not** bumped; module starts in degraded mode (per [`trait.md`](trait.md) §5); user-visible diagnostic surfaces the failure.
+- If migration step N succeeds but N+1 fails: `_meta` is at N (the highest successful step). The module is "partly migrated"; the same degraded-mode handling applies; on next startup the runner retries from N → N+1.
+- If the user actually needs to downgrade (a forensics use case), they delete the sub-db and re-sync from relays. The `nmp dump` format is the supported escape hatch.
+
+### 4.4 Cross-module migration coordination (deferred — see open questions)
+
+`kernel-substrate.md` §12 already flags "cross-module migration coordination" as an open question. M3 implements the per-module path; cross-module coordination is deferred to a future module that needs it. The trait surface does not preclude adding it later: a future `Vec<NamespaceDependency>` field on `DomainMigration` would let the runner topologically order modules' migrations.
+
+## 5. The kernel store's own schema
+
+The kernel's internal LMDB layout (provenance + secondaries + tombstones + watermarks) is itself versioned via the reserved `_kernel` namespace in `_meta`. Current version: `1`. A future schema change (e.g., a new secondary index) bumps this and runs a one-time rebuild from primary events.
+
+The schema migration for `_kernel` v1 → v2 is privileged: it can read every sub-db and rewrite as needed. It runs **before** any `DomainModule` migration so module code never sees a stale kernel schema.
+
+## 6. Diagnostics surface
+
+`WatermarkRow` and `ProvenanceRow` are read by the ADR-0007 diagnostics bridge:
+
+- `LogicalInterestStatus.cache_coverage` resolves from `coverage()` (§1.1 above) for the relevant `(filter_hash, relay_url)` pair.
+- The firehose-tap diagnostic surfaces per-event provenance: which relays contributed each row, with `primary` flagged.
+- `StoreHealth.watermark_count` (per [`gc.md`](gc.md) §7) summarises the global count.
+
+These are read-mostly paths; the only contention is between the actor's `write_watermark` calls (driven by M4 sync completions) and the diagnostics query, which is bounded by LMDB's MVCC reader semantics — no lock contention.

Review: (1) doctrine compliance (in particular D0 + D2 — composite reverse index + working-set bound must remain enforced through the trait); (2) any TODO/FIXME/unimplemented; (3) file-size compliance; (4) consistency of EventStore trait surface across the 6 sub-docs; (5) the choice of nostr-lmdb as on-disk byte store vs roll-our-own — is the gap analysis honest?; (6) the GC eviction algorithm + budget; (7) the migration plumbing's testability; (8) any hidden shortcuts. Be terse; name file:line + fix for any concern.
codex
I’ll ground the review in the current tree rather than just the pasted diff: first the local doctrine/plan/spec docs, then the six new design files for line-level consistency and size issues.
exec
/bin/zsh -lc 'pwd && rg -n "Doctrine|D0|D1|D2|D3|D4|D5|composite reverse|working-set|working set|EventStore|nostr-lmdb|TODO|FIXME|unimplemented|todo!|panic!" docs crates AGENTS.md' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
/Users/pablofernandez/Work/nostr-multi-platform
docs/perf/orchestration-log.md:9:| 2026-05-18 01:55 | 1 | Wave 1 completed in ~30 min. Landed: T7 build-verifier f1e374b (all gates green, sim screenshot captured); T6 debt-auditor d660735 (0 TODO/FIXME, 0 unimplemented in FFI surface, 4 must-fix safety-comment items); T2 m2-designer 031fc07 (subscription compilation spec, 8 files); T3 m3-designer 9fead0e (LMDB schema spec, 6 files); T5 m11-designer 0dfb975 (M11 podcast design, 13 files); T4 m105-designer's ffi-hardening files (5) absorbed into orchestrator commit fcf8b43. Three codex post-merge reviews ran: review-1 on the wave-1 cumulative diff flagged 5 issues (debt-inventory exit-ready language, NIP-XX placeholder, ADR-deferral escape, file sizes); review-2 on fcf8b43 flagged 6 issues (D5 mismatch, pre-merge CI gate, freed-pointer UB, S10 conditional, file sizes again, symbol count); review-3 on 0dfb975 flagged 7 issues (Swift file sizes 898/642, MCP-in-CI, AudioPlayback policy, EmbeddingId cycle, rig.rs weakened, OPML pixel-parity, view count). All flagged issues are being addressed via dedicated fix-it tasks T8 (codex-fixer-1) and T11 (codex-fixer-2). T1 (m1-hardener) unblocked + dispatched. T9 (ffi-safety-commenter) dispatched for the 4 must-fix items. Post-merge codex review protocol established and entered memory. |
docs/perf/m1/build-verification.md:139:- Simulator chosen: **iPhone 17 / iOS 26.5** (UUID `C380BA01-AD22-4F4A-8524-A309DA15144C`)
docs/aim.md:101:### 4.1 Reactive single source of truth ("EventStore")
docs/aim.md:219:## 6. Doctrine — the rules the API must make hard to violate
docs/aim.md:253:- **`rust-nostr`** workspace on GitHub — the protocol foundation. We depend on its `nostr`, `nostr-sdk`, `nostr-database`, `nostr-lmdb`, `nostr-ndb`, `nostr-sqlite`, `nostr-gossip`, `nostr-connect`, `nostr-keyring`, `nostr-blossom`, `nostr-relay-builder`, and `nwc` crates.
docs/perf/firehose-bench/2026-05-17-comprehensive-findings.md:73:- `working_set_100_views`: 1,000,000 cached events, 10,000 hot events, 100 open views, about 19.79 MB modeled hot working-set memory.
docs/perf/firehose-bench/2026-05-17-comprehensive-findings.md:78:The reactivity model is directionally strong. Composite reverse indexes, per-view delta gates, batching, coalescing, and a hot/cold working-set split are the right defaults. The benchmark specifically supports the Applesauce/NDK-derived lesson that the UI should express interest declaratively while the core dedupes work and emits bounded deltas.
docs/perf/firehose-bench/2026-05-17-comprehensive-findings.md:82:The memory figures are modeled as actor hot working set plus indexes and open views. Cold event bodies are treated as disk-resident. The allocation measurement uses a process-wide counting allocator and only samples the post-warmup hot path.
docs/perf/firehose-bench/2026-05-17-comprehensive-findings.md:165:relay <-> rust actor <-> durable store / hot working set
docs/perf/firehose-bench/2026-05-17-comprehensive-findings.md:203:3. Add focused unit tests around the composite reverse index, coalescer, and domain-keyed wrapper lifecycle.
docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/m10.5/debt-inventory.md:169:**Issue:** Casts a `usize` (which can be >u32 on 64-bit systems) to `u32` after explicit saturation at `u32::MAX`. This is intentional: if there are more than 2^32 profile claims (which will not occur in practice given working-set bounds), cap the refcount at u32::MAX. No silent overflow.
docs/perf/m10.5/debt-inventory.md:195:**Issue:** Casting collection `.count()` and arithmetic results to metric types. No overflow risk because counts are bounded by working set size constraints (max 5,000 stored events per ADR-0001; max visible 500 per FFI clamp).
docs/perf/m10.5/debt-inventory.md:270:## 3. Doctrine Violations
docs/perf/m10.5/debt-inventory.md:272:### D0 Audit: Kernel Never Grows App Nouns
docs/perf/m10.5/debt-inventory.md:286:### D1 Audit: Best-Effort Rendering with Placeholders
docs/perf/m10.5/debt-inventory.md:299:### D2 Audit: Reactivity Contract (Composite Reverse Index)
docs/perf/m10.5/debt-inventory.md:313:### D3 Audit: Errors Never Cross FFI
docs/perf/m10.5/debt-inventory.md:327:### D4 Audit: One Writer Per Fact
docs/perf/m10.5/debt-inventory.md:341:### D5 Audit: Capabilities Report, Never Decide
docs/perf/m10.5/debt-inventory.md:359:| 14 integer casts (count→metric types) (F6–F14) | Justified | All bounded by design constraints (metrics counters, working-set bounds); no overflow risk. Casts are intentional and safe. |
docs/perf/m10.5/debt-inventory.md:361:| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
docs/perf/m10.5/debt-inventory.md:362:| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
docs/perf/m10.5/debt-inventory.md:383:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/m10.5/debt-inventory.md:396:The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
docs/perf/m10.5/debt-inventory.md:404:- Doctrine review signed in writing in `docs/perf/m10.5/doctrine-review.md`
docs/perf/reactivity-bench/1779045194-run-002.json:655:        "working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100",
docs/perf/reactivity-bench/1779045194-run-002.json:662:    "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.",
crates/nmp-testing/bin/reactivity-bench/scenario.rs:281:                "working-set gate scenario: cached_events={}, hot_events={}, open_views={}",
docs/perf/reactivity-bench/1779050935-run-002.md:44:- working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100
docs/perf/reactivity-bench/1779050935-run-002.md:49:- Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.
docs/plan.md:9:> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
docs/plan.md:19:- **Kernel substrate** in `crates/nmp-core` (~3,800 LOC): actor on a dedicated OS thread, mailbox-driven (ADR feedback adopted — relay reads happen in tokio reader tasks, the actor blocks on its own channel with deadline timeouts), substrate trait families (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`) in `nmp-core/src/substrate/`, ingest pipeline (`kernel/ingest.rs`), claim/release refcounting for profile interest (commit `23ae829`), composite reverse-index dependency tracking.
docs/plan.md:20:- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
docs/plan.md:23:- **Benches** in `crates/nmp-testing`: `reactivity-bench` (composite-key reverse index + coalescer + working set; run 002 passed all ADR-0001..0004 gates) and `firehose-bench` (replay + capture + live modes; replay scenarios pass the modeled budget contract).
docs/plan.md:65:4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
docs/plan.md:66:5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
docs/plan.md:100:- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
docs/plan.md:147:**Scope.** Swap in-memory `EventStore` for LMDB via `Box<dyn EventStore>`. Implement the full insert invariants from `product-spec.md` §7.1: parameterized replaceable events (kind 30000–39999 by `(pubkey, kind, d-tag)`), kind:5 delete handling with tombstone persistence, NIP-40 expiration scheduling, dedup with provenance merge, claim-based GC running.
docs/plan.md:152:- `EventStore` trait abstracted; LMDB backend; in-memory backend kept for tests.
docs/plan.md:154:- GC working set policy per ADR-0003: hot ≤ 10k events resident + claim-pinned set; cold on disk.
docs/plan.md:271:**Scope.** Per spec doctrine D4 (single writer per fact) extended to account scope:
docs/plan.md:335:- Profile picture update through compose → kind:0 republish with new Blossom URL → in-place refinement across all open Profile / Timeline payloads (per doctrine D1).
docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/plan.md:356:  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Hard zero — no deferral escape, no "tracking issue" carve-out. Every pre-existing one is resolved in M10.5. If something genuinely cannot be done in M10.5 because it belongs to a later milestone (e.g. NIP-65 outbox work), then it is not a TODO/FIXME in the scoped files — it lives as a milestone task in `docs/plan.md`, not as a code marker.
docs/plan.md:380:- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
docs/plan.md:717:| Subsystem integration | `cargo test --test '*'` in `nmp-testing` | EventStore + planner + sync against MockRelay | `crates/nmp-testing/tests/` |
docs/plan.md:720:| Reactivity bench | `reactivity-bench --standard --fail-on-gate` | Composite reverse index, delta coalescing, working-set memory, allocation gates | `crates/nmp-testing/bin/reactivity-bench/` |
docs/plan.md:769:Every design doc has measurable gates. Gates run on the reactivity-bench harness (or `firehose-bench` for end-to-end behavior). Failures revise the design **before** implementation. Pre-implementation measurement is cheaper than post-implementation rework. Run 001 of reactivity-bench established the pattern: the reverse-index direction was validated (100×–1000× headroom), one design refinement landed (composite keys), and two budget bugs surfaced (per-view delta, working-set memory) — all before any view-kind code shipped.
crates/nmp-testing/bin/reactivity-bench/main.rs:23:            "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.".to_string(),
docs/perf/codex-reviews/031fc07.md:15:You are reviewing merge 031fc07 (M2 subscription compilation + outbox routing design) on master in nostr-multi-platform. Doctrine D0-D5 (D0 kernel never grows app nouns, D1 best-effort rendering, D2 reactivity contract, D3 no errors across FFI, D4 one writer per fact, D5 capabilities report don't decide). File size: 300 LOC soft, 500 hard.
docs/perf/codex-reviews/031fc07.md:547:+- **Mailbox cache exists but no consumer.** `crates/nmp-core/src/kernel/ingest.rs:209-233` (`ingest_relay_list`) already parses kind:10002 into `self.author_relay_lists: HashMap<String, AuthorRelayList>` (declared at `crates/nmp-core/src/kernel/mod.rs:269-275` and reserved at `mod.rs:313`). The cache is written; **nothing reads it for routing**. This is the bug doctrine D5 ("capabilities report, never decide") inverted: we have the data, we ignore it.
docs/perf/codex-reviews/031fc07.md:1112:+- The local store insert happens *before* the publish step (optimistic insert), with rollback on `PartiallyFailed` if `required_success_count` is not met. This matches the "atomic with reversibility" reading of doctrine D4 (single writer per fact).
docs/perf/codex-reviews/031fc07.md:1567:Review for: (1) doctrine compliance (in particular D0 — does nmp-nip65 propose any kernel-side type leaks? D4 — single source for relay-set facts?); (2) any TODO/FIXME/unimplemented; (3) file-size compliance (any new file > 300 LOC?); (4) internal consistency across the 8 sub-docs; (5) compatibility with the upcoming M3 LMDB schema (e.g. watermark table); (6) the planned test (m2_subscription_compilation_audit) exit criteria; (7) any hidden shortcuts. Be terse; name file:line + fix for any concern.
docs/perf/codex-reviews/031fc07.md:1578:/bin/zsh -lc 'rg -n "nostr-multi-platform|subscription compilation|M2|031fc07|D0|D4|nmp-nip65|outbox" /Users/pablofernandez/.codex/memories/MEMORY.md' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/031fc07.md:1580:1384:- the compile-safe verification path here was `xcodebuild build -scheme Podcastr -project Podcastr.xcodeproj -destination 'platform=iOS Simulator,id=18354708-738D-4D15-B86C-4FDED4D43A93'`, followed by `xcrun simctl launch booted io.f7z.podcast` [Task 1]
docs/perf/codex-reviews/031fc07.md:1581:1491:- the useful focused verification command was `xcodebuild -project Podcastr.xcodeproj -scheme Podcastr -destination 'id=18354708-738D-4D15-B86C-4FDED4D43A93' -derivedDataPath Derived -only-testing:PodcastrTests/PersistenceDurabilityTests -only-testing:PodcastrTests/OPMLImportTests -only-testing:PodcastrTests/DataExportTests -only-testing:PodcastrTests/AppStateStorePerformanceTests test`, which passed with `32 tests, 0 failures` in the clean worktree [Task 2]
docs/perf/codex-reviews/031fc07.md:1621:/bin/zsh -lc 'rg -n "TODO|FIXME|unimplemented|panic!|todo!|unwrap'"\\(|TBD|stub|placeholder|does not exist yet|proposed|deferred|open question|future|post-M2|M6|M3|M4|M5|M8|M9\" docs/design/subscription-compilation.md docs/design/subscription-compilation" in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/031fc07.md:1698:/bin/zsh -lc 'rg -n "D0|D1|D2|D3|D4|D5|Doctrine|kernel never grows|single writer|capabilities report|M2|subscription compiler|subscription compilation|outbox|NIP-65|watermark|LMDB|relay-set|relay set" docs/plan.md docs/aim.md docs/product-spec/subsystems.md docs/design/kernel-substrate.md docs/design/app-extension-kernel.md docs/decisions/0007-diagnostics-and-non-nostr-domain-data.md docs/design/ndk-applesauce-lessons.md docs/design/reactivity/*.md' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/031fc07.md:1700:docs/design/reactivity/validation-harness.md:39:- Spawns a configurable `EventStore` (in-memory, LMDB, or SQLite backend).
docs/perf/codex-reviews/031fc07.md:1721:docs/design/kernel-substrate.md:173:- Pre-formatted display fields per doctrine D1.
docs/perf/codex-reviews/031fc07.md:1732:docs/aim.md:219:## 6. Doctrine — the rules the API must make hard to violate
docs/perf/codex-reviews/031fc07.md:1739:docs/product-spec/subsystems.md:74:Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
docs/perf/codex-reviews/031fc07.md:1743:docs/product-spec/subsystems.md:181:**Best-effort field contract (per doctrine D1).** Every display-bearing field in every view payload is **non-optional** and has a defined placeholder when the underlying data is missing:
docs/perf/codex-reviews/031fc07.md:1744:docs/product-spec/subsystems.md:242:Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.
docs/perf/codex-reviews/031fc07.md:1750:docs/plan.md:9:> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
docs/perf/codex-reviews/031fc07.md:1751:docs/plan.md:20:- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
docs/perf/codex-reviews/031fc07.md:1754:docs/plan.md:65:4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
docs/perf/codex-reviews/031fc07.md:1755:docs/plan.md:66:5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
docs/perf/codex-reviews/031fc07.md:1756:docs/plan.md:100:- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
docs/perf/codex-reviews/031fc07.md:1767:docs/plan.md:147:**Scope.** Swap in-memory `EventStore` for LMDB via `Box<dyn EventStore>`. Implement the full insert invariants from `product-spec.md` §7.1: parameterized replaceable events (kind 30000–39999 by `(pubkey, kind, d-tag)`), kind:5 delete handling with tombstone persistence, NIP-40 expiration scheduling, dedup with provenance merge, claim-based GC running.
docs/perf/codex-reviews/031fc07.md:1769:docs/plan.md:152:- `EventStore` trait abstracted; LMDB backend; in-memory backend kept for tests.
docs/perf/codex-reviews/031fc07.md:1774:docs/plan.md:271:**Scope.** Per spec doctrine D4 (single writer per fact) extended to account scope:
docs/perf/codex-reviews/031fc07.md:1776:docs/plan.md:335:- Profile picture update through compose → kind:0 republish with new Blossom URL → in-place refinement across all open Profile / Timeline payloads (per doctrine D1).
docs/perf/codex-reviews/031fc07.md:1777:docs/plan.md:356:  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
docs/perf/codex-reviews/031fc07.md:1778:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Hard zero — no deferral escape, no "tracking issue" carve-out. Every pre-existing one is resolved in M10.5. If something genuinely cannot be done in M10.5 because it belongs to a later milestone (e.g. NIP-65 outbox work), then it is not a TODO/FIXME in the scoped files — it lives as a milestone task in `docs/plan.md`, not as a code marker.
docs/perf/codex-reviews/031fc07.md:1779:docs/plan.md:380:- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
docs/perf/codex-reviews/031fc07.md:1914:/bin/zsh -lc 'rg -n "nmp-nip65|nmp-core|protocol module|DomainModule|ViewModule|ActionModule|extension|module composition|LMDB|watermark|EventStore|mailbox|relay" docs/design docs/product-spec docs/decisions | head -n 200' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/031fc07.md:1940:docs/decisions/0009-app-extension-kernel-boundary.md:88:- **Two fixture apps prove the boundary in v1.** A tiny non-Nostr-shaped fixture (e.g., a TODO/notes module with an app-local identity) lands in 1a.1; the Twitter clone is the first Nostr-shaped extension module. Together they prove the kernel works in both directions.
docs/perf/codex-reviews/031fc07.md:1960:docs/product-spec/cli-toolchain-phasing.md:174:- **EventStore.** The reactive single source of truth for all Nostr events. Owned by the actor; not exposed at FFI.
docs/perf/codex-reviews/031fc07.md:1962:docs/product-spec/cli-toolchain-phasing.md:177:- **View.** A pre-built derived projection of `EventStore` contents. Opened by `OpenView` action; payload arrives via `AppState.views` / `ViewBatch`.
docs/perf/codex-reviews/031fc07.md:1964:docs/product-spec/subsystems.md:7:### 7.1 EventStore
docs/perf/codex-reviews/031fc07.md:1982:docs/product-spec/subsystems.md:74:Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
docs/perf/codex-reviews/031fc07.md:2002:docs/product-spec/subsystems.md:242:Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.
docs/perf/codex-reviews/031fc07.md:2003:docs/product-spec/subsystems.md:247:View opens → Live REQ handler starts → Planner consults coverage → Sync engine reconciles gaps → EventStore inserts → ViewBatch emits
docs/perf/codex-reviews/031fc07.md:2027:docs/product-spec/appendices.md:17:The event store, gossip cache, sync watermarks, working set, and signer state all live in the actor and **never cross FFI**.
docs/perf/codex-reviews/031fc07.md:2063:docs/product-spec/overview-and-dx.md:31:### D0. Kernel + extension modules — no app nouns in `nmp-core`
docs/perf/codex-reviews/031fc07.md:2067:docs/product-spec/overview-and-dx.md:60:### D3. Outbox routing is automatic; manual relay selection is the opt-out
docs/perf/codex-reviews/031fc07.md:2091:docs/decisions/0006-vertical-slice-first.md:13:The classic failure mode at this stage is **horizontal expansion** — building "the EventStore" comprehensively, then "the planner" comprehensively, then "the views" comprehensively, then finally stitching them together at the end, only to discover that the FFI surface or the relay adapter or the storage backend doesn't actually compose the way the model assumed.
docs/perf/codex-reviews/031fc07.md:2095:docs/decisions/0006-vertical-slice-first.md:48:│  EventStore (minimal)                                        │
docs/perf/codex-reviews/031fc07.md:2107:docs/decisions/0006-vertical-slice-first.md:94:- A real WebSocket → real EventStore → real DeltaBuffer → real component update is measurable end-to-end.
docs/perf/codex-reviews/031fc07.md:2112:docs/decisions/0006-vertical-slice-first.md:112:- **Subsequent expansion has a working substrate to build on.** Adding LMDB is a `Box<dyn EventStore>` swap. Adding multi-relay is a planner change. Adding negentropy is a planner change. Adding iOS is a UniFFI wrap of the existing actor. None require redesigning the architecture.
docs/perf/codex-reviews/031fc07.md:2561:   215	- The local store insert happens *before* the publish step (optimistic insert), with rollback on `PartiallyFailed` if `required_success_count` is not met. This matches the "atomic with reversibility" reading of doctrine D4 (single writer per fact).
docs/perf/codex-reviews/031fc07.md:3465:    15	- **Mailbox cache exists but no consumer.** `crates/nmp-core/src/kernel/ingest.rs:209-233` (`ingest_relay_list`) already parses kind:10002 into `self.author_relay_lists: HashMap<String, AuthorRelayList>` (declared at `crates/nmp-core/src/kernel/mod.rs:269-275` and reserved at `mod.rs:313`). The cache is written; **nothing reads it for routing**. This is the bug doctrine D5 ("capabilities report, never decide") inverted: we have the data, we ignore it.
docs/perf/codex-reviews/031fc07.md:3656:    74	Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
docs/perf/codex-reviews/031fc07.md:3717:docs/aim.md:101:### 4.1 Reactive single source of truth ("EventStore")
docs/perf/codex-reviews/031fc07.md:3732:docs/aim.md:219:## 6. Doctrine — the rules the API must make hard to violate
docs/perf/codex-reviews/031fc07.md:3832:docs/product-spec/subsystems.md:7:### 7.1 EventStore
docs/perf/codex-reviews/031fc07.md:3913:   219	## 6. Doctrine — the rules the API must make hard to violate
docs/perf/codex-reviews/031fc07.md:4055:     7	### 7.1 EventStore
docs/perf/codex-reviews/031fc07.md:4387:- [nip65.md:192](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:192) plus [nip65.md:197](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:197): `nmp-nip65` depends on `nmp-core`, while `nmp-core::kernel::planner` consumes `nmp-nip65`. That is a Rust crate cycle and blurs D0. Fix by moving mailbox traits/types into `nmp-core` or a tiny no-core crate, with `nmp-nip65` only registering module behavior.
docs/perf/codex-reviews/031fc07.md:4389:- [nip65.md:6](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:6), [nip65.md:98](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:98), [compiler.md:52](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:52): mailbox facts have too many apparent writers/read models: `ingest_relay_list`, `MailboxCache`, and `MailboxesView::on_event_inserted`. D4 wants one writer per fact. Fix by making one canonical mailbox projection/cache writer; views only read that projection.
docs/perf/codex-reviews/031fc07.md:4397:- [compiler.md:166](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:166) and [compiler.md:184](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:184): startup timeline/profile/contact behavior remains framed as `nmp-core` migration work. D0 fix: move social bootstrap modules to `nmp-nip01` / `nmp-nip02` / demo app code; core should only execute registered module interests.
docs/perf/codex-reviews/031fc07.md:4401:Other checks: no literal `TODO`, `FIXME`, or `unimplemented!` in the added docs. File sizes are compliant: all new files are under 300 LOC, largest is `tests.md` at 273. The planned audit test still has weak spots: [tests.md:34](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:34) depends on non-existent `AppActionMeta`, and [tests.md:213](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:213) does not actually assert lane distinctness.
docs/perf/codex-reviews/031fc07.md:4410:- [nip65.md:192](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:192) plus [nip65.md:197](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:197): `nmp-nip65` depends on `nmp-core`, while `nmp-core::kernel::planner` consumes `nmp-nip65`. That is a Rust crate cycle and blurs D0. Fix by moving mailbox traits/types into `nmp-core` or a tiny no-core crate, with `nmp-nip65` only registering module behavior.
docs/perf/codex-reviews/031fc07.md:4412:- [nip65.md:6](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:6), [nip65.md:98](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/nip65.md:98), [compiler.md:52](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:52): mailbox facts have too many apparent writers/read models: `ingest_relay_list`, `MailboxCache`, and `MailboxesView::on_event_inserted`. D4 wants one writer per fact. Fix by making one canonical mailbox projection/cache writer; views only read that projection.
docs/perf/codex-reviews/031fc07.md:4420:- [compiler.md:166](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:166) and [compiler.md:184](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/compiler.md:184): startup timeline/profile/contact behavior remains framed as `nmp-core` migration work. D0 fix: move social bootstrap modules to `nmp-nip01` / `nmp-nip02` / demo app code; core should only execute registered module interests.
docs/perf/codex-reviews/031fc07.md:4424:Other checks: no literal `TODO`, `FIXME`, or `unimplemented!` in the added docs. File sizes are compliant: all new files are under 300 LOC, largest is `tests.md` at 273. The planned audit test still has weak spots: [tests.md:34](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:34) depends on non-existent `AppActionMeta`, and [tests.md:213](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/subscription-compilation/tests.md:213) does not actually assert lane distinctness.
docs/decisions/0009-app-extension-kernel-boundary.md:76:- ADR-0003 (working-set memory): applies to the kernel's hot/cold split regardless of module count.
docs/decisions/0009-app-extension-kernel-boundary.md:81:The doctrines from `product-spec.md` §1.5 (D1 best-effort rendering, D2 negentropy first, D3 outbox automatic, D4 single writer per fact, D5 snapshots bounded by what's open) survive intact and apply across all modules.
docs/decisions/0009-app-extension-kernel-boundary.md:88:- **Two fixture apps prove the boundary in v1.** A tiny non-Nostr-shaped fixture (e.g., a TODO/notes module with an app-local identity) lands in 1a.1; the Twitter clone is the first Nostr-shaped extension module. Together they prove the kernel works in both directions.
docs/perf/reactivity-bench/1779051783-run-002.json:655:        "working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100",
docs/perf/reactivity-bench/1779051783-run-002.json:662:    "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.",
docs/decisions/0005-domain-keyed-platform-shadow.md:59:- **Three-tier data model becomes explicit.** Rust durable storage → Rust working set + projections → platform domain-keyed shadow. Each layer derives from the layer below; only Rust is source of truth.
docs/perf/codex-reviews/fcf8b43.md:15:You are reviewing merge fcf8b431b8d86f1801fef2fad26e81fbf56506f4 on master in the nostr-multi-platform repo. Doctrine D0–D5 (kernel never grows app nouns; best-effort rendering; reactivity contract ≤60 Hz/view; errors never cross FFI; one writer per fact; capabilities report don't decide). File-size: 300 LOC soft, 500 hard.
docs/perf/codex-reviews/fcf8b43.md:85:+   (`docs/product-spec/overview-and-dx.md` §1.5 D0–D5), and every ownership
docs/perf/codex-reviews/fcf8b43.md:132:+| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
docs/perf/codex-reviews/fcf8b43.md:201:+| S1 | Mount/unmount churn | actor recv + refcount | D5 (snapshot bounded), bible #5 |
docs/perf/codex-reviews/fcf8b43.md:203:+| S3 | Snapshot pressure | listener serialization | bible #9 (≤60 Hz), D5 |
docs/perf/codex-reviews/fcf8b43.md:204:+| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
docs/perf/codex-reviews/fcf8b43.md:207:+| S7 | Error-shape exhaustion | every invalid input path | D3 (no errors cross FFI) |
docs/perf/codex-reviews/fcf8b43.md:208:+| S8 | Subscription planner DOS | OpenView/CloseView storm | D2 (≤60 Hz/view), D5 |
docs/perf/codex-reviews/fcf8b43.md:209:+| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
docs/perf/codex-reviews/fcf8b43.md:263:+├── doctrine-review.md       # D0–D5 sign-off (M10.5 exit-gate artifact)
docs/perf/codex-reviews/fcf8b43.md:283:+| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
docs/perf/codex-reviews/fcf8b43.md:291:+grep -rEn '(TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit)' \
docs/perf/codex-reviews/fcf8b43.md:301:+### 7.2 D3 structural gap (named, not hidden)
docs/perf/codex-reviews/fcf8b43.md:307:+caller and without setting any state field. This is **D3-compliant in
docs/perf/codex-reviews/fcf8b43.md:309:+D3-incomplete in the user-visible sense** (no toast surfaces in
docs/perf/codex-reviews/fcf8b43.md:312:+The debt-inventory's D3 audit (lines 317–334) concludes the same:
docs/perf/codex-reviews/fcf8b43.md:327:+## 8. Doctrine review checklist
docs/perf/codex-reviews/fcf8b43.md:329:+Full D0–D5 line-item-to-scenario mapping in
docs/perf/codex-reviews/fcf8b43.md:332:+| Doctrine | Proven by |
docs/perf/codex-reviews/fcf8b43.md:334:+| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
docs/perf/codex-reviews/fcf8b43.md:335:+| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
docs/perf/codex-reviews/fcf8b43.md:336:+| **D2** ≤60Hz/view, working-set bound | S2, S3, S8 — emit-rate cap, planner dedup |
docs/perf/codex-reviews/fcf8b43.md:337:+| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
docs/perf/codex-reviews/fcf8b43.md:338:+| **D4** one writer per fact | S1, S5 — refcount only mutated on actor thread; reentrancy under same single-writer rule |
docs/perf/codex-reviews/fcf8b43.md:339:+| **D5** snapshots bounded by what's open | S1 (refcount drives eviction) + S3 (full-state size scales with open views, not store) |
docs/perf/codex-reviews/fcf8b43.md:472:+        --require-doctrines D0,D1,D2,D3,D4,D5 \
docs/perf/codex-reviews/fcf8b43.md:568:+            fails.append("FFI grep yielded TODO/FIXME tokens; see §7.1")
docs/perf/codex-reviews/fcf8b43.md:724:+4. Doctrine review (D0–D5) signed off in `doctrine-review.md`.
docs/perf/codex-reviews/fcf8b43.md:744:+2. **§D** doctrine D0–D5 review checklist — each line item maps to
docs/perf/codex-reviews/fcf8b43.md:844:+| Peak working-set RSS during storm | `<=` | 150 MiB | 200 MiB |
docs/perf/codex-reviews/fcf8b43.md:887:+## §D. Doctrine D0–D5 review checklist
docs/perf/codex-reviews/fcf8b43.md:892:+> **Note.** The task brief mentioned "D0–D5". The canonical list in
docs/perf/codex-reviews/fcf8b43.md:893:+> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
docs/perf/codex-reviews/fcf8b43.md:896:+> not redundantly re-prove — items beyond D0–D5 are covered by
docs/perf/codex-reviews/fcf8b43.md:899:+### D0. Kernel never grows app nouns
docs/perf/codex-reviews/fcf8b43.md:901:+- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
docs/perf/codex-reviews/fcf8b43.md:908:+  `docs/perf/m10.5/doctrine-review.md` § D0.
docs/perf/codex-reviews/fcf8b43.md:910:+### D1. Best-effort rendering — render now, refine in place
docs/perf/codex-reviews/fcf8b43.md:922:+- 📝 **Sign-off:** doctrine-review.md § D1.
docs/perf/codex-reviews/fcf8b43.md:924:+### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
docs/perf/codex-reviews/fcf8b43.md:930:+- ✅ **Stress proof (working set):** S8 asserts planner state
docs/perf/codex-reviews/fcf8b43.md:934:+- 📝 **Sign-off:** doctrine-review.md § D2.
docs/perf/codex-reviews/fcf8b43.md:936:+### D3. Errors never cross FFI
docs/perf/codex-reviews/fcf8b43.md:938:+- ⚠️ **Current state:** debt-inventory §3 D3 audit concludes
docs/perf/codex-reviews/fcf8b43.md:947:+- 📝 **Sign-off:** doctrine-review.md § D3, with the explicit
docs/perf/codex-reviews/fcf8b43.md:948:+  note that this milestone *closes* the D3-incomplete state
docs/perf/codex-reviews/fcf8b43.md:951:+### D4. Single writer per fact — caches derive
docs/perf/codex-reviews/fcf8b43.md:953:+- ✅ **Proof:** debt-inventory §3 D4 audit — single-threaded actor
docs/perf/codex-reviews/fcf8b43.md:966:+- 📝 **Sign-off:** doctrine-review.md § D4.
docs/perf/codex-reviews/fcf8b43.md:968:+### D5. Snapshots bounded by what's open
docs/perf/codex-reviews/fcf8b43.md:978:+- 📝 **Sign-off:** doctrine-review.md § D5.
docs/perf/codex-reviews/fcf8b43.md:982:+## §D.1 Doctrine sign-off artifact
docs/perf/codex-reviews/fcf8b43.md:988:+# M10.5 Doctrine Review
docs/perf/codex-reviews/fcf8b43.md:990:+| Doctrine | Status | Evidence | Reviewer | Date |
docs/perf/codex-reviews/fcf8b43.md:992:+| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:993:+| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:994:+| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:995:+| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:996:+| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:997:+| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:1687:+working set explodes; relay workers can't send fast enough.
docs/perf/codex-reviews/fcf8b43.md:1698:+1. Peak working-set memory during storm ≤ **150 MB** (planner is the
docs/perf/codex-reviews/fcf8b43.md:1800:+You are reviewing a session's worth of merges on master in the nostr-multi-platform repo. NMP is a Rust multiplatform framework for Nostr apps building toward v1 per docs/plan.md. Doctrine D0–D5 (docs/product-spec.md §1.5):
docs/perf/codex-reviews/fcf8b43.md:1801:+- D0 kernel never grows app nouns
docs/perf/codex-reviews/fcf8b43.md:1802:+- D1 best-effort rendering with placeholders
docs/perf/codex-reviews/fcf8b43.md:1803:+- D2 reactivity contract (composite reverse index, ≤60 Hz/view, working-set bound)
docs/perf/codex-reviews/fcf8b43.md:1804:+- D3 errors never cross FFI (become toast state fields)
docs/perf/codex-reviews/fcf8b43.md:1805:+- D4 one writer per fact
docs/perf/codex-reviews/fcf8b43.md:1806:+- D5 capabilities report, never decide
docs/perf/codex-reviews/fcf8b43.md:1828:+- 0 critical debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!)
docs/perf/codex-reviews/fcf8b43.md:1830:+- All 5 cardinal doctrines (D0–D5) compliant
docs/perf/codex-reviews/fcf8b43.md:1839:+Doctrine compliance (exit gates for M10.5):
docs/perf/codex-reviews/fcf8b43.md:1840:+✅ D0 (kernel never grows app nouns): kernel is domain-agnostic
docs/perf/codex-reviews/fcf8b43.md:1841:+✅ D1 (best-effort rendering): ProfileCard.placeholder renders immediately
docs/perf/codex-reviews/fcf8b43.md:1842:+✅ D2 (reactivity contract): all updates flow through composite reverse index
docs/perf/codex-reviews/fcf8b43.md:1843:+✅ D3 (errors never cross FFI): errors as advisory JSON data, not FFI codes
docs/perf/codex-reviews/fcf8b43.md:1844:+✅ D4 (one writer per fact): kernel actor is single-threaded
docs/perf/codex-reviews/fcf8b43.md:1845:+✅ D5 (capabilities report): iOS bridge is pure relay, no policy decisions
docs/perf/codex-reviews/fcf8b43.md:1871:+- Zero open TODO/FIXME/unimplemented in FFI/actor/relay/kernel/iOS bridge
docs/perf/codex-reviews/fcf8b43.md:1910:++| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/fcf8b43.md:2067:++**Issue:** Casts a `usize` (which can be >u32 on 64-bit systems) to `u32` after explicit saturation at `u32::MAX`. This is intentional: if there are more than 2^32 profile claims (which will not occur in practice given working-set bounds), cap the refcount at u32::MAX. No silent overflow.
docs/perf/codex-reviews/fcf8b43.md:2093:++**Issue:** Casting collection `.count()` and arithmetic results to metric types. No overflow risk because counts are bounded by working set size constraints (max 5,000 stored events per ADR-0001; max visible 500 per FFI clamp).
docs/perf/codex-reviews/fcf8b43.md:2168:++## 3. Doctrine Violations
docs/perf/codex-reviews/fcf8b43.md:2170:++### D0 Audit: Kernel Never Grows App Nouns
docs/perf/codex-reviews/fcf8b43.md:2184:++### D1 Audit: Best-Effort Rendering with Placeholders
docs/perf/codex-reviews/fcf8b43.md:2197:++### D2 Audit: Reactivity Contract (Composite Reverse Index)
docs/perf/codex-reviews/fcf8b43.md:2211:++### D3 Audit: Errors Never Cross FFI
docs/perf/codex-reviews/fcf8b43.md:2225:++### D4 Audit: One Writer Per Fact
docs/perf/codex-reviews/fcf8b43.md:2239:++### D5 Audit: Capabilities Report, Never Decide
docs/perf/codex-reviews/fcf8b43.md:2261:++| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
docs/perf/codex-reviews/fcf8b43.md:2262:++| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
docs/perf/codex-reviews/fcf8b43.md:2292:++- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
docs/perf/codex-reviews/fcf8b43.md:2304:++| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/fcf8b43.md:2320:++**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/fcf8b43.md:2361:++> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
docs/perf/codex-reviews/fcf8b43.md:2386:++**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/fcf8b43.md:2397:++  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
docs/perf/codex-reviews/fcf8b43.md:2404:++- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/fcf8b43.md:2421:++- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
docs/perf/codex-reviews/fcf8b43.md:2577:+1. Doctrine compliance — any rule weakening?
docs/perf/codex-reviews/fcf8b43.md:2578:+2. TODO / FIXME / unimplemented / "for later" snuck in
docs/perf/codex-reviews/fcf8b43.md:2614:+/bin/zsh -lc 'rg -n "TODO|FIXME|XXX|unimplemented!|todo!|for later|later|Optional|optional|recommended|Recommended|No action required|defer|deferral|future|worktree remove --force|dropped" docs/perf/m10.5/debt-inventory.md docs/perf/orchestration-log.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/fcf8b43.md:2616:+docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/fcf8b43.md:2627:+docs/perf/m10.5/debt-inventory.md:406:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/fcf8b43.md:2631:+docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
docs/perf/codex-reviews/fcf8b43.md:2633:+docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
docs/perf/codex-reviews/fcf8b43.md:2635:+docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/fcf8b43.md:2637:+docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/fcf8b43.md:2652:+   363	| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
docs/perf/codex-reviews/fcf8b43.md:2653:+   364	| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
docs/perf/codex-reviews/fcf8b43.md:2683:+   394	- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
docs/perf/codex-reviews/fcf8b43.md:2695:+   406	| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/fcf8b43.md:2711:+   422	**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/fcf8b43.md:2733:+   345	**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/fcf8b43.md:2744:+   356	  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
docs/perf/codex-reviews/fcf8b43.md:2751:+   363	- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/fcf8b43.md:2768:+   380	- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
docs/perf/codex-reviews/fcf8b43.md:2916:+docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/fcf8b43.md:2925:+/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/plan.md | rg -n "deferred|deferral|NIP-XX|optional|for later|wire it up later|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/fcf8b43.md:2927:+/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/perf/m10.5/debt-inventory.md | rg -n "optional|recommended|Recommended|READY|production-ready|No Action Required|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/fcf8b43.md:2929:+18:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/fcf8b43.md:2937:+412:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/fcf8b43.md:2945:+40:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/fcf8b43.md:2946:+58:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/fcf8b43.md:3024:+- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
docs/perf/codex-reviews/fcf8b43.md:3031:+No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
docs/perf/codex-reviews/fcf8b43.md:3041:+- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
docs/perf/codex-reviews/fcf8b43.md:3048:+No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
docs/perf/codex-reviews/fcf8b43.md:3066:+| 14 integer casts (count→metric types) (F6–F14) | Justified | All bounded by design constraints (metrics counters, working-set bounds); no overflow risk. Casts are intentional and safe. |
docs/perf/codex-reviews/fcf8b43.md:3068: | ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
docs/perf/codex-reviews/fcf8b43.md:3069: | Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
docs/perf/codex-reviews/fcf8b43.md:3103:-- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
docs/perf/codex-reviews/fcf8b43.md:3112: | Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/fcf8b43.md:3130:-**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/fcf8b43.md:3135:+The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/fcf8b43.md:3148:+- Doctrine review signed in writing in `docs/perf/m10.5/doctrine-review.md`
docs/perf/codex-reviews/fcf8b43.md:3161:-- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/fcf8b43.md:3162:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Hard zero — no deferral escape, no "tracking issue" carve-out. Every pre-existing one is resolved in M10.5. If something genuinely cannot be done in M10.5 because it belongs to a later milestone (e.g. NIP-65 outbox work), then it is not a TODO/FIXME in the scoped files — it lives as a milestone task in `docs/plan.md`, not as a code marker.
docs/perf/codex-reviews/fcf8b43.md:3176:Review for: (1) doctrine compliance, (2) any TODO/FIXME/unimplemented sneaking in, (3) test coverage where tests should exist, (4) file-size compliance (any new file > 300 LOC?), (5) docs internal consistency (M10.5 gate, M11 pod-NIP language, etc), (6) hidden shortcuts. Be terse. If fine, say so. If concern, name file:line + fix.
docs/perf/codex-reviews/fcf8b43.md:3193:9fead0e design(m3): LMDB schema + EventStore trait + GC policy
docs/perf/codex-reviews/fcf8b43.md:3254:docs/design/ffi-hardening.md:217:| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
docs/perf/codex-reviews/fcf8b43.md:3255:docs/design/ffi-hardening.md:243:D3-incomplete in the user-visible sense** (no toast surfaces in
docs/perf/codex-reviews/fcf8b43.md:3261:docs/design/ffi-hardening.md:271:| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
docs/perf/codex-reviews/fcf8b43.md:3273:docs/plan.md:356:  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
docs/perf/codex-reviews/fcf8b43.md:3304:docs/design/ffi-hardening/gates.md:255:| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3305:docs/design/ffi-hardening/gates.md:256:| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3306:docs/design/ffi-hardening/gates.md:257:| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3307:docs/design/ffi-hardening/gates.md:258:| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3308:docs/design/ffi-hardening/gates.md:259:| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3309:docs/design/ffi-hardening/gates.md:260:| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3312:docs/perf/m10.5/debt-inventory.md:359:| 14 integer casts (count→metric types) (F6–F14) | Justified | All bounded by design constraints (metrics counters, working-set bounds); no overflow risk. Casts are intentional and safe. |
docs/perf/codex-reviews/fcf8b43.md:3324:/bin/zsh -lc 'rg -n "TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit|optional|Optional|recommended|Recommended|defer|deferred|deferral|NIP-XX|production-ready|Exit Criteria|READY|acceptable|fast enough" docs/design/ffi-hardening.md docs/design/ffi-hardening/*.md docs/perf/codex-reviews/2026-05-18-session-1.md docs/perf/m10.5/debt-inventory.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/fcf8b43.md:3392:docs/design/ffi-hardening/gates.md:255:| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3393:docs/design/ffi-hardening/gates.md:256:| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3397:docs/design/ffi-hardening.md:138:| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
docs/perf/codex-reviews/fcf8b43.md:3399:docs/design/ffi-hardening.md:143:| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
docs/perf/codex-reviews/fcf8b43.md:3402:docs/design/ffi-hardening.md:217:| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
docs/perf/codex-reviews/fcf8b43.md:3403:docs/design/ffi-hardening.md:268:| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
docs/perf/codex-reviews/fcf8b43.md:3404:docs/design/ffi-hardening.md:269:| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
docs/perf/codex-reviews/fcf8b43.md:3411:docs/design/ffi-hardening/scenarios.md:261:working set explodes; relay workers can't send fast enough.
docs/perf/codex-reviews/fcf8b43.md:3415:docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/fcf8b43.md:3417:docs/perf/m10.5/debt-inventory.md:383:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/fcf8b43.md:3418:docs/perf/m10.5/debt-inventory.md:396:The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/fcf8b43.md:3423:docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
docs/perf/codex-reviews/fcf8b43.md:3424:docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
docs/perf/codex-reviews/fcf8b43.md:3426:docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/fcf8b43.md:3427:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Hard zero — no deferral escape, no "tracking issue" carve-out. Every pre-existing one is resolved in M10.5. If something genuinely cannot be done in M10.5 because it belongs to a later milestone (e.g. NIP-65 outbox work), then it is not a TODO/FIXME in the scoped files — it lives as a milestone task in `docs/plan.md`, not as a code marker.
docs/perf/codex-reviews/fcf8b43.md:3429:docs/design/ffi-hardening/ci.md:182:            fails.append("FFI grep yielded TODO/FIXME tokens; see §7.1")
docs/perf/codex-reviews/fcf8b43.md:3436:docs/design/ffi-hardening.md:225:grep -rEn '(TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit)' \
docs/perf/codex-reviews/fcf8b43.md:3441:docs/perf/codex-reviews/2026-05-18-session-1.md:43:- 0 critical debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!)
docs/perf/codex-reviews/fcf8b43.md:3445:docs/perf/codex-reviews/2026-05-18-session-1.md:86:- Zero open TODO/FIXME/unimplemented in FFI/actor/relay/kernel/iOS bridge
docs/perf/codex-reviews/fcf8b43.md:3447:docs/perf/codex-reviews/2026-05-18-session-1.md:125:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/fcf8b43.md:3453:docs/perf/codex-reviews/2026-05-18-session-1.md:519:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/fcf8b43.md:3456:docs/perf/codex-reviews/2026-05-18-session-1.md:535:+**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/fcf8b43.md:3461:docs/perf/codex-reviews/2026-05-18-session-1.md:601:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/fcf8b43.md:3462:docs/perf/codex-reviews/2026-05-18-session-1.md:619:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/fcf8b43.md:3465:docs/perf/codex-reviews/2026-05-18-session-1.md:793:2. TODO / FIXME / unimplemented / "for later" snuck in
docs/perf/codex-reviews/fcf8b43.md:3466:docs/perf/codex-reviews/2026-05-18-session-1.md:829:/bin/zsh -lc 'rg -n "TODO|FIXME|XXX|unimplemented!|todo!|for later|later|Optional|optional|recommended|Recommended|No action required|defer|deferral|future|worktree remove --force|dropped" docs/perf/m10.5/debt-inventory.md docs/perf/orchestration-log.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/fcf8b43.md:3467:docs/perf/codex-reviews/2026-05-18-session-1.md:831:docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/fcf8b43.md:3473:docs/perf/codex-reviews/2026-05-18-session-1.md:842:docs/perf/m10.5/debt-inventory.md:406:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/fcf8b43.md:3477:docs/perf/codex-reviews/2026-05-18-session-1.md:846:docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
docs/perf/codex-reviews/fcf8b43.md:3478:docs/perf/codex-reviews/2026-05-18-session-1.md:848:docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
docs/perf/codex-reviews/fcf8b43.md:3480:docs/perf/codex-reviews/2026-05-18-session-1.md:850:docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/fcf8b43.md:3481:docs/perf/codex-reviews/2026-05-18-session-1.md:852:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/fcf8b43.md:3488:docs/perf/codex-reviews/2026-05-18-session-1.md:910:   406	| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/fcf8b43.md:3491:docs/perf/codex-reviews/2026-05-18-session-1.md:926:   422	**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/fcf8b43.md:3494:docs/perf/codex-reviews/2026-05-18-session-1.md:948:   345	**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/fcf8b43.md:3495:docs/perf/codex-reviews/2026-05-18-session-1.md:966:   363	- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/fcf8b43.md:3500:docs/perf/codex-reviews/2026-05-18-session-1.md:1131:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/fcf8b43.md:3507:docs/perf/codex-reviews/2026-05-18-session-1.md:1140:/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/plan.md | rg -n "deferred|deferral|NIP-XX|optional|for later|wire it up later|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/fcf8b43.md:3508:docs/perf/codex-reviews/2026-05-18-session-1.md:1142:/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/perf/m10.5/debt-inventory.md | rg -n "optional|recommended|Recommended|READY|production-ready|No Action Required|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/fcf8b43.md:3509:docs/perf/codex-reviews/2026-05-18-session-1.md:1144:18:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/fcf8b43.md:3515:docs/perf/codex-reviews/2026-05-18-session-1.md:1152:412:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/fcf8b43.md:3521:docs/perf/codex-reviews/2026-05-18-session-1.md:1160:40:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/fcf8b43.md:3522:docs/perf/codex-reviews/2026-05-18-session-1.md:1161:58:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/fcf8b43.md:3527:docs/perf/codex-reviews/2026-05-18-session-1.md:1239:- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
docs/perf/codex-reviews/fcf8b43.md:3529:docs/perf/codex-reviews/2026-05-18-session-1.md:1246:No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
docs/perf/codex-reviews/fcf8b43.md:3532:docs/perf/codex-reviews/2026-05-18-session-1.md:1256:- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
docs/perf/codex-reviews/fcf8b43.md:3534:docs/perf/codex-reviews/2026-05-18-session-1.md:1263:No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
docs/perf/codex-reviews/fcf8b43.md:3573:   107	| Peak working-set RSS during storm | `<=` | 150 MiB | 200 MiB |
docs/perf/codex-reviews/fcf8b43.md:3578:   251	# M10.5 Doctrine Review
docs/perf/codex-reviews/fcf8b43.md:3580:   253	| Doctrine | Status | Evidence | Reviewer | Date |
docs/perf/codex-reviews/fcf8b43.md:3582:   255	| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3583:   256	| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3584:   257	| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3585:   258	| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3586:   259	| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3587:   260	| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3613:    66	| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
docs/perf/codex-reviews/fcf8b43.md:3627:   217	| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
docs/perf/codex-reviews/fcf8b43.md:3635:   225	grep -rEn '(TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit)' \
docs/perf/codex-reviews/fcf8b43.md:3645:   235	### 7.2 D3 structural gap (named, not hidden)
docs/perf/codex-reviews/fcf8b43.md:3651:   241	caller and without setting any state field. This is **D3-compliant in
docs/perf/codex-reviews/fcf8b43.md:3653:   243	D3-incomplete in the user-visible sense** (no toast surfaces in
docs/perf/codex-reviews/fcf8b43.md:3656:   246	The debt-inventory's D3 audit (lines 317–334) concludes the same:
docs/perf/codex-reviews/fcf8b43.md:3779:   182	            fails.append("FFI grep yielded TODO/FIXME tokens; see §7.1")
docs/perf/codex-reviews/fcf8b43.md:3818:I found one clear doctrine mismatch candidate: the new hardening docs appear to redefine D5 as “snapshots bounded by what’s open,” while the session brief and existing rubric define D5 as “capabilities report, never decide.” I’m verifying that against the repo’s canonical docs before calling it.
docs/perf/codex-reviews/fcf8b43.md:3822:   150	## §D. Doctrine D0–D5 review checklist
docs/perf/codex-reviews/fcf8b43.md:3827:   155	> **Note.** The task brief mentioned "D0–D5". The canonical list in
docs/perf/codex-reviews/fcf8b43.md:3828:   156	> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
docs/perf/codex-reviews/fcf8b43.md:3831:   159	> not redundantly re-prove — items beyond D0–D5 are covered by
docs/perf/codex-reviews/fcf8b43.md:3834:   162	### D0. Kernel never grows app nouns
docs/perf/codex-reviews/fcf8b43.md:3836:   164	- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
docs/perf/codex-reviews/fcf8b43.md:3843:   171	  `docs/perf/m10.5/doctrine-review.md` § D0.
docs/perf/codex-reviews/fcf8b43.md:3845:   173	### D1. Best-effort rendering — render now, refine in place
docs/perf/codex-reviews/fcf8b43.md:3857:   185	- 📝 **Sign-off:** doctrine-review.md § D1.
docs/perf/codex-reviews/fcf8b43.md:3859:   187	### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
docs/perf/codex-reviews/fcf8b43.md:3865:   193	- ✅ **Stress proof (working set):** S8 asserts planner state
docs/perf/codex-reviews/fcf8b43.md:3869:   197	- 📝 **Sign-off:** doctrine-review.md § D2.
docs/perf/codex-reviews/fcf8b43.md:3871:   199	### D3. Errors never cross FFI
docs/perf/codex-reviews/fcf8b43.md:3873:   201	- ⚠️ **Current state:** debt-inventory §3 D3 audit concludes
docs/perf/codex-reviews/fcf8b43.md:3882:   210	- 📝 **Sign-off:** doctrine-review.md § D3, with the explicit
docs/perf/codex-reviews/fcf8b43.md:3883:   211	  note that this milestone *closes* the D3-incomplete state
docs/perf/codex-reviews/fcf8b43.md:3886:   214	### D4. Single writer per fact — caches derive
docs/perf/codex-reviews/fcf8b43.md:3888:   216	- ✅ **Proof:** debt-inventory §3 D4 audit — single-threaded actor
docs/perf/codex-reviews/fcf8b43.md:3901:   229	- 📝 **Sign-off:** doctrine-review.md § D4.
docs/perf/codex-reviews/fcf8b43.md:3903:   231	### D5. Snapshots bounded by what's open
docs/perf/codex-reviews/fcf8b43.md:3910:/bin/zsh -lc "rg -n \"D0|D1|D2|D3|D4|D5|capabilities report|never decide|snapshots bounded|bounded by what's open|Capabilities\" docs/product-spec* docs/aim.md docs/plan.md docs/design/ffi-hardening.md docs/design/ffi-hardening/gates.md docs/perf/m10.5/debt-inventory.md" in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/fcf8b43.md:3912:docs/plan.md:9:> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
docs/perf/codex-reviews/fcf8b43.md:3913:docs/plan.md:20:- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
docs/perf/codex-reviews/fcf8b43.md:3914:docs/plan.md:65:4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
docs/perf/codex-reviews/fcf8b43.md:3915:docs/plan.md:66:5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
docs/perf/codex-reviews/fcf8b43.md:3916:docs/plan.md:100:- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
docs/perf/codex-reviews/fcf8b43.md:3917:docs/plan.md:271:**Scope.** Per spec doctrine D4 (single writer per fact) extended to account scope:
docs/perf/codex-reviews/fcf8b43.md:3918:docs/plan.md:335:- Profile picture update through compose → kind:0 republish with new Blossom URL → in-place refinement across all open Profile / Timeline payloads (per doctrine D1).
docs/perf/codex-reviews/fcf8b43.md:3919:docs/plan.md:356:  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
docs/perf/codex-reviews/fcf8b43.md:3920:docs/plan.md:380:- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
docs/perf/codex-reviews/fcf8b43.md:3921:docs/design/ffi-hardening.md:19:   (`docs/product-spec/overview-and-dx.md` §1.5 D0–D5), and every ownership
docs/perf/codex-reviews/fcf8b43.md:3922:docs/design/ffi-hardening.md:66:| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
docs/perf/codex-reviews/fcf8b43.md:3923:docs/design/ffi-hardening.md:135:| S1 | Mount/unmount churn | actor recv + refcount | D5 (snapshot bounded), bible #5 |
docs/perf/codex-reviews/fcf8b43.md:3924:docs/design/ffi-hardening.md:137:| S3 | Snapshot pressure | listener serialization | bible #9 (≤60 Hz), D5 |
docs/perf/codex-reviews/fcf8b43.md:3925:docs/design/ffi-hardening.md:138:| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
docs/perf/codex-reviews/fcf8b43.md:3926:docs/design/ffi-hardening.md:141:| S7 | Error-shape exhaustion | every invalid input path | D3 (no errors cross FFI) |
docs/perf/codex-reviews/fcf8b43.md:3927:docs/design/ffi-hardening.md:142:| S8 | Subscription planner DOS | OpenView/CloseView storm | D2 (≤60 Hz/view), D5 |
docs/perf/codex-reviews/fcf8b43.md:3928:docs/design/ffi-hardening.md:143:| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
docs/perf/codex-reviews/fcf8b43.md:3929:docs/design/ffi-hardening.md:197:├── doctrine-review.md       # D0–D5 sign-off (M10.5 exit-gate artifact)
docs/perf/codex-reviews/fcf8b43.md:3930:docs/design/ffi-hardening.md:217:| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
docs/perf/codex-reviews/fcf8b43.md:3931:docs/design/ffi-hardening.md:235:### 7.2 D3 structural gap (named, not hidden)
docs/perf/codex-reviews/fcf8b43.md:3932:docs/design/ffi-hardening.md:241:caller and without setting any state field. This is **D3-compliant in
docs/perf/codex-reviews/fcf8b43.md:3933:docs/design/ffi-hardening.md:243:D3-incomplete in the user-visible sense** (no toast surfaces in
docs/perf/codex-reviews/fcf8b43.md:3934:docs/design/ffi-hardening.md:246:The debt-inventory's D3 audit (lines 317–334) concludes the same:
docs/perf/codex-reviews/fcf8b43.md:3935:docs/design/ffi-hardening.md:263:Full D0–D5 line-item-to-scenario mapping in
docs/perf/codex-reviews/fcf8b43.md:3936:docs/design/ffi-hardening.md:268:| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
docs/perf/codex-reviews/fcf8b43.md:3937:docs/design/ffi-hardening.md:269:| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
docs/perf/codex-reviews/fcf8b43.md:3938:docs/design/ffi-hardening.md:270:| **D2** ≤60Hz/view, working-set bound | S2, S3, S8 — emit-rate cap, planner dedup |
docs/perf/codex-reviews/fcf8b43.md:3939:docs/design/ffi-hardening.md:271:| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
docs/perf/codex-reviews/fcf8b43.md:3940:docs/design/ffi-hardening.md:272:| **D4** one writer per fact | S1, S5 — refcount only mutated on actor thread; reentrancy under same single-writer rule |
docs/perf/codex-reviews/fcf8b43.md:3941:docs/design/ffi-hardening.md:273:| **D5** snapshots bounded by what's open | S1 (refcount drives eviction) + S3 (full-state size scales with open views, not store) |
docs/perf/codex-reviews/fcf8b43.md:3944:docs/perf/m10.5/debt-inventory.md:272:### D0 Audit: Kernel Never Grows App Nouns
docs/perf/codex-reviews/fcf8b43.md:3945:docs/perf/m10.5/debt-inventory.md:286:### D1 Audit: Best-Effort Rendering with Placeholders
docs/perf/codex-reviews/fcf8b43.md:3946:docs/perf/m10.5/debt-inventory.md:299:### D2 Audit: Reactivity Contract (Composite Reverse Index)
docs/perf/codex-reviews/fcf8b43.md:3947:docs/perf/m10.5/debt-inventory.md:313:### D3 Audit: Errors Never Cross FFI
docs/perf/codex-reviews/fcf8b43.md:3948:docs/perf/m10.5/debt-inventory.md:327:### D4 Audit: One Writer Per Fact
docs/perf/codex-reviews/fcf8b43.md:3949:docs/perf/m10.5/debt-inventory.md:341:### D5 Audit: Capabilities Report, Never Decide
docs/perf/codex-reviews/fcf8b43.md:3950:docs/perf/m10.5/debt-inventory.md:361:| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
docs/perf/codex-reviews/fcf8b43.md:3951:docs/perf/m10.5/debt-inventory.md:362:| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
docs/perf/codex-reviews/fcf8b43.md:3952:docs/perf/m10.5/debt-inventory.md:396:The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/fcf8b43.md:3953:docs/design/ffi-hardening/gates.md:7:2. **§D** doctrine D0–D5 review checklist — each line item maps to
docs/perf/codex-reviews/fcf8b43.md:3954:docs/design/ffi-hardening/gates.md:150:## §D. Doctrine D0–D5 review checklist
docs/perf/codex-reviews/fcf8b43.md:3955:docs/design/ffi-hardening/gates.md:155:> **Note.** The task brief mentioned "D0–D5". The canonical list in
docs/perf/codex-reviews/fcf8b43.md:3956:docs/design/ffi-hardening/gates.md:156:> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
docs/perf/codex-reviews/fcf8b43.md:3957:docs/design/ffi-hardening/gates.md:159:> not redundantly re-prove — items beyond D0–D5 are covered by
docs/perf/codex-reviews/fcf8b43.md:3958:docs/design/ffi-hardening/gates.md:162:### D0. Kernel never grows app nouns
docs/perf/codex-reviews/fcf8b43.md:3959:docs/design/ffi-hardening/gates.md:164:- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
docs/perf/codex-reviews/fcf8b43.md:3960:docs/design/ffi-hardening/gates.md:171:  `docs/perf/m10.5/doctrine-review.md` § D0.
docs/perf/codex-reviews/fcf8b43.md:3961:docs/design/ffi-hardening/gates.md:173:### D1. Best-effort rendering — render now, refine in place
docs/perf/codex-reviews/fcf8b43.md:3962:docs/design/ffi-hardening/gates.md:185:- 📝 **Sign-off:** doctrine-review.md § D1.
docs/perf/codex-reviews/fcf8b43.md:3963:docs/design/ffi-hardening/gates.md:187:### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
docs/perf/codex-reviews/fcf8b43.md:3964:docs/design/ffi-hardening/gates.md:197:- 📝 **Sign-off:** doctrine-review.md § D2.
docs/perf/codex-reviews/fcf8b43.md:3965:docs/design/ffi-hardening/gates.md:199:### D3. Errors never cross FFI
docs/perf/codex-reviews/fcf8b43.md:3966:docs/design/ffi-hardening/gates.md:201:- ⚠️ **Current state:** debt-inventory §3 D3 audit concludes
docs/perf/codex-reviews/fcf8b43.md:3967:docs/design/ffi-hardening/gates.md:210:- 📝 **Sign-off:** doctrine-review.md § D3, with the explicit
docs/perf/codex-reviews/fcf8b43.md:3968:docs/design/ffi-hardening/gates.md:211:  note that this milestone *closes* the D3-incomplete state
docs/perf/codex-reviews/fcf8b43.md:3969:docs/design/ffi-hardening/gates.md:214:### D4. Single writer per fact — caches derive
docs/perf/codex-reviews/fcf8b43.md:3970:docs/design/ffi-hardening/gates.md:216:- ✅ **Proof:** debt-inventory §3 D4 audit — single-threaded actor
docs/perf/codex-reviews/fcf8b43.md:3971:docs/design/ffi-hardening/gates.md:229:- 📝 **Sign-off:** doctrine-review.md § D4.
docs/perf/codex-reviews/fcf8b43.md:3972:docs/design/ffi-hardening/gates.md:231:### D5. Snapshots bounded by what's open
docs/perf/codex-reviews/fcf8b43.md:3973:docs/design/ffi-hardening/gates.md:241:- 📝 **Sign-off:** doctrine-review.md § D5.
docs/perf/codex-reviews/fcf8b43.md:3974:docs/design/ffi-hardening/gates.md:255:| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3975:docs/design/ffi-hardening/gates.md:256:| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3976:docs/design/ffi-hardening/gates.md:257:| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3977:docs/design/ffi-hardening/gates.md:258:| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3978:docs/design/ffi-hardening/gates.md:259:| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3979:docs/design/ffi-hardening/gates.md:260:| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
docs/perf/codex-reviews/fcf8b43.md:3980:docs/product-spec/appendices.md:11:**`AppState` is bounded by what's open.** It does not contain the event store, the gossip cache, the working set, or anything proportional to the local cache size. It contains:
docs/perf/codex-reviews/fcf8b43.md:3981:docs/product-spec/cli-toolchain-phasing.md:181:- **Best-effort rendering.** Doctrine D1: render what's available, refine in place; never withhold cached data; never block on fetches.
docs/perf/codex-reviews/fcf8b43.md:3982:docs/product-spec/overview-and-dx.md:31:### D0. Kernel + extension modules — no app nouns in `nmp-core`
docs/perf/codex-reviews/fcf8b43.md:3983:docs/product-spec/overview-and-dx.md:41:### D1. Best-effort rendering — render now, refine in place
docs/perf/codex-reviews/fcf8b43.md:3984:docs/product-spec/overview-and-dx.md:54:### D2. Negentropy first, REQ second
docs/perf/codex-reviews/fcf8b43.md:3985:docs/product-spec/overview-and-dx.md:60:### D3. Outbox routing is automatic; manual relay selection is the opt-out
docs/perf/codex-reviews/fcf8b43.md:3986:docs/product-spec/overview-and-dx.md:73:### D4. Single writer per fact; caches derive
docs/perf/codex-reviews/fcf8b43.md:3987:docs/product-spec/overview-and-dx.md:77:### D5. Snapshots bounded by what's open
docs/perf/codex-reviews/fcf8b43.md:3990:docs/product-spec/subsystems.md:74:Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
docs/perf/codex-reviews/fcf8b43.md:3991:docs/product-spec/subsystems.md:181:**Best-effort field contract (per doctrine D1).** Every display-bearing field in every view payload is **non-optional** and has a defined placeholder when the underlying data is missing:
docs/perf/codex-reviews/fcf8b43.md:3992:docs/product-spec/subsystems.md:242:Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.
docs/perf/codex-reviews/fcf8b43.md:4005:     9	> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
docs/perf/codex-reviews/fcf8b43.md:4054:    31	### D0. Kernel + extension modules — no app nouns in `nmp-core`
docs/perf/codex-reviews/fcf8b43.md:4064:    41	### D1. Best-effort rendering — render now, refine in place
docs/perf/codex-reviews/fcf8b43.md:4077:    54	### D2. Negentropy first, REQ second
docs/perf/codex-reviews/fcf8b43.md:4083:    60	### D3. Outbox routing is automatic; manual relay selection is the opt-out
docs/perf/codex-reviews/fcf8b43.md:4096:    73	### D4. Single writer per fact; caches derive
docs/perf/codex-reviews/fcf8b43.md:4098:    75	The "single source of truth" doctrine does not mean one cache — there are five layers (durable event store, in-memory working set, view payloads, gossip cache, platform reactive shadow). It means **one writer per fact**, and every downstream cache derives from the writer mechanically. Cache invalidation is not a concept in the public API. Recomputation happens in the actor; the platform receives new derived state.
docs/perf/codex-reviews/fcf8b43.md:4100:    77	### D5. Snapshots bounded by what's open
docs/perf/codex-reviews/fcf8b43.md:4113:   327	### D4 Audit: One Writer Per Fact
docs/perf/codex-reviews/fcf8b43.md:4127:   341	### D5 Audit: Capabilities Report, Never Decide
docs/perf/codex-reviews/fcf8b43.md:4137:   396	The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/fcf8b43.md:4145:   404	- Doctrine review signed in writing in `docs/perf/m10.5/doctrine-review.md`
docs/perf/codex-reviews/fcf8b43.md:4153:   231	### D5. Snapshots bounded by what's open
docs/perf/codex-reviews/fcf8b43.md:4163:   241	- 📝 **Sign-off:** doctrine-review.md § D5.
docs/perf/codex-reviews/fcf8b43.md:4263:    86	        --require-doctrines D0,D1,D2,D3,D4,D5 \
docs/perf/codex-reviews/fcf8b43.md:4340:   361	| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
docs/perf/codex-reviews/fcf8b43.md:4341:   362	| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
docs/perf/codex-reviews/fcf8b43.md:4362:   383	| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/fcf8b43.md:4375:   396	The FFI and iOS bridge code is **clean of blocking debt markers** (TODO/FIXME/unimplemented/panic). All 20 code inspection findings are either justified by design or actionable as documentation cleanup. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/fcf8b43.md:4383:   404	- Doctrine review signed in writing in `docs/perf/m10.5/doctrine-review.md`
docs/perf/codex-reviews/fcf8b43.md:4746:- [docs/design/ffi-hardening/gates.md:231](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:231) redefines D5 as “snapshots bounded by what’s open,” but [docs/plan.md:9](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:9) and [debt-inventory.md:341](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:341) define D5 as “capabilities report, never decide.” Fix the doctrine mapping before sign-off; add capability-bridge evidence instead of snapshot evidence, or explicitly reconcile the canonical doctrine docs.
docs/perf/codex-reviews/fcf8b43.md:4752:- [docs/design/ffi-hardening/ci.md:334](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:334) allows M10.5 close with S10 PASS-with-deferral, while [gates.md:256](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:256) uses S10 as D1 sign-off evidence. Fix: either make S10 required for M10.5 or remove it from doctrine evidence and replace with a non-conditional proof.
docs/perf/codex-reviews/fcf8b43.md:4759:No executable `TODO/FIXME/unimplemented` debt was added in source; hits are docs/audit text and the archived Codex transcript.
docs/perf/codex-reviews/fcf8b43.md:4765:- [docs/design/ffi-hardening/gates.md:231](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:231) redefines D5 as “snapshots bounded by what’s open,” but [docs/plan.md:9](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:9) and [debt-inventory.md:341](/Users/pablofernandez/Work/nostr-multi-platform/docs/perf/m10.5/debt-inventory.md:341) define D5 as “capabilities report, never decide.” Fix the doctrine mapping before sign-off; add capability-bridge evidence instead of snapshot evidence, or explicitly reconcile the canonical doctrine docs.
docs/perf/codex-reviews/fcf8b43.md:4771:- [docs/design/ffi-hardening/ci.md:334](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/ci.md:334) allows M10.5 close with S10 PASS-with-deferral, while [gates.md:256](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/ffi-hardening/gates.md:256) uses S10 as D1 sign-off evidence. Fix: either make S10 required for M10.5 or remove it from doctrine evidence and replace with a non-conditional proof.
docs/perf/codex-reviews/fcf8b43.md:4778:No executable `TODO/FIXME/unimplemented` debt was added in source; hits are docs/audit text and the archived Codex transcript.
docs/design/view-catalog/stubs-validation-next.md:56:- **Computed-from-native fields.** All formatting, all derivations live in Rust per doctrine D5.
docs/design/view-catalog/conversation-and-cross-cutting.md:95:- Decryption failure → don't add to view; record in `DebugDiagnostics`. Doctrine: never expose ciphertext as a message.
docs/design/view-catalog/profile-timeline-thread-reactions.md:103:- **Bare pubkey path.** If no kind:0 exists in the store yet, `open()` returns a placeholder-filled payload and the reverse-index dependency triggers `on_event_inserted` when one arrives. Doctrine D1 prohibits returning `None` here.
docs/design/view-catalog/profile-timeline-thread-reactions.md:234:- **Author kind:0 not yet loaded.** Per doctrine D1, render the item with placeholders. The `by_author` reindex on `on_projection_changed` updates the item in place. **The post is rendered immediately.**
docs/perf/reactivity-bench/1779046753-run-002.json:655:        "working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100",
docs/perf/reactivity-bench/1779046753-run-002.json:662:    "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.",
docs/design/reactivity/scheduling-and-data-model.md:83:│   Tier 2: Rust working set + projections                 │
docs/design/reactivity/scheduling-and-data-model.md:89:                       │ EventStore reads
docs/design/reactivity/scheduling-and-data-model.md:102:- **Tier 2 is bounded** (working-set policy, ADR-0003). The reverse index keys on attributes, not bodies, so it can cover unbounded Tier-1 events.
docs/design/reactivity/scheduling-and-data-model.md:104:- **Reads flow up; updates flow down.** Component reads happen entirely in Tier 3 — no FFI on the read path. Updates from relays land in Tier 1, propagate to Tier 2 working set + projections, then to Tier 3 via `ViewBatch`.
docs/design/reactivity/scheduling-and-data-model.md:109:The `EventStore` holds a **bounded hot working set** in memory; cold events live in the durable storage backend. The reverse index covers both.
docs/design/reactivity/scheduling-and-data-model.md:121:The working-set memory budget (≤ 100 MB at 100 active views, 10k hot events) is what reactivity-bench gates against. Total cached events on disk is unbounded.
docs/decisions/0001-composite-dependency-keys.md:34:- Index registration size grows by the product of axis sizes for a view. A timeline with 1k authors × 3 kinds inserts 3k composite entries (vs ~1k under the v0 model). Acceptable; far smaller than the working-set memory budget.
docs/decisions/0008-twitter-clone-on-ios-as-the-slice-target.md:66:The original ADR-0006 slice. Desktop iced binary; in-memory `EventStore` with kind:0 supersession + composite reverse index `(kind, author)`; minimal `Profile` view kind; manually-written `useProfile(pubkey)` wrapper for iced; one WebSocket via `nostr-sdk` to primal.
docs/decisions/0008-twitter-clone-on-ios-as-the-slice-target.md:87:- Storage backend abstraction: `Box<dyn EventStore>` swap from in-memory to LMDB. LMDB schema design (key encoding, secondary indexes, kind:5 tombstones, watermarks placeholder for 1b).
docs/decisions/0008-twitter-clone-on-ios-as-the-slice-target.md:94:**Exit gate.** Cold launch with primed LMDB renders the seed-driven timeline in ≤ 1.5s, showing fresh content from hundreds of authors followed by the seeds; tap an author → profile screen → back works; kind:0 arriving mid-scroll updates all author rows in place per doctrine D1; updating one seed's kind:3 mid-session re-resolves the timeline's author set without manual intervention; reactivity-bench `--standard` continues to pass at the larger author-set size; firehose-bench `live` for `sustained_firehose` (running at the real seed-author scale, not modeled) lands within budgets.
docs/design/reactivity/view-deltas-and-projections.md:20:pub fn open(spec: TimelineSpec, store: &EventStore) -> (State, Dependencies, TimelineView) {
docs/design/reactivity/view-deltas-and-projections.md:28:pub fn on_event_inserted(state: &mut State, event: &Event, store: &EventStore) -> Option<TimelineDelta> {
docs/design/reactivity/view-deltas-and-projections.md:38:pub fn on_event_replaced(state: &mut State, old_id: &EventId, new_event: &Event, store: &EventStore) -> Option<TimelineDelta> { ... }
docs/design/reactivity/view-deltas-and-projections.md:53:    fn on_event_inserted(&mut self, event: &Event, store: &EventStore) -> Option<ViewDelta> {
docs/design/podcast/inventory.md:65:| `Utilities/ErrorPresentation.swift` | 58 | swift | `ios/NmpPodcast/Bridge/ErrorPresentation.swift` | UI-only — `AppError`/`ErrorHandler` map `toast: Option<String>` from `AppState` (doctrine D3). No business logic. |
docs/design/podcast/podcast-core.md:240:17 `ViewModule`s. Each in `podcast-core/src/views/<name>.rs`. All share `View*` payload conventions: pre-formatted strings (per doctrine D1), no business logic in Swift, composite-keyed `ViewDependencies` (per ADR-0001).
docs/perf/reactivity-bench/2026-05-17-run-001.md:14:The reverse-index lookup and per-view recompute paths have 100×–1000× headroom against their gates. The current dependency model produces excessive false wakes. The delta-volume gate was set as an absolute and should have been per-view. The memory gate assumed all events resident and should have been working-set only.
docs/perf/reactivity-bench/2026-05-17-run-001.md:66:The 100 MB gate at 1M events assumed all events resident in memory. This is the anti-pattern the spec already calls out: the storage backend (LMDB / SQLite / IndexedDB) holds events; the actor keeps a bounded working set hot.
docs/perf/reactivity-bench/2026-05-17-run-001.md:68:**Refined gate: working-set memory ≤ 100 MB**, measured at 100 active views and ≤ 10k hot events. Total cached events on disk is unbounded (or capped by the storage backend's quota).
docs/perf/reactivity-bench/2026-05-17-run-001.md:135:### EventStore working-set discipline
docs/perf/reactivity-bench/2026-05-17-run-001.md:137:Add to `reactivity.md` §3 and `product-spec.md` §7.1: the EventStore holds a **bounded hot working set** in memory; cold events live in the storage backend. The reverse index indexes both. Lookups against cold events return their ids and attributes immediately; loading event bodies for delta construction happens lazily and synchronously via the backend.
docs/perf/reactivity-bench/2026-05-17-run-001.md:144:This makes the "1M events at 130 MB" finding a non-finding: total cached events is unbounded; working-set is bounded.
docs/perf/reactivity-bench/2026-05-17-run-001.md:157:| Memory | ≤ 100 MB at 100k events | **≤ 100 MB working set at 100 views, 10k hot events** |
docs/perf/reactivity-bench/2026-05-17-run-001.md:167:- **Storage-tier interaction with reverse-index updates.** When a cold event is loaded for delta construction, we don't need to add it to the working set unless it'll be re-referenced. Define the policy explicitly in the EventStore design.
docs/perf/reactivity-bench/2026-05-17-run-001.md:175:- ADR-0003: working-set memory budget (this run).
docs/decisions/0006-vertical-slice-first.md:13:The classic failure mode at this stage is **horizontal expansion** — building "the EventStore" comprehensively, then "the planner" comprehensively, then "the views" comprehensively, then finally stitching them together at the end, only to discover that the FFI surface or the relay adapter or the storage backend doesn't actually compose the way the model assumed.
docs/decisions/0006-vertical-slice-first.md:43:│  - on_event_inserted dispatched via composite reverse index  │
docs/decisions/0006-vertical-slice-first.md:48:│  EventStore (minimal)                                        │
docs/decisions/0006-vertical-slice-first.md:51:│  - composite reverse index keyed by (kind, author)           │
docs/decisions/0006-vertical-slice-first.md:94:- A real WebSocket → real EventStore → real DeltaBuffer → real component update is measurable end-to-end.
docs/decisions/0006-vertical-slice-first.md:112:- **Subsequent expansion has a working substrate to build on.** Adding LMDB is a `Box<dyn EventStore>` swap. Adding multi-relay is a planner change. Adding negentropy is a planner change. Adding iOS is a UniFFI wrap of the existing actor. None require redesigning the architecture.
docs/perf/codex-reviews/9fead0e.md:15:You are reviewing merge 9fead0e (M3 LMDB schema + EventStore trait + GC policy design) on master in nostr-multi-platform. Doctrine D0-D5. File size: 300 LOC soft, 500 hard.
docs/perf/codex-reviews/9fead0e.md:17:M3 goal (per docs/plan.md §M3): swap in-memory EventStore for LMDB; implement full insert invariants (replaceable kinds, kind:5 delete, NIP-40 expiration, dedup w/ provenance merge); claim-based GC; watermark table populated in M4.
docs/perf/codex-reviews/9fead0e.md:30:9fead0e design(m3): LMDB schema + EventStore trait + GC policy
docs/perf/codex-reviews/9fead0e.md:37:Decision: adopt nostr-lmdb as the on-disk byte store, wrap behind a
docs/perf/codex-reviews/9fead0e.md:38:NMP-owned EventStore trait, add NMP sub-databases for the rows
docs/perf/codex-reviews/9fead0e.md:39:nostr-lmdb does not model (claim-pinning, watermarks, projection
docs/perf/codex-reviews/9fead0e.md:52:+# Design: LMDB schema + EventStore trait + GC policy (M3)
docs/perf/codex-reviews/9fead0e.md:57:+> **Prerequisites:** `docs/product-spec/subsystems.md` §7.1 (insert invariants), `docs/decisions/0003-working-set-memory.md` (GC policy intent), `docs/decisions/0009-app-extension-kernel-boundary.md` (DomainModule storage), `docs/design/kernel-substrate.md` §2 (DomainModule trait).
docs/perf/codex-reviews/9fead0e.md:62:+## 1. Decision: wrap `nostr-lmdb` behind our own `EventStore` trait
docs/perf/codex-reviews/9fead0e.md:64:+**Adopt `nostr-lmdb` as the on-disk byte store. Wrap it behind the NMP `EventStore` trait. Add NMP-owned LMDB sub-databases for the rows `nostr-lmdb` does not model.**
docs/perf/codex-reviews/9fead0e.md:66:+The competing options were (1) use `nostr-lmdb` directly via its concrete `NostrLMDB` type (or via `nostr_database::NostrEventsDatabase`), (2) wrap behind our own trait, or (3) hand-roll an LMDB layer.
docs/perf/codex-reviews/9fead0e.md:68:+**`nostr-lmdb` gives us** (per `docs.rs/nostr-lmdb`): `save_event(&Event)`, `event_by_id(&EventId)`, `check_id(&EventId) -> DatabaseEventStatus`, `query(Filter) -> Events`, `count(Filter)`, `delete(Filter)`, `wipe()`, `negentropy_items(Filter) -> Vec<(EventId, Timestamp)>`. It owns the page allocator, the LMDB environment, primary by-id store, indexes derived from `Filter`, replaceable / parameterized-replaceable supersession, and NIP-09 delete handling. It is the only mature Rust LMDB store for Nostr events with proven NIP-77 integration; reinventing it is a year of work that we will not recoup.
docs/perf/codex-reviews/9fead0e.md:70:+**What `nostr-lmdb` does *not* model** (the gap that justifies a wrapper):
docs/perf/codex-reviews/9fead0e.md:72:+| Concern | Why `nostr-lmdb` doesn't cover it | Where NMP needs it |
docs/perf/codex-reviews/9fead0e.md:84:+**Therefore.** `EventStore` is a NMP-owned trait, with one production impl `LmdbEventStore` that holds (a) a `NostrLMDB` for the canonical event store and Nostr-shaped queries, and (b) NMP-owned secondary LMDB sub-databases under the same `lmdb::Environment` for the gap rows. The in-memory backend (`MemEventStore`) remains, both for tests and as the web-pre-M15 fallback. See [`lmdb/trait.md`](lmdb/trait.md) for the exact trait shape and the relayed-vs-owned method split.
docs/perf/codex-reviews/9fead0e.md:88:+- *Use `NostrLMDB` directly, no wrapper.* Loses every gap row above. Forces the kernel actor to know about LMDB transactions and a non-NMP concrete type, breaking the `Box<dyn EventStore>` substitutability M3 requires.
docs/perf/codex-reviews/9fead0e.md:89:+- *Roll our own.* Reinvents NIP-09 / replaceable handling that `nostr-lmdb` already gets right. ~2,000 LOC of avoidable code with a worse bug surface than upstream.
docs/perf/codex-reviews/9fead0e.md:90:+- *SQLite-backed `nostr-sdk` store.* Larger memory footprint at our 10k-event hot working set; iOS-disk-WAL fsync cost not justified for this access pattern. Held in reserve for the web port (M15) if IndexedDB OPFS proves unworkable.
docs/perf/codex-reviews/9fead0e.md:97:+  trait.rs              — `EventStore` (see lmdb/trait.md)
docs/perf/codex-reviews/9fead0e.md:100:+    mod.rs              — `LmdbEventStore` orchestrator
docs/perf/codex-reviews/9fead0e.md:114:+## 3. EventStore trait
docs/perf/codex-reviews/9fead0e.md:116:+See [`lmdb/trait.md`](lmdb/trait.md) for the exact `pub trait EventStore` signature with all required methods, return types, and the `StoreError` enum. Summary:
docs/perf/codex-reviews/9fead0e.md:126:+**Error semantics.** All methods return `Result<T, StoreError>`. Per doctrine D3, store errors do **not** cross FFI — the actor maps every variant to either (a) a typed `Effect` (e.g. `StoreCorrupt`, surfaces via diagnostics + toast), (b) a `tracing::warn!` log + degraded state, or (c) a panic at startup if the LMDB environment refuses to open. The trait itself uses `Result` since it is internal to the actor process.
docs/perf/codex-reviews/9fead0e.md:132:+- Primary `events`: `event_id[32]` → `Event` (CBOR via `nostr` crate's serialization). Owned by `nostr-lmdb`.
docs/perf/codex-reviews/9fead0e.md:160:+Key: `filter_hash[32] || relay_url_bytes` (no length prefix needed — relay URL is the variable suffix; lookup uses exact key). Populated by M4 (NIP-77) and consulted by M2's planner (cache-coverage check before issuing backfill REQ). Survives restarts; loaded into the actor on startup as a `HashMap<(filter_hash, relay_url), WatermarkRow>` for hot lookups, with all writes going through `EventStore` for durability.
docs/perf/codex-reviews/9fead0e.md:177:+## 7. GC working-set policy
docs/perf/codex-reviews/9fead0e.md:193:+ADR-0003's numbers are preserved as the M3 exit gate (§11 below): ≤ 100 MB working-set at 100 active views / 10k hot events / 1M cached on disk.
docs/perf/codex-reviews/9fead0e.md:211:+Rejected: stuffing provenance into the `Event` row. That requires re-serializing the full `Event` on every relay redelivery (high write amplification — popular events arrive 5–20× from the relay fan-out) and forks the `nostr-lmdb` row format, which we explicitly want to keep upstream-compatible. The sidecar is appended cheaply with a single CBOR re-encode of the (typically small) `sources` vector.
docs/perf/codex-reviews/9fead0e.md:259:+| Working-set memory at 100 active views / 10k hot / 1M on disk | ≤ 100 MB resident | Instruments Allocations + `reactivity-bench` working-set scenario |
docs/perf/codex-reviews/9fead0e.md:269:+1. **`nostr-lmdb` LMDB environment sharing.** Can we open the same `lmdb::Environment` for both `NostrLMDB`'s sub-databases and our own NMP sub-databases (provenance, watermarks, claims, domain rows)? If yes, we get atomic cross-sub-db transactions for free (a single `RwTxn` covers event + provenance + secondary indexes). If `nostr-lmdb` insists on opening its own `Environment`, we lose that and the insert path needs a two-phase write with crash-recovery logic. Investigate before implementation — may require an upstream PR exposing `Environment` access.
docs/perf/codex-reviews/9fead0e.md:274:+6. **`ModuleRegistry::register_domain` API stability.** Adding `DomainFactories` to `ModuleDescriptor` is a non-breaking additive change to the public substrate API (existing callers using only the generic `register_domain::<M>()` continue to compile), but it commits us to keeping `DomainModule::SCHEMA_VERSION` and `DomainModule::migrations` as compile-time-resolvable items rather than object-safe methods. Confirm this with the substrate maintainer before M3 lands — if `DomainModule` is expected to support runtime composition (e.g., plugin loading), we need option (c): the actor passes the live `&[Box<dyn DomainModule>]` to `EventStore::open` instead. Recommended default: stay with `fn`-pointer factories; revisit if a plugin-loading use case appears.
docs/perf/codex-reviews/9fead0e.md:280:+- Replaceable supersession (current scattered logic to be centralised in `EventStore::insert`): `kernel/ingest.rs:166-185` (profile replace by `(pubkey, kind)`), `ingest.rs:218-233` (NIP-65 list replace by `(pubkey, 10002)`).
docs/perf/codex-reviews/9fead0e.md:281:+- Profile claim refcounting (current in-memory analogue of `EventStore::claim/release`): `kernel/mod.rs:315` (`profile_claims: HashMap<String, BTreeSet<String>>`), `kernel/requests.rs:202` (`claim_profile`), `requests.rs:239` (`release_profile`).
docs/perf/codex-reviews/9fead0e.md:285:+The M3 implementation deletes none of the existing files outright — the kernel's `events: HashMap` and `profiles: HashMap` are replaced by calls to `Box<dyn EventStore>` held inside the `Kernel` struct, and the per-field tests (`kernel/tests.rs`) shift to the new trait via `MemEventStore`. No public FFI surface changes.
docs/perf/codex-reviews/9fead0e.md:292:+# LMDB sub-design: GC working-set policy
docs/perf/codex-reviews/9fead0e.md:294:+> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Formalises the hot-resident / cold-on-disk split required by ADR-0003 (`docs/decisions/0003-working-set-memory.md`).
docs/perf/codex-reviews/9fead0e.md:306:+                  computed from the composite reverse-index per ADR-0001
docs/perf/codex-reviews/9fead0e.md:421:+2. The composite reverse-index resolves the dependency set to a (small, bounded) set of currently-known event ids — the *view cover*.
docs/perf/codex-reviews/9fead0e.md:434:+The relevant figure for the M3 exit gate is **working-set RSS at the configuration described in ADR-0003 §Decision**: 100 active views, 10k hot events, 1M cached on disk, ≤ 100 MB.
docs/perf/codex-reviews/9fead0e.md:444:+| LMDB page cache (kernel-owned, *not* counted toward RSS budget) | 0 | OS-paged, evicted under pressure; counts against system memory but not app working set |
docs/perf/codex-reviews/9fead0e.md:495:+> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Fixes the byte layout of every sub-database the NMP store opens. Primary event storage is owned by `nostr-lmdb`; everything below is NMP-owned and lives in NMP-managed sub-databases under the same `lmdb::Environment` (per open question 1 in the master doc).
docs/perf/codex-reviews/9fead0e.md:503:+| (multiple) | `nostr-lmdb` | upstream | upstream | event primary, internal filter indexes, kind:5 suppression |
docs/perf/codex-reviews/9fead0e.md:518:+Sub-databases are opened lazily on first access and cached on the `LmdbEventStore`.
docs/perf/codex-reviews/9fead0e.md:546:+The value is the `event_id`; the primary event itself lives in the `nostr-lmdb` events sub-db. On supersession, the old event-id is fetched from this row, both primary and old `idx_*` rows are deleted, and the value is overwritten with the new id.
docs/perf/codex-reviews/9fead0e.md:651:+> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Maps every insert invariant in `docs/product-spec/subsystems.md` §7.1 to a concrete test in `crates/nmp-testing/tests/`. Each test exists for both `MemEventStore` (always) and `LmdbEventStore` (under `#[cfg(feature = "lmdb-backend")]`).
docs/perf/codex-reviews/9fead0e.md:658:+    pub store: Box<dyn EventStore>,
docs/perf/codex-reviews/9fead0e.md:664:+    pub fn mem() -> Self { /* MemEventStore */ }
docs/perf/codex-reviews/9fead0e.md:665:+    pub fn lmdb() -> Self { /* LmdbEventStore in tmp dir */ }
docs/perf/codex-reviews/9fead0e.md:704:+Plus a static-assertion-style test ensuring no other public function on `EventStore` writes to the primary store (compile-time check by inspecting trait method list via a build script — deferred to v1.x; v1 covers via review).
docs/perf/codex-reviews/9fead0e.md:878:+# LMDB sub-design: `EventStore` trait
docs/perf/codex-reviews/9fead0e.md:884:+`crates/nmp-core/src/store/events.rs` (filename note: `trait` is a Rust keyword, so the file is named `events.rs` and exposes `pub trait EventStore`). Re-exported from `nmp_core::store::EventStore`. The actor (`crates/nmp-core/src/actor.rs`) holds the store as `store: Box<dyn EventStore>`; backends are constructed by the factory in `store/mod.rs::open_event_store(&AppConfig) -> Result<Box<dyn EventStore>, StoreError>`.
docs/perf/codex-reviews/9fead0e.md:1021:+pub trait EventStore: Send + Sync {
docs/perf/codex-reviews/9fead0e.md:1159:+## 5. Error semantics (doctrine D3)
docs/perf/codex-reviews/9fead0e.md:1165:+- `Encoding` → `tracing::error!` with the offending key/namespace; the action that triggered it fails with a `toast: Some("internal storage error; please restart")` per D3.
docs/perf/codex-reviews/9fead0e.md:1176:+pub struct MemEventStore { /* HashMap-backed; preserves the current kernel state */ }
docs/perf/codex-reviews/9fead0e.md:1179:+pub struct LmdbEventStore { /* wraps nostr_lmdb::NostrLMDB + NMP sub-dbs */ }
docs/perf/codex-reviews/9fead0e.md:1181:+pub fn open_event_store(cfg: &AppConfig) -> Result<Box<dyn EventStore>, StoreError> {
docs/perf/codex-reviews/9fead0e.md:1183:+        StorageBackend::Memory => Ok(Box::new(MemEventStore::new())),
docs/perf/codex-reviews/9fead0e.md:1184:+        StorageBackend::Lmdb { ref path } => Ok(Box::new(LmdbEventStore::open(path)?)),
docs/perf/codex-reviews/9fead0e.md:1189:+`MemEventStore` implements every method using `HashMap` / `BTreeMap`. The same test suite runs against both backends with `#[cfg(feature = "lmdb-backend")]` gating only the LMDB-specific edge tests (corruption recovery, oversized values).
docs/perf/codex-reviews/9fead0e.md:1242:+On `LmdbEventStore::open()`, the store reads all `watermarks` rows and builds an in-memory `HashMap<WatermarkKey, WatermarkRow>` for hot lookups. Every `write_watermark` updates both the in-memory map and the LMDB row in a single `RwTxn`. Restart re-derives the map; we don't need a separate cache file.
docs/perf/codex-reviews/9fead0e.md:1388:Review: (1) doctrine compliance (in particular D0 + D2 — composite reverse index + working-set bound must remain enforced through the trait); (2) any TODO/FIXME/unimplemented; (3) file-size compliance; (4) consistency of EventStore trait surface across the 6 sub-docs; (5) the choice of nostr-lmdb as on-disk byte store vs roll-our-own — is the gap analysis honest?; (6) the GC eviction algorithm + budget; (7) the migration plumbing's testability; (8) any hidden shortcuts. Be terse; name file:line + fix for any concern.
docs/design/podcast/capabilities.md:4:> Substrate reference: [`../kernel-substrate.md`](../kernel-substrate.md) §5; doctrine: D5 (capabilities report, never decide).
docs/design/podcast/capabilities.md:85:### Bounded-state proof (D5)
docs/design/reactivity/validation-harness.md:11:- **Mutation of `EventStore` from within a view.** Views observe; they don't write. Only the actor's top-level handlers and actions write.
docs/design/reactivity/validation-harness.md:39:- Spawns a configurable `EventStore` (in-memory, LMDB, or SQLite backend).
docs/product-spec/cli-toolchain-phasing.md:174:- **EventStore.** The reactive single source of truth for all Nostr events. Owned by the actor; not exposed at FFI.
docs/product-spec/cli-toolchain-phasing.md:177:- **View.** A pre-built derived projection of `EventStore` contents. Opened by `OpenView` action; payload arrives via `AppState.views` / `ViewBatch`.
docs/product-spec/cli-toolchain-phasing.md:181:- **Best-effort rendering.** Doctrine D1: render what's available, refine in place; never withhold cached data; never block on fetches.
docs/decisions/0003-working-set-memory.md:1:# ADR 0003: Memory budget is for working set, not total cached events
docs/decisions/0003-working-set-memory.md:11:The actor should keep a **bounded working set** of hot events in memory; cold events live on disk. The reverse index can cover both — it keys on attributes, not event bodies.
docs/decisions/0003-working-set-memory.md:15:The memory budget targets **working-set memory at typical active load**, not total cached events.
docs/decisions/0003-working-set-memory.md:34:- The 1M-events-resident scenario is no longer a failure — it's an unintended test of an unintended configuration. Re-run with bounded working set.
docs/decisions/0003-working-set-memory.md:46:Re-run reactivity-bench with bounded working set; require ≤ 100 MB at 100 views / 10k hot events / 1M cached events on disk.
docs/perf/codex-reviews/0dfb975.md:15:You are reviewing merge 0dfb97581315aa04bd66341a98a32215de5e14d6 (the M11 podcast-app rebuild design) on master in nostr-multi-platform. Doctrine D0-D5 (D0 kernel never grows app nouns, D1 best-effort rendering, D2 reactivity ≤60Hz/view, D3 no errors across FFI, D4 one writer per fact, D5 capabilities report don't decide). File size: 300 LOC soft, 500 hard.
docs/perf/codex-reviews/0dfb975.md:244:+> Substrate reference: [`../kernel-substrate.md`](../kernel-substrate.md) §5; doctrine: D5 (capabilities report, never decide).
docs/perf/codex-reviews/0dfb975.md:325:+### Bounded-state proof (D5)
docs/perf/codex-reviews/0dfb975.md:675:+- The placeholder block is fenced by `// MARK: NMP-WIRE` (start) and either `// MARK: NMP-WIRE — wired` (after wiring) or `// MARK: NMP-WIRE — TODO` (still pending).
docs/perf/codex-reviews/0dfb975.md:677:+- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/` is part of the M11 exit gate (must be zero).
docs/perf/codex-reviews/0dfb975.md:686:+// MARK: NMP-WIRE — TODO
docs/perf/codex-reviews/0dfb975.md:795:+- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` is the work-remaining counter for Step 7.
docs/perf/codex-reviews/0dfb975.md:895:+Every test file ≤ 500 LOC. The cross-cutting kill-relaunch test is the most load-bearing — it asserts D2 (reactivity) and D4 (single writer per fact) hold under app termination at every state-transition boundary.
docs/perf/codex-reviews/0dfb975.md:916:+- The doctrine review at `docs/perf/m11/doctrine-review.md` signs off D0–D5 against the M11 surface (template: `docs/perf/m10.5/doctrine-review.md`).
docs/perf/codex-reviews/0dfb975.md:993:+| `Utilities/ErrorPresentation.swift` | 58 | swift | `ios/NmpPodcast/Bridge/ErrorPresentation.swift` | UI-only — `AppError`/`ErrorHandler` map `toast: Option<String>` from `AppState` (doctrine D3). No business logic. |
docs/perf/codex-reviews/0dfb975.md:1182:+- "No business logic in native." (Doctrine D0 + AGENTS.md guardrails.)
docs/perf/codex-reviews/0dfb975.md:1441:+17 `ViewModule`s. Each in `podcast-core/src/views/<name>.rs`. All share `View*` payload conventions: pre-formatted strings (per doctrine D1), no business logic in Swift, composite-keyed `ViewDependencies` (per ADR-0001).
docs/perf/codex-reviews/0dfb975.md:1767:+All actions return parsed records. None of them write to the domain store directly — `podcast-core::SubscribePodcast`/`RefreshFeed` orchestrate the writes, preserving the doctrine: **one writer per fact** (D4).
docs/perf/codex-reviews/0dfb975.md:2087:+- Failure-mode test: capability emits `Error { reason: "rate_limit" }` mid-stream → `ChatTurn` commits with `is_error: true` and the `toast` field is set per doctrine D3.
docs/perf/codex-reviews/0dfb975.md:2753:+3. Replace `// MARK: NMP-WIRE — TODO` blocks with `// MARK: NMP-WIRE — wired` consuming the generated `@<View>` property wrapper.
docs/perf/codex-reviews/0dfb975.md:2830:+- Acceptance: subscribe to a real RSS feed (Tim Ferriss) → library shows it → tap → see ≥ 5 episodes parsed; UI updates as `FetchFeed` action completes (no spinner gate; placeholder rows during fetch per doctrine D1).
docs/perf/codex-reviews/0dfb975.md:2943:+`grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` ratchets to zero across the 7 lanes.
docs/perf/codex-reviews/0dfb975.md:2954:+- [ ] All `// MARK: NMP-WIRE — TODO` in the lane removed
docs/perf/codex-reviews/0dfb975.md:2973:1. D0 enforcement — does anything in the design leak podcast nouns into nmp-core? Are the new capabilities (Audio, Background, Embedding, KV) general-shape or accidentally podcast-shaped?
docs/perf/codex-reviews/0dfb975.md:2974:2. D5 — do the capability traits report only, or do any of them embed policy?
docs/perf/codex-reviews/0dfb975.md:2979:7. Any TODO/FIXME/unimplemented sneaking in?
docs/perf/codex-reviews/0dfb975.md:2986:/bin/zsh -lc 'rg -n "nostr-multi-platform|M11|podcast-app|D0|kernel never" /Users/pablofernandez/.codex/memories/MEMORY.md' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/0dfb975.md:3037:docs/plan.md:20:- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
docs/perf/codex-reviews/0dfb975.md:3041:docs/plan.md:66:5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
docs/perf/codex-reviews/0dfb975.md:3055:docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/0dfb975.md:3169:docs/decisions/0008-twitter-clone-on-ios-as-the-slice-target.md:66:The original ADR-0006 slice. Desktop iced binary; in-memory `EventStore` with kind:0 supersession + composite reverse index `(kind, author)`; minimal `Profile` view kind; manually-written `useProfile(pubkey)` wrapper for iced; one WebSocket via `nostr-sdk` to primal.
docs/perf/codex-reviews/0dfb975.md:3178:/bin/zsh -lc 'rg -n "NO podcast nouns|grep -RE|AudioPlayback|AppleIntelligence|KeyValueStore|Authoritative settings|ImportOpml|QueueEntry|disabled by default|ImageCache|ClearImageCache|App/PodcastApp|Swift files staying|Swift files moving|18 view files|47 Swift files|8,793|24 files|Generated property wrappers|@propertyWrapper|UseFoo|useFoo|rig-core|gpt-4o-mini|SpeechAnalyzer|iOS 26|FoundationModels|sqlite-vec|load_extension|Prompt|byte-identical|Reference Swift app|commit at task time|Source:|mcp__xcode|macOS-15|iOS 26.5|Line|TODO|FIXME|unimplemented" docs/design/podcast-app-rebuild.md docs/design/podcast/*.md' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/0dfb975.md:3252:docs/design/podcast/copy.md:81:- The placeholder block is fenced by `// MARK: NMP-WIRE` (start) and either `// MARK: NMP-WIRE — wired` (after wiring) or `// MARK: NMP-WIRE — TODO` (still pending).
docs/perf/codex-reviews/0dfb975.md:3254:docs/design/podcast/copy.md:83:- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/` is part of the M11 exit gate (must be zero).
docs/perf/codex-reviews/0dfb975.md:3255:docs/design/podcast/copy.md:92:// MARK: NMP-WIRE — TODO
docs/perf/codex-reviews/0dfb975.md:3258:docs/design/podcast/copy.md:201:- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` is the work-remaining counter for Step 7.
docs/perf/codex-reviews/0dfb975.md:3261:docs/design/podcast/wiring.md:11:3. Replace `// MARK: NMP-WIRE — TODO` blocks with `// MARK: NMP-WIRE — wired` consuming the generated `@<View>` property wrapper.
docs/perf/codex-reviews/0dfb975.md:3276:docs/design/podcast/wiring.md:201:`grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` ratchets to zero across the 7 lanes.
docs/perf/codex-reviews/0dfb975.md:3277:docs/design/podcast/wiring.md:212:- [ ] All `// MARK: NMP-WIRE — TODO` in the lane removed
docs/perf/codex-reviews/0dfb975.md:3493:A few issues are now concrete rather than stylistic: the design has at least one hard LOC conflict with the copy-verbatim requirement, and the screenshot gate currently depends on an agent-only UI introspection path. I’m pulling the exact lines for those plus the doctrine/D4 ownership concerns.
docs/perf/codex-reviews/0dfb975.md:3720:    85	### Bounded-state proof (D5)
docs/perf/codex-reviews/0dfb975.md:3938:   201	- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` is the work-remaining counter for Step 7.
docs/perf/codex-reviews/0dfb975.md:3978:3. [capabilities.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/capabilities.md:79): `AudioPlaybackBridge` decides repeated `Load` is a no-op based on `currentEpisode`. That is playback policy in native, and it conflicts with the same doc’s D5 claim at [capabilities.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/capabilities.md:87) plus the lesson at [lessons.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/lessons.md:58). Fix: Rust decides whether to send `Load`; the bridge only executes requests and reports state.
docs/perf/codex-reviews/0dfb975.md:3980:4. [podcast-core.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/podcast-core.md:130), [podcast-core.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/podcast-core.md:231), and [podcast-rag.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/podcast-rag.md:132): `podcast-core` stores `embedding_id`, says it is re-exported from `podcast-rag`, while `podcast-rag` depends on `podcast-core` and backfills the core record. That creates either a crate cycle or a D4 one-writer violation. Fix: define `EmbeddingId` in `podcast-core` or a shared types crate, and have core own any writes to core records after `podcast-rag` returns `Indexed`.
docs/perf/codex-reviews/0dfb975.md:3997:3. [capabilities.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/capabilities.md:79): `AudioPlaybackBridge` decides repeated `Load` is a no-op based on `currentEpisode`. That is playback policy in native, and it conflicts with the same doc’s D5 claim at [capabilities.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/capabilities.md:87) plus the lesson at [lessons.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/lessons.md:58). Fix: Rust decides whether to send `Load`; the bridge only executes requests and reports state.
docs/perf/codex-reviews/0dfb975.md:3999:4. [podcast-core.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/podcast-core.md:130), [podcast-core.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/podcast-core.md:231), and [podcast-rag.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/podcast/podcast-rag.md:132): `podcast-core` stores `embedding_id`, says it is re-exported from `podcast-rag`, while `podcast-rag` depends on `podcast-core` and backfills the core record. That creates either a crate cycle or a D4 one-writer violation. Fix: define `EmbeddingId` in `podcast-core` or a shared types crate, and have core own any writes to core records after `podcast-rag` returns `Indexed`.
docs/design/podcast/podcast-llm.md:272:- Failure-mode test: capability emits `Error { reason: "rate_limit" }` mid-stream → `ChatTurn` commits with `is_error: true` and the `toast` field is set per doctrine D3.
docs/perf/codex-reviews/03d7a23.md:15:You are reviewing merge 03d7a23710136279b730ec0227e49575e3658695 on master. NMP nostr-multi-platform. Doctrine D0-D5, file size 300 LOC soft/500 hard. This merge adds SAFETY comments to ffi.rs unsafe blocks and a clarifying comment in relay_worker.rs. Verify:
docs/perf/codex-reviews/03d7a23.md:18:3. No new TODO/FIXME introduced.
docs/perf/codex-reviews/03d7a23.md:464:/bin/zsh -lc 'rg -n "TODO|FIXME" crates/nmp-core/src/ffi.rs crates/nmp-core/src/relay_worker.rs' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a41054fd04781ce06
docs/perf/codex-reviews/03d7a23.md:467:The committed diff is comments only, and the TODO/FIXME scan of the two touched files is clean. I’m checking the workspace commands now so the review covers the requested build surface too.
docs/perf/codex-reviews/03d7a23.md:1139:/bin/zsh -lc 'git show --no-ext-diff --format= --no-renames 03d7a23710136279b730ec0227e49575e3658695 | rg -n "TODO|FIXME"' in /Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a41054fd04781ce06
docs/perf/codex-reviews/03d7a23.md:1300:- No new TODO/FIXME.
docs/perf/codex-reviews/03d7a23.md:1313:- No new TODO/FIXME.
docs/design/podcast/podcast-feeds.md:122:All actions return parsed records. None of them write to the domain store directly — `podcast-core::SubscribePodcast`/`RefreshFeed` orchestrate the writes, preserving the doctrine: **one writer per fact** (D4).
docs/perf/reactivity-bench/1779049354-run-002.md:44:- working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100
docs/perf/reactivity-bench/1779049354-run-002.md:49:- Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.
docs/perf/reactivity-bench/1779046753-run-002.md:44:- working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100
docs/perf/reactivity-bench/1779046753-run-002.md:49:- Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.
docs/design/podcast/copy.md:81:- The placeholder block is fenced by `// MARK: NMP-WIRE` (start) and either `// MARK: NMP-WIRE — wired` (after wiring) or `// MARK: NMP-WIRE — TODO` (still pending).
docs/design/podcast/copy.md:83:- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/` is part of the M11 exit gate (must be zero).
docs/design/podcast/copy.md:92:// MARK: NMP-WIRE — TODO
docs/design/podcast/copy.md:201:- `grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` is the work-remaining counter for Step 7.
docs/design/reactivity/loop-and-reverse-index.md:9:> **Status:** rev 1 — incorporating findings from reactivity-bench run 001 (2026-05-17). See `docs/perf/reactivity-bench/2026-05-17-run-001.md` for the measurement report. Decisions: ADR-0001 (composite keys), ADR-0002 (per-view delta budget), ADR-0003 (working-set memory), ADR-0004 (allocation measurement).
docs/design/reactivity/loop-and-reverse-index.md:38:                │  EventStore::insert(event)               │
docs/design/reactivity/loop-and-reverse-index.md:67:- **`EventStore`** owns the actual events and the reverse index. Inserting goes through it.
docs/design/reactivity/loop-and-reverse-index.md:133:**Why composite-first:** reactivity-bench run 001 measured 98% false-wakeup rate in quiet_idle and 49% in following_timeline_scroll under the v0 design (which unioned independent axis buckets). Conjunctive composite keys eliminate the false wakes. The cost is registration-size growth (kinds × authors cartesian product), bounded by working-set memory budget.
docs/perf/reactivity-bench/1779050935-run-002.json:655:        "working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100",
docs/perf/reactivity-bench/1779050935-run-002.json:662:    "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.",
docs/product-spec/subsystems.md:7:### 7.1 EventStore
docs/product-spec/subsystems.md:74:Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
docs/product-spec/subsystems.md:181:**Best-effort field contract (per doctrine D1).** Every display-bearing field in every view payload is **non-optional** and has a defined placeholder when the underlying data is missing:
docs/product-spec/subsystems.md:242:Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.
docs/product-spec/subsystems.md:247:View opens → Live REQ handler starts → Planner consults coverage → Sync engine reconciles gaps → EventStore inserts → ViewBatch emits
docs/product-spec/subsystems.md:419:- Memory footprint of the actor's working set.
docs/perf/reactivity-bench/1779045194-run-002.md:44:- working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100
docs/perf/reactivity-bench/1779045194-run-002.md:49:- Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.
docs/perf/reactivity-bench/1779051783-run-002.md:44:- working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100
docs/perf/reactivity-bench/1779051783-run-002.md:49:- Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.
docs/perf/codex-reviews/2026-05-18-session-1.md:15:You are reviewing a session's worth of merges on master in the nostr-multi-platform repo. NMP is a Rust multiplatform framework for Nostr apps building toward v1 per docs/plan.md. Doctrine D0–D5 (docs/product-spec.md §1.5):
docs/perf/codex-reviews/2026-05-18-session-1.md:16:- D0 kernel never grows app nouns
docs/perf/codex-reviews/2026-05-18-session-1.md:17:- D1 best-effort rendering with placeholders
docs/perf/codex-reviews/2026-05-18-session-1.md:18:- D2 reactivity contract (composite reverse index, ≤60 Hz/view, working-set bound)
docs/perf/codex-reviews/2026-05-18-session-1.md:19:- D3 errors never cross FFI (become toast state fields)
docs/perf/codex-reviews/2026-05-18-session-1.md:20:- D4 one writer per fact
docs/perf/codex-reviews/2026-05-18-session-1.md:21:- D5 capabilities report, never decide
docs/perf/codex-reviews/2026-05-18-session-1.md:43:- 0 critical debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!)
docs/perf/codex-reviews/2026-05-18-session-1.md:45:- All 5 cardinal doctrines (D0–D5) compliant
docs/perf/codex-reviews/2026-05-18-session-1.md:54:Doctrine compliance (exit gates for M10.5):
docs/perf/codex-reviews/2026-05-18-session-1.md:55:✅ D0 (kernel never grows app nouns): kernel is domain-agnostic
docs/perf/codex-reviews/2026-05-18-session-1.md:56:✅ D1 (best-effort rendering): ProfileCard.placeholder renders immediately
docs/perf/codex-reviews/2026-05-18-session-1.md:57:✅ D2 (reactivity contract): all updates flow through composite reverse index
docs/perf/codex-reviews/2026-05-18-session-1.md:58:✅ D3 (errors never cross FFI): errors as advisory JSON data, not FFI codes
docs/perf/codex-reviews/2026-05-18-session-1.md:59:✅ D4 (one writer per fact): kernel actor is single-threaded
docs/perf/codex-reviews/2026-05-18-session-1.md:60:✅ D5 (capabilities report): iOS bridge is pure relay, no policy decisions
docs/perf/codex-reviews/2026-05-18-session-1.md:86:- Zero open TODO/FIXME/unimplemented in FFI/actor/relay/kernel/iOS bridge
docs/perf/codex-reviews/2026-05-18-session-1.md:125:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/2026-05-18-session-1.md:282:+**Issue:** Casts a `usize` (which can be >u32 on 64-bit systems) to `u32` after explicit saturation at `u32::MAX`. This is intentional: if there are more than 2^32 profile claims (which will not occur in practice given working-set bounds), cap the refcount at u32::MAX. No silent overflow.
docs/perf/codex-reviews/2026-05-18-session-1.md:308:+**Issue:** Casting collection `.count()` and arithmetic results to metric types. No overflow risk because counts are bounded by working set size constraints (max 5,000 stored events per ADR-0001; max visible 500 per FFI clamp).
docs/perf/codex-reviews/2026-05-18-session-1.md:383:+## 3. Doctrine Violations
docs/perf/codex-reviews/2026-05-18-session-1.md:385:+### D0 Audit: Kernel Never Grows App Nouns
docs/perf/codex-reviews/2026-05-18-session-1.md:399:+### D1 Audit: Best-Effort Rendering with Placeholders
docs/perf/codex-reviews/2026-05-18-session-1.md:412:+### D2 Audit: Reactivity Contract (Composite Reverse Index)
docs/perf/codex-reviews/2026-05-18-session-1.md:426:+### D3 Audit: Errors Never Cross FFI
docs/perf/codex-reviews/2026-05-18-session-1.md:440:+### D4 Audit: One Writer Per Fact
docs/perf/codex-reviews/2026-05-18-session-1.md:454:+### D5 Audit: Capabilities Report, Never Decide
docs/perf/codex-reviews/2026-05-18-session-1.md:476:+| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
docs/perf/codex-reviews/2026-05-18-session-1.md:477:+| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
docs/perf/codex-reviews/2026-05-18-session-1.md:507:+- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
docs/perf/codex-reviews/2026-05-18-session-1.md:519:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/2026-05-18-session-1.md:535:+**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/2026-05-18-session-1.md:576:+> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
docs/perf/codex-reviews/2026-05-18-session-1.md:601:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/2026-05-18-session-1.md:612:+  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
docs/perf/codex-reviews/2026-05-18-session-1.md:619:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/2026-05-18-session-1.md:636:+- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
docs/perf/codex-reviews/2026-05-18-session-1.md:792:1. Doctrine compliance — any rule weakening?
docs/perf/codex-reviews/2026-05-18-session-1.md:793:2. TODO / FIXME / unimplemented / "for later" snuck in
docs/perf/codex-reviews/2026-05-18-session-1.md:829:/bin/zsh -lc 'rg -n "TODO|FIXME|XXX|unimplemented!|todo!|for later|later|Optional|optional|recommended|Recommended|No action required|defer|deferral|future|worktree remove --force|dropped" docs/perf/m10.5/debt-inventory.md docs/perf/orchestration-log.md docs/plan.md' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/2026-05-18-session-1.md:831:docs/perf/m10.5/debt-inventory.md:12:| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/2026-05-18-session-1.md:842:docs/perf/m10.5/debt-inventory.md:406:| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/2026-05-18-session-1.md:846:docs/plan.md:21:- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
docs/perf/codex-reviews/2026-05-18-session-1.md:848:docs/plan.md:77:**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
docs/perf/codex-reviews/2026-05-18-session-1.md:850:docs/plan.md:345:**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/2026-05-18-session-1.md:852:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/2026-05-18-session-1.md:867:   363	| ProfileCard.placeholder in iOS (D1 evidence) | Design compliance | Correct implementation of D1 (best-effort rendering); refinement in place. |
docs/perf/codex-reviews/2026-05-18-session-1.md:868:   364	| Error strings in JSON payloads (D3 evidence) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. No control flow decisions at boundary. |
docs/perf/codex-reviews/2026-05-18-session-1.md:898:   394	- All cardinal doctrines (D0–D5) are compliant; no design changes needed.
docs/perf/codex-reviews/2026-05-18-session-1.md:910:   406	| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/2026-05-18-session-1.md:926:   422	**Code Quality:** Zero bugs. All 20 code inspection findings are acceptable or justified by design. All cardinal doctrines (D0–D5) are upheld.
docs/perf/codex-reviews/2026-05-18-session-1.md:948:   345	**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/2026-05-18-session-1.md:959:   356	  - Error-shape exhaustion: every typed FFI error path exercised; assert each one becomes a `toast: Option<String>` state field, never a thrown exception across the boundary (D3).
docs/perf/codex-reviews/2026-05-18-session-1.md:966:   363	- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/2026-05-18-session-1.md:983:   380	- Doctrine review (D0–D5) signed off on the FFI surface in writing in `docs/perf/m10.5/doctrine-review.md`.
docs/perf/codex-reviews/2026-05-18-session-1.md:1131:docs/plan.md:363:- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/2026-05-18-session-1.md:1140:/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/plan.md | rg -n "deferred|deferral|NIP-XX|optional|for later|wire it up later|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/2026-05-18-session-1.md:1142:/bin/zsh -lc 'git diff e9cbafa..d660735 -- docs/perf/m10.5/debt-inventory.md | rg -n "optional|recommended|Recommended|READY|production-ready|No Action Required|TODO|FIXME|unimplemented"' in /Users/pablofernandez/Work/nostr-multi-platform
docs/perf/codex-reviews/2026-05-18-session-1.md:1144:18:+| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
docs/perf/codex-reviews/2026-05-18-session-1.md:1152:412:+| Blocking debt markers (TODO/FIXME/XXX/HACK/panic!/unimplemented!/todo!) | 0 | ✓ Clean |
docs/perf/codex-reviews/2026-05-18-session-1.md:1160:40:+**Why this milestone exists separately.** Every milestone M1–M10 has run iOS measurements, but each in service of its own feature. M10.5 is the dedicated *FFI surface* hardening pass — finding and fixing every shape of FFI bug that a non-social-domain consumer (M11 podcast app) would otherwise discover the hard way. This is also the milestone where we delete every shortcut and "TODO: revisit" comment in the FFI layer.
docs/perf/codex-reviews/2026-05-18-session-1.md:1161:58:+- **Zero open `TODO`/`FIXME`/`XXX`/`unimplemented!()`** in `crates/nmp-core/src/ffi.rs`, `crates/nmp-core/src/actor.rs`, `crates/nmp-core/src/relay.rs`, `crates/nmp-core/src/kernel/**`, and the iOS bridge sources (`ios/NmpStress/NmpStress/KernelBridge.swift`, `KernelModel.swift`). Each pre-existing one is either resolved or has an ADR justifying the deferral with a tracking issue.
docs/perf/codex-reviews/2026-05-18-session-1.md:1239:- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
docs/perf/codex-reviews/2026-05-18-session-1.md:1246:No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
docs/perf/codex-reviews/2026-05-18-session-1.md:1256:- [docs/plan.md:363](/Users/pablofernandez/Work/nostr-multi-platform/docs/plan.md:363): “or has an ADR justifying the deferral” weakens the hard “zero open TODO/FIXME/XXX/unimplemented” gate. Fix: remove the deferral escape for scoped files.
docs/perf/codex-reviews/2026-05-18-session-1.md:1263:No executable TODO/FIXME/unimplemented debt was added in the changed files; the hits are doc text. No tests are needed for the plan/log docs themselves, but the audit doc must not be treated as empirical M10.5 coverage.
docs/design/subscription-compilation/outbox.md:215:- The local store insert happens *before* the publish step (optimistic insert), with rollback on `PartiallyFailed` if `required_success_count` is not met. This matches the "atomic with reversibility" reading of doctrine D4 (single writer per fact).
docs/perf/reactivity-bench/1779049354-run-002.json:655:        "working-set gate scenario: cached_events=1000000, hot_events=10000, open_views=100",
docs/perf/reactivity-bench/1779049354-run-002.json:662:    "Memory is an estimate of the actor hot working set plus reverse-index/view structures; cached cold event bodies are modeled as disk-resident.",
docs/design/subscription-compilation/intro.md:15:- **Mailbox cache exists but no consumer.** `crates/nmp-core/src/kernel/ingest.rs:209-233` (`ingest_relay_list`) already parses kind:10002 into `self.author_relay_lists: HashMap<String, AuthorRelayList>` (declared at `crates/nmp-core/src/kernel/mod.rs:269-275` and reserved at `mod.rs:313`). The cache is written; **nothing reads it for routing**. This is the bug doctrine D5 ("capabilities report, never decide") inverted: we have the data, we ignore it.
docs/design/podcast/exit-gate.md:85:Every test file ≤ 500 LOC. The cross-cutting kill-relaunch test is the most load-bearing — it asserts D2 (reactivity) and D4 (single writer per fact) hold under app termination at every state-transition boundary.
docs/design/podcast/exit-gate.md:106:- The doctrine review at `docs/perf/m11/doctrine-review.md` signs off D0–D5 against the M11 surface (template: `docs/perf/m10.5/doctrine-review.md`).
docs/product-spec/appendices.md:11:**`AppState` is bounded by what's open.** It does not contain the event store, the gossip cache, the working set, or anything proportional to the local cache size. It contains:
docs/product-spec/appendices.md:17:The event store, gossip cache, sync watermarks, working set, and signer state all live in the actor and **never cross FFI**.
docs/design/kernel-substrate.md:173:- Pre-formatted display fields per doctrine D1.
docs/design/kernel-substrate.md:429:Integration tests across modules use the real EventStore and a `MockRelay`.
docs/design/kernel-substrate.md:483:1. Phase 1a.1 (kernel substrate prototype) ships with one fixture module (`fixture-todo-core`) demonstrating each of the five trait families. Codegen produces a working `nmp-app-fixture` crate. Desktop iced app renders a TODO list, no business logic in Swift / iced.
docs/design/lmdb/watermarks.md:47:On `LmdbEventStore::open()`, the store reads all `watermarks` rows and builds an in-memory `HashMap<WatermarkKey, WatermarkRow>` for hot lookups. Every `write_watermark` updates both the in-memory map and the LMDB row in a single `RwTxn`. Restart re-derives the map; we don't need a separate cache file.
docs/design/lmdb/gc.md:1:# LMDB sub-design: GC working-set policy
docs/design/lmdb/gc.md:3:> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Formalises the hot-resident / cold-on-disk split required by ADR-0003 (`docs/decisions/0003-working-set-memory.md`).
docs/design/lmdb/gc.md:15:                  computed from the composite reverse-index per ADR-0001
docs/design/lmdb/gc.md:130:2. The composite reverse-index resolves the dependency set to a (small, bounded) set of currently-known event ids — the *view cover*.
docs/design/lmdb/gc.md:143:The relevant figure for the M3 exit gate is **working-set RSS at the configuration described in ADR-0003 §Decision**: 100 active views, 10k hot events, 1M cached on disk, ≤ 100 MB.
docs/design/lmdb/gc.md:153:| LMDB page cache (kernel-owned, *not* counted toward RSS budget) | 0 | OS-paged, evicted under pressure; counts against system memory but not app working set |
docs/design/ffi-hardening.md:19:   (`docs/product-spec/overview-and-dx.md` §1.5 D0–D5), and every ownership
docs/design/ffi-hardening.md:66:| 8 | `nmp_app_open_author(*mut, *const c_char)` | `void nmp_app_open_author(void *app, const char *pubkey)` | UTF-8 C string, expected 64-char lowercase hex pubkey. Hex-validated via `is_hex_pubkey`. Trimmed of leading/trailing whitespace. Empty / non-hex inputs are **silently dropped** (see §7 finding D3-gap). | Silent no-op on null app or null pubkey. |
docs/design/ffi-hardening.md:135:| S1 | Mount/unmount churn | actor recv + refcount | D5 (snapshot bounded), bible #5 |
docs/design/ffi-hardening.md:137:| S3 | Snapshot pressure | listener serialization | bible #9 (≤60 Hz), D5 |
docs/design/ffi-hardening.md:138:| S4 | Reconciler back-pressure | listener channel growth | bible #9, D1 |
docs/design/ffi-hardening.md:141:| S7 | Error-shape exhaustion | every invalid input path | D3 (no errors cross FFI) |
docs/design/ffi-hardening.md:142:| S8 | Subscription planner DOS | OpenView/CloseView storm | D2 (≤60 Hz/view), D5 |
docs/design/ffi-hardening.md:143:| S9 | Relay flap | reconnect + watermark | bible #7, D2 |
docs/design/ffi-hardening.md:197:├── doctrine-review.md       # D0–D5 sign-off (M10.5 exit-gate artifact)
docs/design/ffi-hardening.md:217:| D3-doc | `crates/nmp-core/src/kernel/status.rs::relay_status_for` | Doc that `last_error`/`last_notice` are advisory data fields (D3-compliant: errors as state, not as FFI returns) | 3 min |
docs/design/ffi-hardening.md:225:grep -rEn '(TODO|FIXME|XXX|HACK|unimplemented!|todo!|for later|revisit)' \
docs/design/ffi-hardening.md:235:### 7.2 D3 structural gap (named, not hidden)
docs/design/ffi-hardening.md:241:caller and without setting any state field. This is **D3-compliant in
docs/design/ffi-hardening.md:243:D3-incomplete in the user-visible sense** (no toast surfaces in
docs/design/ffi-hardening.md:246:The debt-inventory's D3 audit (lines 317–334) concludes the same:
docs/design/ffi-hardening.md:261:## 8. Doctrine review checklist
docs/design/ffi-hardening.md:263:Full D0–D5 line-item-to-scenario mapping in
docs/design/ffi-hardening.md:266:| Doctrine | Proven by |
docs/design/ffi-hardening.md:268:| **D0** kernel never grows app nouns | debt-inventory §3 D0 audit + S6 (the kernel does not grow capability variants under churn) |
docs/design/ffi-hardening.md:269:| **D1** best-effort rendering with placeholders | S3 (snapshot pressure) + S10 (long suspend) — placeholder-then-refine path |
docs/design/ffi-hardening.md:270:| **D2** ≤60Hz/view, working-set bound | S2, S3, S8 — emit-rate cap, planner dedup |
docs/design/ffi-hardening.md:271:| **D3** errors never cross FFI | S7 (exhaustion) + §7.2 (toast bridge) |
docs/design/ffi-hardening.md:272:| **D4** one writer per fact | S1, S5 — refcount only mutated on actor thread; reentrancy under same single-writer rule |
docs/design/ffi-hardening.md:273:| **D5** snapshots bounded by what's open | S1 (refcount drives eviction) + S3 (full-state size scales with open views, not store) |
docs/design/lmdb/keys.md:3:> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Fixes the byte layout of every sub-database the NMP store opens. Primary event storage is owned by `nostr-lmdb`; everything below is NMP-owned and lives in NMP-managed sub-databases under the same `lmdb::Environment` (per open question 1 in the master doc).
docs/design/lmdb/keys.md:11:| (multiple) | `nostr-lmdb` | upstream | upstream | event primary, internal filter indexes, kind:5 suppression |
docs/design/lmdb/keys.md:26:Sub-databases are opened lazily on first access and cached on the `LmdbEventStore`.
docs/design/lmdb/keys.md:54:The value is the `event_id`; the primary event itself lives in the `nostr-lmdb` events sub-db. On supersession, the old event-id is fetched from this row, both primary and old `idx_*` rows are deleted, and the value is overwritten with the new id.
docs/design/lmdb/tests.md:3:> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Maps every insert invariant in `docs/product-spec/subsystems.md` §7.1 to a concrete test in `crates/nmp-testing/tests/`. Each test exists for both `MemEventStore` (always) and `LmdbEventStore` (under `#[cfg(feature = "lmdb-backend")]`).
docs/design/lmdb/tests.md:10:    pub store: Box<dyn EventStore>,
docs/design/lmdb/tests.md:16:    pub fn mem() -> Self { /* MemEventStore */ }
docs/design/lmdb/tests.md:17:    pub fn lmdb() -> Self { /* LmdbEventStore in tmp dir */ }
docs/design/lmdb/tests.md:56:Plus a static-assertion-style test ensuring no other public function on `EventStore` writes to the primary store (compile-time check by inspecting trait method list via a build script — deferred to v1.x; v1 covers via review).
docs/design/podcast/lessons.md:106:- "No business logic in native." (Doctrine D0 + AGENTS.md guardrails.)
docs/design/firehose-bench.md:7:> **Prerequisites:** `product-spec.md` (especially §7.1 EventStore, §7.2 planner, §7.3 outbox, §7.8 sync engine, §7.16 metrics); `reactivity.md`; ADRs 0001–0005.
docs/design/lmdb/trait.md:1:# LMDB sub-design: `EventStore` trait
docs/design/lmdb/trait.md:7:`crates/nmp-core/src/store/events.rs` (filename note: `trait` is a Rust keyword, so the file is named `events.rs` and exposes `pub trait EventStore`). Re-exported from `nmp_core::store::EventStore`. The actor (`crates/nmp-core/src/actor.rs`) holds the store as `store: Box<dyn EventStore>`; backends are constructed by the factory in `store/mod.rs::open_event_store(&AppConfig) -> Result<Box<dyn EventStore>, StoreError>`.
docs/design/lmdb/trait.md:144:pub trait EventStore: Send + Sync {
docs/design/lmdb/trait.md:282:## 5. Error semantics (doctrine D3)
docs/design/lmdb/trait.md:288:- `Encoding` → `tracing::error!` with the offending key/namespace; the action that triggered it fails with a `toast: Some("internal storage error; please restart")` per D3.
docs/design/lmdb/trait.md:299:pub struct MemEventStore { /* HashMap-backed; preserves the current kernel state */ }
docs/design/lmdb/trait.md:302:pub struct LmdbEventStore { /* wraps nostr_lmdb::NostrLMDB + NMP sub-dbs */ }
docs/design/lmdb/trait.md:304:pub fn open_event_store(cfg: &AppConfig) -> Result<Box<dyn EventStore>, StoreError> {
docs/design/lmdb/trait.md:306:        StorageBackend::Memory => Ok(Box::new(MemEventStore::new())),
docs/design/lmdb/trait.md:307:        StorageBackend::Lmdb { ref path } => Ok(Box::new(LmdbEventStore::open(path)?)),
docs/design/lmdb/trait.md:312:`MemEventStore` implements every method using `HashMap` / `BTreeMap`. The same test suite runs against both backends with `#[cfg(feature = "lmdb-backend")]` gating only the LMDB-specific edge tests (corruption recovery, oversized values).
docs/design/ffi-hardening/ci.md:86:        --require-doctrines D0,D1,D2,D3,D4,D5 \
docs/design/ffi-hardening/ci.md:182:            fails.append("FFI grep yielded TODO/FIXME tokens; see §7.1")
docs/design/ffi-hardening/ci.md:338:4. Doctrine review (D0–D5) signed off in `doctrine-review.md`.
docs/design/ffi-hardening/scenarios.md:261:working set explodes; relay workers can't send fast enough.
docs/design/ffi-hardening/scenarios.md:272:1. Peak working-set memory during storm ≤ **150 MB** (planner is the
docs/product-spec/api-surface.md:157:Doctrine:
docs/design/ffi-hardening/gates.md:7:2. **§D** doctrine D0–D5 review checklist — each line item maps to
docs/design/ffi-hardening/gates.md:107:| Peak working-set RSS during storm | `<=` | 150 MiB | 200 MiB |
docs/design/ffi-hardening/gates.md:150:## §D. Doctrine D0–D5 review checklist
docs/design/ffi-hardening/gates.md:155:> **Note.** The task brief mentioned "D0–D5". The canonical list in
docs/design/ffi-hardening/gates.md:156:> the spec **is exactly six items: D0, D1, D2, D3, D4, D5.** This
docs/design/ffi-hardening/gates.md:159:> not redundantly re-prove — items beyond D0–D5 are covered by
docs/design/ffi-hardening/gates.md:162:### D0. Kernel never grows app nouns
docs/design/ffi-hardening/gates.md:164:- ✅ **Proof:** [debt-inventory.md §3 D0 audit](../../perf/m10.5/debt-inventory.md) — verified
docs/design/ffi-hardening/gates.md:171:  `docs/perf/m10.5/doctrine-review.md` § D0.
docs/design/ffi-hardening/gates.md:173:### D1. Best-effort rendering — render now, refine in place
docs/design/ffi-hardening/gates.md:185:- 📝 **Sign-off:** doctrine-review.md § D1.
docs/design/ffi-hardening/gates.md:187:### D2. Reactivity contract — composite reverse index, ≤60Hz/view, working-set bound
docs/design/ffi-hardening/gates.md:193:- ✅ **Stress proof (working set):** S8 asserts planner state
docs/design/ffi-hardening/gates.md:197:- 📝 **Sign-off:** doctrine-review.md § D2.
docs/design/ffi-hardening/gates.md:199:### D3. Errors never cross FFI
docs/design/ffi-hardening/gates.md:201:- ⚠️ **Current state:** debt-inventory §3 D3 audit concludes
docs/design/ffi-hardening/gates.md:210:- 📝 **Sign-off:** doctrine-review.md § D3, with the explicit
docs/design/ffi-hardening/gates.md:211:  note that this milestone *closes* the D3-incomplete state
docs/design/ffi-hardening/gates.md:214:### D4. Single writer per fact — caches derive
docs/design/ffi-hardening/gates.md:216:- ✅ **Proof:** debt-inventory §3 D4 audit — single-threaded actor
docs/design/ffi-hardening/gates.md:229:- 📝 **Sign-off:** doctrine-review.md § D4.
docs/design/ffi-hardening/gates.md:231:### D5. Snapshots bounded by what's open
docs/design/ffi-hardening/gates.md:241:- 📝 **Sign-off:** doctrine-review.md § D5.
docs/design/ffi-hardening/gates.md:245:## §D.1 Doctrine sign-off artifact
docs/design/ffi-hardening/gates.md:251:# M10.5 Doctrine Review
docs/design/ffi-hardening/gates.md:253:| Doctrine | Status | Evidence | Reviewer | Date |
docs/design/ffi-hardening/gates.md:255:| D0 | PASS | debt-inventory §3 D0 + S6 metrics.json | <name> | <date> |
docs/design/ffi-hardening/gates.md:256:| D1 | PASS | S3 + S10 metrics.json + S3/screenshots | <name> | <date> |
docs/design/ffi-hardening/gates.md:257:| D2 | PASS | S2/S3/S8 metrics.json | <name> | <date> |
docs/design/ffi-hardening/gates.md:258:| D3 | PASS | S7 metrics.json + toast-bridge merge SHA | <name> | <date> |
docs/design/ffi-hardening/gates.md:259:| D4 | PASS | debt-inventory §3 D4 + S5/S1 metrics.json | <name> | <date> |
docs/design/ffi-hardening/gates.md:260:| D5 | PASS | S1/S3/S8 metrics.json | <name> | <date> |
docs/design/lmdb-schema.md:1:# Design: LMDB schema + EventStore trait + GC policy (M3)
docs/design/lmdb-schema.md:6:> **Prerequisites:** `docs/product-spec/subsystems.md` §7.1 (insert invariants), `docs/decisions/0003-working-set-memory.md` (GC policy intent), `docs/decisions/0009-app-extension-kernel-boundary.md` (DomainModule storage), `docs/design/kernel-substrate.md` §2 (DomainModule trait).
docs/design/lmdb-schema.md:11:## 1. Decision: wrap `nostr-lmdb` behind our own `EventStore` trait
docs/design/lmdb-schema.md:13:**Adopt `nostr-lmdb` as the on-disk byte store. Wrap it behind the NMP `EventStore` trait. Add NMP-owned LMDB sub-databases for the rows `nostr-lmdb` does not model.**
docs/design/lmdb-schema.md:15:The competing options were (1) use `nostr-lmdb` directly via its concrete `NostrLMDB` type (or via `nostr_database::NostrEventsDatabase`), (2) wrap behind our own trait, or (3) hand-roll an LMDB layer.
docs/design/lmdb-schema.md:17:**`nostr-lmdb` gives us** (per `docs.rs/nostr-lmdb`): `save_event(&Event)`, `event_by_id(&EventId)`, `check_id(&EventId) -> DatabaseEventStatus`, `query(Filter) -> Events`, `count(Filter)`, `delete(Filter)`, `wipe()`, `negentropy_items(Filter) -> Vec<(EventId, Timestamp)>`. It owns the page allocator, the LMDB environment, primary by-id store, indexes derived from `Filter`, replaceable / parameterized-replaceable supersession, and NIP-09 delete handling. It is the only mature Rust LMDB store for Nostr events with proven NIP-77 integration; reinventing it is a year of work that we will not recoup.
docs/design/lmdb-schema.md:19:**What `nostr-lmdb` does *not* model** (the gap that justifies a wrapper):
docs/design/lmdb-schema.md:21:| Concern | Why `nostr-lmdb` doesn't cover it | Where NMP needs it |
docs/design/lmdb-schema.md:33:**Therefore.** `EventStore` is a NMP-owned trait, with one production impl `LmdbEventStore` that holds (a) a `NostrLMDB` for the canonical event store and Nostr-shaped queries, and (b) NMP-owned secondary LMDB sub-databases under the same `lmdb::Environment` for the gap rows. The in-memory backend (`MemEventStore`) remains, both for tests and as the web-pre-M15 fallback. See [`lmdb/trait.md`](lmdb/trait.md) for the exact trait shape and the relayed-vs-owned method split.
docs/design/lmdb-schema.md:37:- *Use `NostrLMDB` directly, no wrapper.* Loses every gap row above. Forces the kernel actor to know about LMDB transactions and a non-NMP concrete type, breaking the `Box<dyn EventStore>` substitutability M3 requires.
docs/design/lmdb-schema.md:38:- *Roll our own.* Reinvents NIP-09 / replaceable handling that `nostr-lmdb` already gets right. ~2,000 LOC of avoidable code with a worse bug surface than upstream.
docs/design/lmdb-schema.md:39:- *SQLite-backed `nostr-sdk` store.* Larger memory footprint at our 10k-event hot working set; iOS-disk-WAL fsync cost not justified for this access pattern. Held in reserve for the web port (M15) if IndexedDB OPFS proves unworkable.
docs/design/lmdb-schema.md:46:  trait.rs              — `EventStore` (see lmdb/trait.md)
docs/design/lmdb-schema.md:49:    mod.rs              — `LmdbEventStore` orchestrator
docs/design/lmdb-schema.md:63:## 3. EventStore trait
docs/design/lmdb-schema.md:65:See [`lmdb/trait.md`](lmdb/trait.md) for the exact `pub trait EventStore` signature with all required methods, return types, and the `StoreError` enum. Summary:
docs/design/lmdb-schema.md:75:**Error semantics.** All methods return `Result<T, StoreError>`. Per doctrine D3, store errors do **not** cross FFI — the actor maps every variant to either (a) a typed `Effect` (e.g. `StoreCorrupt`, surfaces via diagnostics + toast), (b) a `tracing::warn!` log + degraded state, or (c) a panic at startup if the LMDB environment refuses to open. The trait itself uses `Result` since it is internal to the actor process.
docs/design/lmdb-schema.md:81:- Primary `events`: `event_id[32]` → `Event` (CBOR via `nostr` crate's serialization). Owned by `nostr-lmdb`.
docs/design/lmdb-schema.md:109:Key: `filter_hash[32] || relay_url_bytes` (no length prefix needed — relay URL is the variable suffix; lookup uses exact key). Populated by M4 (NIP-77) and consulted by M2's planner (cache-coverage check before issuing backfill REQ). Survives restarts; loaded into the actor on startup as a `HashMap<(filter_hash, relay_url), WatermarkRow>` for hot lookups, with all writes going through `EventStore` for durability.
docs/design/lmdb-schema.md:126:## 7. GC working-set policy
docs/design/lmdb-schema.md:142:ADR-0003's numbers are preserved as the M3 exit gate (§11 below): ≤ 100 MB working-set at 100 active views / 10k hot events / 1M cached on disk.
docs/design/lmdb-schema.md:160:Rejected: stuffing provenance into the `Event` row. That requires re-serializing the full `Event` on every relay redelivery (high write amplification — popular events arrive 5–20× from the relay fan-out) and forks the `nostr-lmdb` row format, which we explicitly want to keep upstream-compatible. The sidecar is appended cheaply with a single CBOR re-encode of the (typically small) `sources` vector.
docs/design/lmdb-schema.md:208:| Working-set memory at 100 active views / 10k hot / 1M on disk | ≤ 100 MB resident | Instruments Allocations + `reactivity-bench` working-set scenario |
docs/design/lmdb-schema.md:218:1. **`nostr-lmdb` LMDB environment sharing.** Can we open the same `lmdb::Environment` for both `NostrLMDB`'s sub-databases and our own NMP sub-databases (provenance, watermarks, claims, domain rows)? If yes, we get atomic cross-sub-db transactions for free (a single `RwTxn` covers event + provenance + secondary indexes). If `nostr-lmdb` insists on opening its own `Environment`, we lose that and the insert path needs a two-phase write with crash-recovery logic. Investigate before implementation — may require an upstream PR exposing `Environment` access.
docs/design/lmdb-schema.md:223:6. **`ModuleRegistry::register_domain` API stability.** Adding `DomainFactories` to `ModuleDescriptor` is a non-breaking additive change to the public substrate API (existing callers using only the generic `register_domain::<M>()` continue to compile), but it commits us to keeping `DomainModule::SCHEMA_VERSION` and `DomainModule::migrations` as compile-time-resolvable items rather than object-safe methods. Confirm this with the substrate maintainer before M3 lands — if `DomainModule` is expected to support runtime composition (e.g., plugin loading), we need option (c): the actor passes the live `&[Box<dyn DomainModule>]` to `EventStore::open` instead. Recommended default: stay with `fn`-pointer factories; revisit if a plugin-loading use case appears.
docs/design/lmdb-schema.md:229:- Replaceable supersession (current scattered logic to be centralised in `EventStore::insert`): `kernel/ingest.rs:166-185` (profile replace by `(pubkey, kind)`), `ingest.rs:218-233` (NIP-65 list replace by `(pubkey, 10002)`).
docs/design/lmdb-schema.md:230:- Profile claim refcounting (current in-memory analogue of `EventStore::claim/release`): `kernel/mod.rs:315` (`profile_claims: HashMap<String, BTreeSet<String>>`), `kernel/requests.rs:202` (`claim_profile`), `requests.rs:239` (`release_profile`).
docs/design/lmdb-schema.md:234:The M3 implementation deletes none of the existing files outright — the kernel's `events: HashMap` and `profiles: HashMap` are replaced by calls to `Box<dyn EventStore>` held inside the `Kernel` struct, and the per-field tests (`kernel/tests.rs`) shift to the new trait via `MemEventStore`. No public FFI surface changes.
docs/product-spec/overview-and-dx.md:31:### D0. Kernel + extension modules — no app nouns in `nmp-core`
docs/product-spec/overview-and-dx.md:41:### D1. Best-effort rendering — render now, refine in place
docs/product-spec/overview-and-dx.md:54:### D2. Negentropy first, REQ second
docs/product-spec/overview-and-dx.md:60:### D3. Outbox routing is automatic; manual relay selection is the opt-out
docs/product-spec/overview-and-dx.md:73:### D4. Single writer per fact; caches derive
docs/product-spec/overview-and-dx.md:75:The "single source of truth" doctrine does not mean one cache — there are five layers (durable event store, in-memory working set, view payloads, gossip cache, platform reactive shadow). It means **one writer per fact**, and every downstream cache derives from the writer mechanically. Cache invalidation is not a concept in the public API. Recomputation happens in the actor; the platform receives new derived state.
docs/product-spec/overview-and-dx.md:77:### D5. Snapshots bounded by what's open
docs/design/podcast/wiring.md:11:3. Replace `// MARK: NMP-WIRE — TODO` blocks with `// MARK: NMP-WIRE — wired` consuming the generated `@<View>` property wrapper.
docs/design/podcast/wiring.md:88:- Acceptance: subscribe to a real RSS feed (Tim Ferriss) → library shows it → tap → see ≥ 5 episodes parsed; UI updates as `FetchFeed` action completes (no spinner gate; placeholder rows during fetch per doctrine D1).
docs/design/podcast/wiring.md:201:`grep -RnE '// MARK: NMP-WIRE — TODO' ios/NmpPodcast/Views/ | wc -l` ratchets to zero across the 7 lanes.
docs/design/podcast/wiring.md:212:- [ ] All `// MARK: NMP-WIRE — TODO` in the lane removed

codex
The first pass found the relevant doctrine anchors and the M3 docs. I’m also checking prior project memory for any known review pattern or prior M3 context, then I’ll reduce this to concrete findings.
exec
/bin/zsh -lc 'rg -n "nostr-multi-platform|9fead0e|M3|lmdb|EventStore|Doctrine D0|D2|working-set|working set" /Users/pablofernandez/.codex/memories/MEMORY.md' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
236:- com.podcastr.next.Podcastr, devicectl, 00008150-001E118E3CD2401C, Pablo's iPhone, xcodebuild -showdestinations, /tmp/podcast-rmp-add-toolbar-device-build, App installed, Launched application
250:- the working device path here was `xcodebuild -showdestinations`, then `xcodebuild build` for device id `00008150-001E118E3CD2401C`, then `xcrun devicectl device install app`, then `xcrun devicectl device process launch --terminate-existing com.podcastr.next.Podcastr` [Task 2]
2304:- `xcodegen generate` is required here after adding new Swift files, and the working device path was `xcodebuild -project WeightTracker.xcodeproj -scheme WeightTracker -destination 'platform=iOS,id=00008150-001E118E3CD2401C' build` followed by `xcrun devicectl device install app` and `xcrun devicectl device process launch` [Task 4][Task 5]
2468:- the working iPhone path was `cargo build --target aarch64-apple-ios --release -p tenex-core`, then `xcodebuild -workspace TenexMVP.xcworkspace -scheme TenexMVP -configuration Debug -destination 'platform=iOS,id=00008150-001E118E3CD2401C' -derivedDataPath .codex-derived-ios-device -clonedSourcePackagesDirPath .codex-derived-ios-device/SourcePackages build`, followed by `xcrun devicectl device install app` and `xcrun devicectl device process launch --terminate-existing com.tenex.mvp` [Task 1]
2637:- the working iPhone deployment sequence was `xcodebuild ... -destination 'id=00008150-001E118E3CD2401C' -derivedDataPath /tmp/WinTheDayDerived build`, then `xcrun devicectl device install app`, then `xcrun devicectl device process launch`; `No provider was found` warnings from `devicectl` were noisy but non-fatal here [Task 3]
2943:- rust migration, tenex-daemon, daemon.ts, M1, M2, M3, M8, M9, quality gates, publish_outbox, worker_protocol, can I use it yet, test:rust:publish-interop

exec
/bin/zsh -lc "nl -ba docs/product-spec/overview-and-dx.md | sed -n '25,85p'" in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc 'wc -l docs/design/lmdb-schema.md docs/design/lmdb/gc.md docs/design/lmdb/keys.md docs/design/lmdb/tests.md docs/design/lmdb/trait.md docs/design/lmdb/watermarks.md' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     234 docs/design/lmdb-schema.md
     195 docs/design/lmdb/gc.md
     150 docs/design/lmdb/keys.md
     223 docs/design/lmdb/tests.md
     312 docs/design/lmdb/trait.md
     191 docs/design/lmdb/watermarks.md
    1305 total

 succeeded in 0ms:
    25	---
    26	
    27	## 1.5 Cardinal doctrines
    28	
    29	Six named principles that subsume the rest of this spec. Every API decision answers to at least one of these; conflicts between them resolve in the order listed.
    30	
    31	### D0. Kernel + extension modules — no app nouns in `nmp-core`
    32	
    33	Per ADR-0009, NMP is a Nostr-native app kernel plus extension modules. The kernel provides substrate; protocol modules and app modules contribute typed variants via `ViewModule`, `ActionModule`, `DomainModule`, `CapabilityModule`, and `IdentityModule`. If implementing a real app requires adding domain nouns to `nmp-core`, the kernel boundary is wrong and must change.
    34	
    35	This rules out:
    36	
    37	- `nmp-core` becoming a junk drawer of every consumer's domain concepts.
    38	- App-specific business logic in Swift, Kotlin, or TypeScript shells.
    39	- Closed FFI enums that prevent modules from contributing typed views, actions, updates, capabilities, or identity scopes.
    40	
    41	### D1. Best-effort rendering — render now, refine in place
    42	
    43	Apps built with this framework **never withhold cached data and never block on fetches**. Every view payload field carries a value, not a "loading" status. Missing display names default to a shortened npub; missing pictures default to a deterministic identicon URI; missing timestamps default to "now". When a more authoritative value (e.g., the author's kind:0) arrives later, the view payload updates in place and the affected cell re-renders. The UI never sees a spinner gating already-renderable content.
    44	
    45	The doctrine is enforced by the view payload **types**: display fields are non-`Option`, placeholders are part of the type contract, and freshness is exposed (when relevant) as an optional badge hint, not a render gate. There is no `if has_profile { render } else { spinner }` pattern available in the API — the framework does not provide one.
    46	
    47	This rules out, by construction, the most common Nostr-client failure modes:
    48	
    49	- Hiding a post because the author's profile hasn't loaded yet.
    50	- Replacing cached profile metadata with a spinner because "we might have something newer."
    51	- Refusing to render threads because the root event isn't in cache.
    52	- Profile-picture flicker between cached and placeholder.
    53	
    54	### D2. Negentropy first, REQ second
    55	
    56	NIP-77 negentropy reconciliation is the default backfill mechanism. Every `(filter, relay)` pair the app touches is treated as a tracked sync target with a watermark. Live REQ remains the tailing path, but historical gaps consult coverage first and prefer sync over REQ scans when relays support it.
    57	
    58	This is not a product feature you opt into later; it is a subscription policy built on explicit coverage metadata. See §7.8.
    59	
    60	### D3. Outbox routing is automatic; manual relay selection is the opt-out
    61	
    62	Per NIP-65, reads and writes are routed to the relevant relays by framework policy without normal app code specifying them. Subscriptions with `authors` filters route to those authors' write relays; publishes go to the author's write relays plus tagged recipients' inbox relays; discovery falls back to a configurable indexer set.
    63	
    64	The safe public path does not ask the developer to pick relays per operation. Explicit override and diagnostic/test paths exist, but they are named, observable, and excluded from the default app-building flow.
    65	
    66	This rules out, by construction:
    67	
    68	- Posts to relays the author hasn't declared as write relays.
    69	- DMs leaked to public relays.
    70	- Silent reads against a default relay set that miss an author's actual relays; unknown relay lists surface as coverage/diagnostic state and use a bounded fallback policy.
    71	- Hand-rolled fan-out logic in app code.
    72	
    73	### D4. Single writer per fact; caches derive
    74	
    75	The "single source of truth" doctrine does not mean one cache — there are five layers (durable event store, in-memory working set, view payloads, gossip cache, platform reactive shadow). It means **one writer per fact**, and every downstream cache derives from the writer mechanically. Cache invalidation is not a concept in the public API. Recomputation happens in the actor; the platform receives new derived state.
    76	
    77	### D5. Snapshots bounded by what's open
    78	
    79	What crosses FFI is the projection through currently-open views, not the underlying event store. `AppState` carries small screen-shaped data plus a map of `ViewId → ViewPayload` for views currently in use. Closing a view evicts its payload from the snapshot. The event store itself never crosses FFI. See §6.2 and the FFI architecture appendix (§A1).
    80	
    81	---
    82	
    83	## 2. Audience and use cases
    84	
    85	**Primary audience.** Application developers building Nostr clients for production distribution on iOS, Android, desktop, and web — including LLM-driven and inexperienced developers who lack the protocol literacy to navigate Nostr's footguns unaided.

exec
/bin/zsh -lc "nl -ba docs/product-spec/subsystems.md | sed -n '120,275p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   120	    pub contacts_view_id: ViewId,
   121	    pub mailboxes_view_id: ViewId,
   122	    pub mutes_view_id: ViewId,
   123	    pub status: AccountStatus,
   124	}
   125	```
   126	
   127	Signers are managed entirely in `nmp-core`. The initial product signer catalog is:
   128	
   129	- Local key (raw nsec, stored encrypted via `KeyringCapability`)
   130	- NIP-49 (password-encrypted private key)
   131	- NIP-46 bunker / Nostr Connect
   132	- NIP-07 (web only)
   133	- External — Android Amber (NIP-55) bridged via `ExternalSignerCapability`
   134	
   135	The signer abstraction inside `nmp-core` is a Rust trait with `sign(unsigned_event) -> Future<signed_event>`. Adding a signer kind is an internal task; external developers do not implement signers.
   136	
   137	### 7.5 Actions catalog
   138	
   139	Actions live in `nmp-actions`. Each action is a Rust async fn taking an action context (`event_store`, `signer`, `publisher`, `active_account`) and producing zero or more signed events. The actor runs actions on its tokio runtime; results route through `InternalEvent` back to the actor for atomic state update.
   140	
   141	Action authoring contract for the framework's own contributors (not exposed at FFI):
   142	
   143	```rust
   144	#[async_trait]
   145	pub trait Action: Send + Sync + 'static {
   146	    type Output: Send + 'static;
   147	    async fn run(self, cx: &ActionCx) -> Result<Self::Output>;
   148	}
   149	```
   150	
   151	Built-in actions (long-term product catalog): the AppAction variants listed in §6.3 each map to one Action implementation. v1 ships only the generic kernel actions named in `docs/plan.md`. Custom actions are first-class via a sister crate pattern (apps add their own actions crate that depends on `nmp-actions`).
   152	
   153	Atomicity invariant: an action's local event-store commit, side-effect intent, and ledger transition happen as actor messages with one parent action id. The action future runs on the tokio runtime, but all state mutation happens in `handle_message`. There is no public API that lets a developer publish, upload, sign, or issue an NWC request without a renderable action-ledger row.
   154	
   155	The ledger is general, not relay-only. It can represent local optimistic commit, signer prompt, per-relay publish attempt, HTTP upload, NWC request, retry, repair, partial failure, timeout, and final status. Relay publishes additionally track attempted/acked/failed/timed-out by relay plus required success count.
   156	
   157	### 7.6 Views
   158	
   159	`nmp-views` defines `ViewSpec` and all built-in `ViewPayload` variants:
   160	
   161	| View | Inputs | Payload |
   162	|---|---|---|
   163	| Profile | `pubkey` | latest kind-0 parsed; pre-formatted display name; verified domain |
   164	| Contacts | `pubkey` | parsed kind-3 follow list, with per-followee metadata |
   165	| Mailboxes | `pubkey` | parsed kind-10002 |
   166	| Mutes | `pubkey` | parsed kind-10000 |
   167	| Blossom servers | `pubkey` | parsed kind-10063 |
   168	| Timeline | `filter` (kind, authors, hashtags, time window) | sorted slice with pagination cursor |
   169	| Thread | `root_event_id` | tree with per-node metadata |
   170	| Replies | `event_coord` | flat list with per-reply metadata |
   171	| Reactions | `event_coord` | grouped count by emoji + per-pubkey list |
   172	| Conversation list | `account_pubkey` | sorted DM threads with unread counts and latest message preview |
   173	| Conversation | `peer_pubkey` | paginated decrypted messages |
   174	| Zap history | `account_pubkey` | bidirectional list |
   175	| Wallet balance | `wallet_id` | balance + pending transactions |
   176	| WoT rank | `pubkey` | trust score + reasoning |
   177	| Search | `query`, `kinds`, `time_window` | result list |
   178	
   179	Each payload type carries **pre-formatted** display strings (timestamps in user locale, npub-shortened forms, sat amounts). Per bible doctrine: no platform-side formatting.
   180	
   181	**Best-effort field contract (per doctrine D1).** Every display-bearing field in every view payload is **non-optional** and has a defined placeholder when the underlying data is missing:
   182	
   183	| Field | Placeholder when missing |
   184	|---|---|
   185	| Display name | Shortened npub: `npub1abc…xyz` |
   186	| Picture URL | Deterministic identicon URI derived from pubkey |
   187	| NIP-05 verified domain | empty string (UI conditionally renders a checkmark only when non-empty) |
   188	| Timestamp string | "just now" |
   189	| Reaction count | 0 |
   190	| Zap total | 0 sats |
   191	| Content body (if missing) | empty string (the item still renders; only the body region is blank) |
   192	
   193	When the underlying data arrives — kind:0 for an author, kind-9735 zap receipts for a note, the actual decrypted body for a DM — the view payload updates in place, the platform's reactive primitive detects the change, and only the affected cell re-renders. No spinner is ever shown for already-rendered cells.
   194	
   195	**Stale freshness is exposed, not gated.** Each enriched-from-cache field may optionally carry a sibling field `xxx_freshness: FreshnessHint` (recent, hours_old, days_old, never_verified). UI may choose to render a small badge. The framework never withholds the underlying value based on freshness.
   196	
   197	**Concrete example: lean timeline payload.**
   198	
   199	```rust
   200	#[derive(Clone, uniffi::Record)]
   201	pub struct TimelineView {
   202	    pub cursor: Cursor,
   203	    pub items: Vec<TimelineItem>,
   204	    pub has_more: bool,
   205	}
   206	
   207	#[derive(Clone, uniffi::Record)]
   208	pub struct TimelineItem {
   209	    pub id: String,                   // event id hex
   210	    pub author_pubkey: String,
   211	    pub author_display: String,       // never empty; npub-shortened if no kind:0
   212	    pub author_picture: String,       // never empty; identicon URI if no kind:0
   213	    pub author_nip05_domain: String,  // empty if not verified
   214	    pub content_preview: String,      // pre-truncated for list display
   215	    pub created_at_display: String,   // pre-formatted, locale-aware
   216	    pub reaction_summary: ReactionSummary,
   217	    pub zap_sats_total: u64,
   218	    pub reply_count: u32,
   219	    pub repost_of: Option<EventCoord>,
   220	    pub quote_of: Option<EventCoord>,
   221	}
   222	```
   223	
   224	`TimelineItem` is a flat summary. The full event content, raw tags, signature, and provenance live in the event store inside Rust and do not cross FFI. This matches the precedent set by the bible's reference implementation (Pika): chat list is summaries; current chat loads full content on demand.
   225	
   226	View warmth: a view stays cached for 30 seconds after its last claim is dropped (configurable). Re-opening within the window costs zero relay traffic and zero re-sync.
   227	
   228	Post-v1 content rendering contract: protocol-aware content parsing lives in Rust, not in platform shells. The content layer emits serializable nodes for text, links, NIP-19/NIP-21 entities, hashtags, media hints, mentions, quotes, and truncation boundaries. Platform shells render those nodes and may style them, but they do not parse Nostr content or decide URL/media safety policy.
   229	
   230	### 7.7 Web of Trust
   231	
   232	`nmp-wot` ships as an optional subsystem (gated by `AppConfig.wot_enabled`). On enable:
   233	
   234	- Loads the active account's follow graph to a configurable depth (default 2).
   235	- Computes per-pubkey trust scores (default algorithm: simple in-degree weighted by depth; pluggable via a trait).
   236	- Exposes a global filter: when on, every view applies the score threshold before emitting; pubkeys below the threshold are tagged but rendered with a "low trust" UI hint (the renderer chooses; the payload exposes the score).
   237	
   238	Computation is incremental; updates to follow lists update scores without recomputing from scratch.
   239	
   240	### 7.8 Sync engine (live REQ plus NIP-77 backfill)
   241	
   242	Per doctrine D2, live views tail with REQ immediately and use NIP-77 as the preferred historical backfill mechanism when support can be proven. The sync engine is a planner policy over cache coverage, relay capabilities, and progress state.
   243	
   244	**Position in the stack.**
   245	
   246	```
   247	View opens → Live REQ handler starts → Planner consults coverage → Sync engine reconciles gaps → EventStore inserts → ViewBatch emits
   248	                                ↓ (fallback)
   249	                                bounded fetch / REQ scan
   250	```
   251	
   252	**Watermarks as a first-class type.** The engine reads and writes the `watermarks` table introduced in §7.1. A watermark answers two questions:
   253	
   254	- Has this `(filter, relay)` pair ever been synced?
   255	- If so, up to what timestamp?
   256	
   257	Answers to those questions inform every backfill, every fallback-loader decision, and every "is this cache miss authoritative?" check.
   258	
   259	**Three triggers, all built-in.**
   260	
   261	1. **App foreground.** On `AppAction::Foreground`, the engine schedules an incremental sync for the active user's home filter (kind:1, kind:6, kind:7 matching followed authors) against their write relays. Runs in the tokio runtime; emits `SyncState` updates as it progresses; no UI blocking.
   262	2. **View open.** When a view opens whose filter has a gap (per watermark/coverage), the engine reconciles the gap concurrently with the live REQ tail. Progress is visible in `SyncState`; the view payload streams in as events land.
   263	3. **Relay reconnect.** On reconnect, the planner re-establishes live REQs and schedules a coverage-aware gap fill. The gap between disconnect and reconnect is filled by sync when possible, not by re-fetching from scratch.
   264	
   265	**Manual sync as an action.** `AppAction::RunSync { spec }` lets apps trigger arbitrary reconciliations (e.g., "sync this user's last 30 days of articles"). Same engine, different trigger.
   266	
   267	```rust
   268	pub struct SyncSpec {
   269	    pub filter: Filter,
   270	    pub relay: String,
   271	    pub time_window: Option<(u64, u64)>,
   272	    pub direction: SyncDirection,           // Pull, Push, Bidirectional
   273	    pub on_completion: SyncCompletionAction,
   274	}
   275	```

exec
/bin/zsh -lc "nl -ba docs/decisions/0003-working-set-memory.md | sed -n '1,220p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# ADR 0003: Memory budget is for working set, not total cached events
     2	
     3	**Date:** 2026-05-17
     4	**Status:** accepted
     5	**Supersedes:** `reactivity.md` rev 0 §10.3 memory budget
     6	
     7	## Context
     8	
     9	The initial gate read "≤ 100 MB at 100k events / 100 views." The reactivity-bench harness (run 001) reported 130.8 MB at 1M events, failing the gate. But this is misleading: holding 1M events resident in memory is the anti-pattern the spec already calls out for the durable storage backend (LMDB / SQLite / IndexedDB / nostrdb).
    10	
    11	The actor should keep a **bounded working set** of hot events in memory; cold events live on disk. The reverse index can cover both — it keys on attributes, not event bodies.
    12	
    13	## Decision
    14	
    15	The memory budget targets **working-set memory at typical active load**, not total cached events.
    16	
    17	| Metric | Budget |
    18	|---|---|
    19	| Working-set memory at 100 active views, 10k hot events | ≤ 100 MB |
    20	| Total cached events on disk | unbounded (or capped by backend quota) |
    21	
    22	Working-set policy:
    23	
    24	- **Hot:** events referenced by any open view's claim set, plus a configurable recency window (default: most recent 5,000 events globally).
    25	- **Cold:** everything else, on disk only.
    26	- **Eviction:** LRU among hot events not currently claimed.
    27	
    28	The reverse index indexes both hot and cold events. Lookup returns view ids immediately; event bodies for delta construction load lazily and synchronously via the storage backend.
    29	
    30	Projection caches (`author_display`, `reaction_summary`, etc.) are LRU-bounded by referenced-view count; not every pubkey ever seen stays in the projection cache.
    31	
    32	## Consequences
    33	
    34	- The 1M-events-resident scenario is no longer a failure — it's an unintended test of an unintended configuration. Re-run with bounded working set.
    35	- Cold-event delta construction has a one-time disk hit; this is acceptable for replaceable events (kind:0 re-load on profile fan-out) but worth measuring.
    36	- Eviction policy needs explicit design; LRU is the default but priority-ordered (e.g., never evict claimed events) is the real invariant.
    37	
    38	## Alternatives considered
    39	
    40	- **Keep absolute gate, raise number.** Rejected — doesn't address the underlying anti-pattern.
    41	- **Cap total cached events.** Rejected — the storage backend already handles this; the framework should not duplicate.
    42	- **All-in-memory cache.** Rejected — doesn't scale and is contrary to the storage abstraction.
    43	
    44	## Validation
    45	
    46	Re-run reactivity-bench with bounded working set; require ≤ 100 MB at 100 views / 10k hot events / 1M cached events on disk.

exec
/bin/zsh -lc "nl -ba docs/product-spec/subsystems.md | sed -n '1,120p'" in /Users/pablofernandez/Work/nostr-multi-platform
exec
/bin/zsh -lc "nl -ba docs/plan.md | sed -n '1,175p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# Product Spec: Subsystems
     2	
     3	[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)
     4	
     5	## 7. Subsystem specifications
     6	
     7	### 7.1 EventStore
     8	
     9	Single instance per `FfiApp`, owned by the actor. Public to the framework (not to native).
    10	
    11	Behaviors guaranteed at insert time:
    12	
    13	| Concern | Behavior |
    14	|---|---|
    15	| Insert API | Every event enters through one actor-owned insert path returning `InsertOutcome`; no caller mutates indexes or storage directly. |
    16	| Signature/delegation validity | Verified before any tombstone, provenance, replaceable-index, or durable-storage mutation. |
    17	| Duplicate id | Merge relay provenance set; keep earliest `received_at`; do not overwrite. |
    18	| Replaceable kinds (0, 3, 10000-19999) | Compare `(pubkey, kind)` against existing; keep newest `created_at`; tie-break by lexicographically smallest `id`. |
    19	| Parameterized replaceable (30000-39999) | Compare `(pubkey, kind, d-tag)`; same supersession rule. |
    20	| Kind 5 (delete) | After verification, scan referenced `e` and `a` tags and remove matching events authored by the deleter. Persisted as tombstone so later re-insertion is suppressed. Tombstone timestamp is the maximum delete timestamp observed for that target. |
    21	| NIP-40 expiration | Schedule a tokio timer to remove the event at the expiration timestamp; on actor restart, scan and re-schedule. |
    22	| NIP-26 delegation | Validate delegation tag at insert; reject malformed. |
    23	| Ephemeral events | Delivered to live consumers but not durably stored. |
    24	| Provenance | Every event records typed sidecar provenance: relay URL, first seen, last seen, source, and deterministic primary relay. |
    25	| Query matching | Storage backends may return candidates; every result is re-run through the canonical matcher before it affects state or views. |
    26	
    27	Storage backend is configurable via `AppConfig.storage_backend` (LMDB or SQLite-style native backend, IndexedDB/OPFS strategy for web, final choice resolved before v1). The store wraps the Rust Nostr SDK protocol types, but NMP owns the application-kernel storage traits because the app kernel needs typed provenance, action ledger rows, relay metadata, domain records, and bounded-view indexes in addition to raw events.
    28	
    29	GC: a claim-based collector tracks `view_id → Vec<event_id>` references. View close drops claims. A periodic `prune()` removes events with zero claims that are also absent from declared "pinned" sets (sessions' contact-list events, sessions' relay-list events).
    30	
    31	**Sync watermarks.** The store maintains a per-`(filter_signature, relay_url)` table:
    32	
    33	```
    34	watermarks {
    35	  filter_sig: Hash,            // canonicalized filter
    36	  relay_url: String,
    37	  synced_up_to: u64,           // unix seconds; "we have everything matching this filter on this relay up to T"
    38	  last_sync_method: SyncMethod, // Negentropy | ReqScan | Manual
    39	  bytes_saved_vs_req: u64,     // cumulative, for diagnostics
    40	  updated_at: u64,
    41	}
    42	```
    43	
    44	Watermarks are durable. On startup they are loaded into the actor; they survive app restarts. The planner (§7.2) consults them before issuing any backfill, and the sync engine (§7.8) updates them after every reconciliation.
    45	
    46	A cache-miss query against a fully-synced `(filter, relay)` pair is **authoritative**: the answer is "this event does not exist on that relay." A cache-miss against an unsynced pair triggers either a sync (if NIP-77 supported) or a fallback fetch.
    47	
    48	Fallback loading is split by need:
    49	
    50	- Pointer/address misses: cache-first lookup for event id or replaceable address, batched and deduped across waiting views, then relay hints, then configured fallback sources.
    51	- Tag-value and timeline-window misses: bounded historical window loads that record what range is still unknown.
    52	- Authoritative absence: only a complete coverage record/watermark can turn a miss into "not found." A non-empty cache result is not proof that a query is complete.
    53	
    54	The default loader queries open relays and configured sources. Users can add custom sources (CDN cache, local mirror, etc.) through app-kernel extension points, but loaded events still enter through the same verified insert path.
    55	
    56	### 7.2 Subscription planner
    57	
    58	Owns the mapping from `ViewSpec` → `Vec<Filter>` → `Vec<RelayUrl>` → on-the-wire REQ.
    59	
    60	Behaviors:
    61	
    62	- **Live tail first.** Live subscriptions register their local handler and start REQ tailing immediately. Historical backfill runs beside it, not before it.
    63	- **Coverage-aware backfill.** Before issuing historical traffic, the planner consults cache coverage/watermarks (§7.1). Complete coverage serves from cache; partial coverage schedules a gap fill; unknown coverage triggers bounded fetch/REQ or NIP-77 if supported. A non-empty cache result is never treated as complete by itself.
    64	- **Logical vs. wire subscriptions.** A logical subscription belongs to a view/action/monitor. A wire subscription belongs to a relay. Many logical consumers may share one wire REQ, and each consumer still receives only events matching its canonical filter.
    65	- **Coalescing.** Filters that are equal or safely subsumable into a single broader filter share one REQ per relay. The planner maintains a formal merge lattice for `limit`, `since`, `until`, multi-filter arrays, and tag operators.
    66	- **Loader integration.** Pointer/address/tag/timeline misses go through the pointer loader registry with cache-first batching, dedupe, relay hints, cancellation on view close, and explicit missing-window state.
    67	- **Auto-close.** REQs without consumers are CLOSE'd. One-shot filters (those with no live subscribers, only an `until` upper bound) are CLOSE'd on EOSE.
    68	- **Buffering.** Inbound events are batched to ≤ 60Hz per view (configurable). Batches turn into one `ViewBatch` per tick.
    69	- **Backpressure.** If platform-side rendering falls behind, the planner drops `ViewBatch` updates in favor of a single `FullState` catch-up. View payload semantics make this lossless.
    70	- **Reconnect.** On relay reconnect, the planner restores live REQs and schedules a coverage-aware gap fill. View payloads do not reset.
    71	
    72	### 7.3 Outbox routing
    73	
    74	Per doctrine D3, NIP-65 routing is the long-term default policy for reads and writes. v1 ships only the relay-target resolver seam and explicit/simple resolver; this subsystem is the post-v1 policy layer that consumes that seam.
    75	
    76	**Resolution algorithm.**
    77	
    78	| Operation | Relay set |
    79	|---|---|
    80	| Subscription with `authors` filter | Union of each pubkey's write relays (kind-10002), deduplicated. Pubkeys without known mailboxes trigger an opportunistic kind-10002 fetch from indexer relays. |
    81	| Subscription with `p` tag filter or notifications | Union of each tagged pubkey's inbox relays. |
    82	| Subscription with neither | Active session's read relays. |
    83	| Publish of any signed event | Author's write relays. |
    84	| Publish with `p` tags (DMs, mentions, reactions) | Author's write relays **plus** each tagged pubkey's inbox relays. |
    85	| DM (NIP-17 gift-wrapped) | **Only** resolved recipient inbox relays. Never the author's write relays. Never the active session's "default" relays. Missing recipient inbox relays fail closed. |
    86	| Discovery (kind-10002 fetch for unknown pubkeys) | Configurable indexer relay set (default: a curated list of high-coverage relays). |
    87	
    88	**Why this prevents specific failure modes.**
    89	
    90	- "Publish leaked to wrong relays" → ruled out by the safe API. The developer cannot supply a relay list to `SendNote`. Explicit overrides are named, one-shot, and debug-flagged in logs.
    91	- "DM accidentally public" → ruled out by the safe API. The DM publish path consults only resolved inbox relays; there is no fallback-to-all-relays path for gift wraps.
    92	- "Reads missing an author's actual relays" → bounded and surfaced. If the author's kind-10002 is reachable it is opportunistically fetched on first contact; if not, coverage and diagnostic state expose the miss risk and configured fallback policy.
    93	- "Hand-rolled fan-out logic" → no API surface for it.
    94	
    95	**Per-pubkey relay-list lifecycle.**
    96	
    97	- First contact with an unknown pubkey → enqueue kind-10002 fetch from indexer relays.
    98	- Fresher kind-10002 arrives → invalidate dependent subscriptions, recompute relay sets, re-issue REQs as needed.
    99	- Kind-10002 missing for a pubkey after N seconds → fall back to indexer set for reads only; do not publish to indexers.
   100	
   101	The gossip cache is the `nostr-gossip` crate; backend selection (in-memory vs SQLite) follows the storage backend choice. Watermarks (§7.1) intersect with outbox: a sync watermark is keyed by `(filter, relay)` and naturally tracks per-author per-relay coverage.
   102	
   103	### 7.4 Sessions
   104	
   105	`SessionState` holds:
   106	
   107	```rust
   108	pub struct SessionState {
   109	    pub accounts: Vec<Account>,
   110	    pub active: Option<String>,             // pubkey
   111	    pub status: SessionStatus,              // Loading / Syncing / Online / Offline
   112	    pub last_activity_ms: u64,
   113	}
   114	
   115	pub struct Account {
   116	    pub pubkey: String,
   117	    pub display: AccountDisplay,            // pre-formatted name + npub
   118	    pub signer_kind: SignerKind,
   119	    pub profile_view_id: ViewId,            // points into ViewSnapshots
   120	    pub contacts_view_id: ViewId,

 succeeded in 0ms:
     1	# Build & Validation Plan
     2	
     3	> Companion to `docs/product-spec.md` (what we ship) and the design docs in `docs/design/` (how each subsystem works). This document defines **the single ladder of milestones**, each one a runnable product that proves a specific architectural claim with real (not modeled) evidence.
     4	
     5	> **Four arcs:** Kernel substrate + Nostr social stack (M0–M10) → FFI hardening + iOS empirical proof (M10.5) → kernel-boundary proof with a non-social-domain app (M11, the **`../podcast` rebuild on NMP**) → wallet/WoT + cross-platform + release (M12–M17).
     6	
     7	> **Each milestone is gated.** Every milestone ends with: a runnable artifact, automated tests in `nmp-testing`, a measured-numbers report in `docs/perf/m<N>/`, and an explicit ADR if a design decision was revised in flight. **No silent endings.** **No "for later" carve-outs** — if a slice is in the milestone scope, it ships in that milestone, or the milestone is not done.
     8	
     9	> **The doctrine is final** (`docs/product-spec.md` §1.5): D0 kernel never grows app nouns · D1 best-effort rendering with placeholders · D2 reactivity contract (composite reverse index, ≤60Hz/view, working-set bound) · D3 errors never cross FFI · D4 one writer per fact · D5 capabilities report, never decide. Every PR is reviewed against this rubric; a change that makes any doctrine harder to enforce is rewritten or rejected.
    10	
    11	---
    12	
    13	## 0. Where we are right now
    14	
    15	Honest accounting before forecasting forward.
    16	
    17	### Implemented and running
    18	
    19	- **Kernel substrate** in `crates/nmp-core` (~3,800 LOC): actor on a dedicated OS thread, mailbox-driven (ADR feedback adopted — relay reads happen in tokio reader tasks, the actor blocks on its own channel with deadline timeouts), substrate trait families (`DomainModule`, `ViewModule`, `ActionModule`, `CapabilityModule`, `IdentityModule`) in `nmp-core/src/substrate/`, ingest pipeline (`kernel/ingest.rs`), claim/release refcounting for profile interest (commit `23ae829`), composite reverse-index dependency tracking.
    20	- **Live Nostr-connected iOS app** in `ios/NmpStress` (~1,375 LOC Swift): SwiftUI shell wired to the Rust kernel via raw C FFI. Connects to `wss://relay.primal.net` (content) + `wss://purplepag.es` (indexer). Renders seed-driven timeline from union of pablof7z + fiatjaf + jb55 follow lists. Profile resolution with placeholders → in-place refinement on kind:0 arrival per doctrine D1. Thread view. Diagnostics screen showing relay status, logical interests, wire subscriptions (ADR-0007).
    21	- **Fixture proving the kernel boundary** in `crates/fixture-todo-core` (~304 LOC) plus generated `apps/fixture/nmp-app-fixture`: a non-Nostr TODO module implementing all five substrate trait families, with codegen producing the per-app crate. Proves the kernel works for arbitrary domains, not just Nostr.
    22	- **Codegen tool** in `crates/nmp-codegen` (~423 LOC): reads `nmp.toml`, produces a per-app crate, has determinism tests.
    23	- **Benches** in `crates/nmp-testing`: `reactivity-bench` (composite-key reverse index + coalescer + working set; run 002 passed all ADR-0001..0004 gates) and `firehose-bench` (replay + capture + live modes; replay scenarios pass the modeled budget contract).
    24	- **Perf reports** in `docs/perf/` documenting reactivity-bench run 002, firehose-bench replay runs, and three iOS measurement reports (relay lifecycle, profile/thread subscriptions, the primal slice baseline).
    25	- **Architecture decisions** locked in 10 ADRs (`docs/decisions/0001`–`0010`).
    26	
    27	### Designed but not implemented
    28	
    29	- LMDB / IndexedDB persistent storage (in-memory only today).
    30	- NIP-65 outbox routing (hardcoded content + indexer relays today).
    31	- NIP-77 negentropy sync.
    32	- NIP-42 relay auth.
    33	- Multi-account / multi-session model and account switching.
    34	- Signer trait + local-key signer + NIP-46 bunker signer.
    35	- Action ledger + write path (compose / react / repost / quote).
    36	- NIP-17 messaging and the NSE companion crate.
    37	- Blossom uploads / downloads with resumable progress.
    38	- Wallet stack (NWC, NIP-57 zaps, Cashu, nutzaps).
    39	- Web-of-Trust subsystem.
    40	- UniFFI bindings (current iOS bridge is raw C FFI).
    41	- Android shell, Desktop shell, Web shell.
    42	- The `nmp` CLI scaffolding tool.
    43	- A non-Nostr-shaped product (podcast app) demonstrating the kernel boundary in production.
    44	
    45	### Gaps in the prior plan that this rewrite addresses
    46	
    47	- The prior plan was phase-numbered (Phase 1, 2, …) without explicit *demoable products* per phase.
    48	- NIP-42 wasn't covered.
    49	- Subscription compilation (the load-bearing NDK/Applesauce lesson) wasn't elevated as its own milestone.
    50	- Blossom and media-capability lifecycle (long-running, resumable, background) were one bullet under Phase 6.
    51	- No milestone proved the kernel boundary for a fundamentally non-social product.
    52	- The plan didn't reflect that M0 and M1 are largely done.
    53	- **No dedicated FFI hardening + iOS empirical proof gate before the kernel-boundary proof.** The prior M11 implicitly assumed the FFI surface was ready; this rewrite makes it a separate milestone (M10.5).
    54	- **M11 was generic.** This rewrite ties it concretely to `/Users/pablofernandez/src/podcast` (the fully-functional Swift app) as the rebuild target, with copy-first UI fidelity and an explicit view-by-view module mapping.
    55	
    56	The plan below is a single ladder of eighteen milestones (M0–M17, with M10.5 inserted as the FFI gate), each producing a runnable artifact, ordered so that each milestone strictly adds capabilities to the prior demoable product.
    57	
    58	---
    59	
    60	## 1. Principles of execution
    61	
    62	1. **Each milestone is a runnable product.** Not a feature branch; a thing you can build, launch on real hardware, and demo. Unit tests verify correctness; the milestone product validates the architecture.
    63	2. **Real measured evidence over modeled budgets.** Modeled passes in `firehose-bench` replay establish the budget contract. Real passes in `firehose-bench live` against the iOS / Android / Desktop / Web app are the actual gate.
    64	3. **Capability layering is strict.** Each milestone adds exactly one new architectural ingredient on top of the previous demo. No "we'll wire it up later" — wiring is the milestone.
    65	4. **The doctrine rubric is final.** Every PR is reviewed against the cardinal doctrines (`product-spec.md` §1.5, D0–D5). A change that makes any doctrine harder to enforce is rewritten or rejected.
    66	5. **The kernel never grows app nouns.** ADR-0009 doctrine D0 is enforced by review and by the M11 podcast-app proof.
    67	6. **No phase ends silently.** Each milestone exit produces: regression tests added to `nmp-testing`, a perf report in `docs/perf/m<N>/`, an ADR if a design decision was revised, and a runnable artifact tagged in git.
    68	
    69	---
    70	
    71	## 2. The milestone ladder
    72	
    73	Each milestone has: **demo product**, **scope (what gets built)**, **subsystem deliverables**, **exit gate (measurable)**, and **runnable artifact**. Estimates are for one experienced developer focused on the work; they are not commitments.
    74	
    75	### M0 — Kernel substrate + non-Nostr fixture *(DONE)*
    76	
    77	**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.
    78	
    79	**Scope.** Five extension trait families. Composite reverse index. Delta buffer with coalescing. Claim-based GC. Codegen producing a working per-app crate from a fixture module.
    80	
    81	**Subsystem deliverables.** `nmp-core::substrate`, `nmp-codegen`, `fixture-todo-core`, `nmp-testing` harness skeletons.
    82	
    83	**Exit gate.** ✅ Fixture compiles and runs; codegen determinism test passes; substrate registry test passes (`crates/nmp-core/tests/substrate_registry.rs`).
    84	
    85	**Runnable artifact.** `cargo test --workspace`; the fixture module loads in any host.
    86	
    87	---
    88	
    89	### M1 — Read-only Twitter slice on iOS *(LARGELY DONE; final hardening in flight)*
    90	
    91	**Demo product:** `ios/NmpStress` — SwiftUI app pulling live from primal, rendering seed-driven timeline, profile cards, threads, diagnostics screen.
    92	
    93	**Scope.** Per ADR-0006 + ADR-0008 + ADR-0009: kind:0 Profile path end-to-end against a real relay, on iOS, through real FFI. Seed-driven discovery (union of follow lists from pablof7z + fiatjaf + jb55). Refcounted claim/release pattern per ADR-0005 (profile interest commit `23ae829`). Diagnostics surface per ADR-0007.
    94	
    95	**Subsystem deliverables.**
    96	
    97	- ✅ Kernel actor with mailbox-driven relay ingestion (commit `9e9ce04`).
    98	- ✅ Real WebSocket connections via `tungstenite` + `rustls`.
    99	- ✅ Profile / Timeline / Thread view kinds wired through the kernel.
   100	- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
   101	- ✅ iOS bridge (`KernelBridge.swift`, `KernelModel.swift`, content views).
   102	- ✅ Diagnostics screen showing relay state, logical interests, wire subs (ADR-0007).
   103	- 🟡 Firehose-bench `live` scenarios `cold_start` + `profile_thrashing` running against the iOS app's kernel with **measured numbers** documented as the M1 baseline. (Initial reports exist in `docs/perf/ios-demo/` but should be promoted to `docs/perf/m1/` and gated.)
   104	
   105	**Exit gate.**
   106	
   107	- Avatar / name / picture / NIP-05 fields update in place when kind:0 arrives mid-scroll without any spinner gate.
   108	- Mount/unmount of 100 avatar components rapidly produces correct refcount lifecycle (no leaks, claim drops on grace period).
   109	- Primal connection survives a 30-second disconnect via reconnect with no observable data loss in a retried scroll.
   110	- Firehose-bench `live cold_start` against primal: time to first profile rendered ≤ 800 ms p99, time to filled timeline (200 items) ≤ 5 s p99 on developer hardware.
   111	- Firehose-bench `live profile_thrashing` (50/sec mount/unmount over 10 min) against primal: zero subscription leaks; `OpenView`/`CloseView` dispatch rate ≤ 60% of mount rate (grace-period absorption working).
   112	- All reactivity-bench `--standard` gates continue to pass against the real kernel code path, not just the synthetic model.
   113	
   114	**Runnable artifact.** `just run-ios` launches the app on iPhone simulator pulled from real primal. `docs/perf/m1/baseline.md` published with measured numbers.
   115	
   116	---
   117	
   118	### M2 — Subscription compilation + outbox routing
   119	
   120	**Demo product:** Same iOS app as M1, but timeline subscriptions are routed per-author to those authors' write relays per NIP-65, not to the hardcoded primal/purplepag.es pair. Diagnostics screen visibly shows the per-relay fan-out and which authors each relay covers.
   121	
   122	**Scope.** The planner becomes a **subscription compilation stage** per the NDK/Applesauce lessons doc. Logical interests get compiled into per-relay plans; recompilation happens when relay metadata arrives late. NIP-65 routing is the default for both reads and writes. Provenance / NIP-65 / relay hints / user-configured relays are four distinct facts that inform each other but never collapse.
   123	
   124	**Subsystem deliverables.**
   125	
   126	- `nmp-nip65` protocol module: Mailboxes view module (parsed kind:10002); outbox routing as a planner subsystem; recompilation triggers (kind:10002 arrival, view open, relay reconnect).
   127	- Planner refactored from "hardcoded relay set" to "compiler of logical interests → per-relay plans." See `docs/design/ndk-applesauce-lessons.md` §7 (subscription compilation lessons).
   128	- Per-pubkey relay-list cache (durable, even before LMDB lands — keep it in-memory until M3, but the data model is correct).
   129	- Indexer fallback when a pubkey's kind:10002 is unknown: opportunistic discovery from a configurable indexer relay set.
   130	- Reverse-relay-coverage view for diagnostics: "this relay is serving N authors of our timeline."
   131	
   132	**Exit gate.**
   133	
   134	- Bug-extinction test #3 (publish to wrong relays): no public API path lets the developer specify relays for a publish; explicit override action exists and produces a debug warning.
   135	- Subscription compilation correctness: for a timeline of 1000 authors, the compiled plan opens REQs only against the union of those authors' write relays (de-duplicated). Test asserts on the wire frame count.
   136	- Late-arriving kind:10002 triggers recompilation: an author whose mailbox was unknown gets re-routed once their kind:10002 lands, without the platform observing protocol churn.
   137	- Distinct-source visibility: the diagnostics screen shows the four relay-fact lanes (NIP-65 / hint / provenance / user-configured) separately.
   138	
   139	**Runnable artifact.** iOS app with measurably different relay-fan-out behavior; demo screenshot in `docs/perf/m2/outbox-routing.md` showing per-relay coverage.
   140	
   141	---
   142	
   143	### M3 — Persistence (LMDB) + full insert invariants
   144	
   145	**Demo product:** iOS app cold-starts in ≤ 1.5 s with the previous session's events already on screen.
   146	
   147	**Scope.** Swap in-memory `EventStore` for LMDB via `Box<dyn EventStore>`. Implement the full insert invariants from `product-spec.md` §7.1: parameterized replaceable events (kind 30000–39999 by `(pubkey, kind, d-tag)`), kind:5 delete handling with tombstone persistence, NIP-40 expiration scheduling, dedup with provenance merge, claim-based GC running.
   148	
   149	**Subsystem deliverables.**
   150	
   151	- LMDB schema design doc (`docs/design/lmdb-schema.md`) — key encoding, secondary indexes, tombstones, watermarks table (populated in M4), backup/export format.
   152	- `EventStore` trait abstracted; LMDB backend; in-memory backend kept for tests.
   153	- Migration plumbing (ties into `DomainModule::migrations()`).
   154	- GC working set policy per ADR-0003: hot ≤ 10k events resident + claim-pinned set; cold on disk.
   155	
   156	**Exit gate.**
   157	
   158	- Cold-start with primed LMDB: time-to-first-painted-timeline ≤ 1.5 s on iPhone 12.
   159	- Working-set memory under sustained scroll: ≤ 100 MB at 100 active views / 10k hot events / 1 M cached on disk.
   160	- Replaceable correctness across restart: a kind:0 written, app killed, app reopened — the latest version is served, not stale.
   161	- Kind:5 self-delete persists; foreign kind:5 ignored.
   162	
   163	**Runnable artifact.** iOS app surviving termination + relaunch with state preserved. Report in `docs/perf/m3/persistence.md`.
   164	
   165	---
   166	
   167	### M4 — NIP-77 negentropy sync engine
   168	
   169	**Demo product:** Profile screen for a new author cold-syncs via NIP-77 against primal, visibly faster and with measured bytes savings vs REQ scan.
   170	
   171	**Scope.** Per `product-spec.md` §7.8 and ADR (sync as engine, not feature):
   172	
   173	**Subsystem deliverables.**
   174	
   175	- `nmp-nip77` protocol module: negentropy reconciliation client (use `nostr-sdk`'s implementation or `negentropy` crate directly).

exec
/bin/zsh -lc "nl -ba docs/design/lmdb-schema.md | sed -n '1,245p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# Design: LMDB schema + EventStore trait + GC policy (M3)
     2	
     3	> **Audience:** kernel implementers landing M3 (persistence).
     4	> **Status:** rev 0 — proposed; opens ADR slot for any open-question resolution.
     5	> **Companion docs:** [`lmdb/trait.md`](lmdb/trait.md), [`lmdb/keys.md`](lmdb/keys.md), [`lmdb/gc.md`](lmdb/gc.md), [`lmdb/watermarks.md`](lmdb/watermarks.md), [`lmdb/tests.md`](lmdb/tests.md).
     6	> **Prerequisites:** `docs/product-spec/subsystems.md` §7.1 (insert invariants), `docs/decisions/0003-working-set-memory.md` (GC policy intent), `docs/decisions/0009-app-extension-kernel-boundary.md` (DomainModule storage), `docs/design/kernel-substrate.md` §2 (DomainModule trait).
     7	> **Plan reference:** [`docs/plan.md`](../plan.md) §M3.
     8	
     9	---
    10	
    11	## 1. Decision: wrap `nostr-lmdb` behind our own `EventStore` trait
    12	
    13	**Adopt `nostr-lmdb` as the on-disk byte store. Wrap it behind the NMP `EventStore` trait. Add NMP-owned LMDB sub-databases for the rows `nostr-lmdb` does not model.**
    14	
    15	The competing options were (1) use `nostr-lmdb` directly via its concrete `NostrLMDB` type (or via `nostr_database::NostrEventsDatabase`), (2) wrap behind our own trait, or (3) hand-roll an LMDB layer.
    16	
    17	**`nostr-lmdb` gives us** (per `docs.rs/nostr-lmdb`): `save_event(&Event)`, `event_by_id(&EventId)`, `check_id(&EventId) -> DatabaseEventStatus`, `query(Filter) -> Events`, `count(Filter)`, `delete(Filter)`, `wipe()`, `negentropy_items(Filter) -> Vec<(EventId, Timestamp)>`. It owns the page allocator, the LMDB environment, primary by-id store, indexes derived from `Filter`, replaceable / parameterized-replaceable supersession, and NIP-09 delete handling. It is the only mature Rust LMDB store for Nostr events with proven NIP-77 integration; reinventing it is a year of work that we will not recoup.
    18	
    19	**What `nostr-lmdb` does *not* model** (the gap that justifies a wrapper):
    20	
    21	| Concern | Why `nostr-lmdb` doesn't cover it | Where NMP needs it |
    22	|---|---|---|
    23	| Per-relay provenance (which relays delivered each event; first seen / last seen) | Out of scope; the crate models events, not their wire history | `subsystems.md` §7.1 "Duplicate id → merge relay provenance set"; ADR-0007 diagnostics; outbox routing scoring in M2+ |
    24	| Sync watermarks `(filter_hash, relay) → synced_up_to` | Out of scope; the crate does not know about logical filters or relay identity | `subsystems.md` §7.1 + §7.8; M4 NIP-77 engine needs them to be authoritative |
    25	| Claim register / release for view-driven GC | Out of scope; the crate has no concept of an "open view" | ADR-0003; current in-memory analogue in `kernel/mod.rs:315` `profile_claims: HashMap<String, BTreeSet<String>>` |
    26	| Working-set hot/cold split with eviction LRU | LMDB is OS-paged; the crate trusts the kernel page cache | ADR-0003 numeric gate (≤ 100 MB at 100 views / 10k hot) |
    27	| Kernel-side secondary indexes for query shapes our planner uses (e.g. `(p-tag, timestamp)` desc scan, `(e-tag, timestamp)` desc scan, `(expires_at, event_id)` for NIP-40 wakeups) | The `Filter` API recomputes per call; not optimal for our planner's repeat shapes | Planner cache-coverage queries (§7.2); NIP-40 expiration scheduling (§7.1) |
    28	| Tombstone-as-row that survives independent of the deleted event | The crate suppresses re-insert via its own delete index; we want it exposed for export / restoring across re-syncs | `subsystems.md` §7.1 kind:5 row "persisted as tombstone so later re-insertion is suppressed" |
    29	| `DomainModule` rows (drafts, settings, action ledger, projection caches) | Entirely out of scope; the crate stores Nostr `Event` only | ADR-0009 + `kernel-substrate.md` §2 — kernel hosts non-Nostr typed rows |
    30	| Migrations versioned per namespace | Out of scope | `kernel-substrate.md` §2: `DomainModule::migrations() -> Vec<DomainMigration>` |
    31	| `nmp dump` deterministic export | Out of scope | M3 exit criteria; M11 cross-app proof |
    32	
    33	**Therefore.** `EventStore` is a NMP-owned trait, with one production impl `LmdbEventStore` that holds (a) a `NostrLMDB` for the canonical event store and Nostr-shaped queries, and (b) NMP-owned secondary LMDB sub-databases under the same `lmdb::Environment` for the gap rows. The in-memory backend (`MemEventStore`) remains, both for tests and as the web-pre-M15 fallback. See [`lmdb/trait.md`](lmdb/trait.md) for the exact trait shape and the relayed-vs-owned method split.
    34	
    35	**Rejected alternatives.**
    36	
    37	- *Use `NostrLMDB` directly, no wrapper.* Loses every gap row above. Forces the kernel actor to know about LMDB transactions and a non-NMP concrete type, breaking the `Box<dyn EventStore>` substitutability M3 requires.
    38	- *Roll our own.* Reinvents NIP-09 / replaceable handling that `nostr-lmdb` already gets right. ~2,000 LOC of avoidable code with a worse bug surface than upstream.
    39	- *SQLite-backed `nostr-sdk` store.* Larger memory footprint at our 10k-event hot working set; iOS-disk-WAL fsync cost not justified for this access pattern. Held in reserve for the web port (M15) if IndexedDB OPFS proves unworkable.
    40	
    41	## 2. Subsystem ownership map
    42	
    43	```
    44	crates/nmp-core/src/store/
    45	  mod.rs                — trait re-exports + factory
    46	  trait.rs              — `EventStore` (see lmdb/trait.md)
    47	  mem.rs                — in-memory backend (preserved from kernel/mod.rs current state)
    48	  lmdb/
    49	    mod.rs              — `LmdbEventStore` orchestrator
    50	    env.rs              — `lmdb::Environment` + sub-db handles + open()
    51	    events.rs           — wraps `nostr_lmdb::NostrLMDB`; primary-by-id, replaceable rules, kind:5 handling
    52	    secondary.rs        — NMP-owned secondary indexes (see lmdb/keys.md §3)
    53	    provenance.rs       — provenance sub-db (see lmdb/watermarks.md §2)
    54	    watermarks.rs       — watermark sub-db (see lmdb/watermarks.md §1)
    55	    claims.rs           — claim register / release + hot-set hints (see lmdb/gc.md §2)
    56	    gc.rs               — `gc_step()` algorithm (see lmdb/gc.md §3)
    57	    domain.rs           — per-DomainModule sub-db namespacing + migration runner
    58	    dump.rs             — `nmp dump` (see §9 below)
    59	```
    60	
    61	Each file is bounded ≤ 300 LOC per AGENTS.md. The trait module is read by the actor; backend modules are read only by the orchestrator.
    62	
    63	## 3. EventStore trait
    64	
    65	See [`lmdb/trait.md`](lmdb/trait.md) for the exact `pub trait EventStore` signature with all required methods, return types, and the `StoreError` enum. Summary:
    66	
    67	- **Reads:** `get_by_id`, `scan_by_author_kind`, `scan_by_kind_dtag`, `scan_by_etag`, `scan_by_ptag`, `scan_by_kind_time`, `scan_expiring_before`. All `scan_*` methods return a streaming `EventIter` so the planner pages without materialising. Cache-coverage queries take a `WatermarkKey` and answer authoritatively.
    68	- **Writes:** `insert(event, RelayUrl)` returns `InsertOutcome` matching §7.1's table. `delete_by_filter` for foreign-relay cleanups. `tombstones_for` for replay.
    69	- **Watermarks / sync:** `read_watermark`, `write_watermark`, `list_watermarks_for_relay`.
    70	- **GC:** `claim(ClaimerId, &[EventId])`, `release(ClaimerId)`, `hot_set_hint(&[EventId])`, `gc_step(GcBudget) -> GcReport`.
    71	- **Domain rows:** `domain_open(namespace) -> DomainHandle` returns a typed handle; `DomainHandle::put/get/scan_index` is the per-DomainModule API.
    72	- **Migration:** `run_migrations(&[DomainMigration])` runs at startup, transactional per migration.
    73	- **Export:** `dump(out: &mut dyn Write, format: DumpFormat) -> Result<DumpStats>`.
    74	
    75	**Error semantics.** All methods return `Result<T, StoreError>`. Per doctrine D3, store errors do **not** cross FFI — the actor maps every variant to either (a) a typed `Effect` (e.g. `StoreCorrupt`, surfaces via diagnostics + toast), (b) a `tracing::warn!` log + degraded state, or (c) a panic at startup if the LMDB environment refuses to open. The trait itself uses `Result` since it is internal to the actor process.
    76	
    77	## 4. Key encoding
    78	
    79	Full byte layout for primary + every secondary in [`lmdb/keys.md`](lmdb/keys.md). At a glance:
    80	
    81	- Primary `events`: `event_id[32]` → `Event` (CBOR via `nostr` crate's serialization). Owned by `nostr-lmdb`.
    82	- Secondary `idx_author_kind`: `pubkey[32] || kind_be[4] || created_at_be[8] || event_id[32]` → empty. NMP-owned.
    83	- Secondary `idx_kind_dtag`: `kind_be[4] || dtag_len_be[2] || dtag_bytes || pubkey[32]` → `event_id[32]`. NMP-owned. Parameterized replaceable address lookup.
    84	- Secondary `idx_etag_time`, `idx_ptag_time`: `tag_value[32] || created_at_desc_be[8] || event_id[32]` → empty. NMP-owned. `created_at_desc = u64::MAX - created_at` so a forward LMDB scan is newest-first.
    85	- Secondary `idx_kind_time`: `kind_be[4] || created_at_desc_be[8] || event_id[32]` → empty.
    86	- Secondary `idx_expires`: `expires_at_be[8] || event_id[32]` → empty. Scanned by the NIP-40 reaper.
    87	- `tombstones`: `target_id[32]` → `TombstoneRow { kind5_event_id, deleter_pubkey, deleted_at, sources: Vec<RelayUrl> }` (CBOR).
    88	
    89	`created_at_be` is big-endian so byte order matches numeric order; `created_at_desc_be = u64::MAX - created_at` then big-endian for newest-first scans without `MDB_LAST + MDB_PREV`.
    90	
    91	All secondaries are maintained inside the same `RwTxn` as the primary write — atomicity is achieved by LMDB transactionality, not by post-hoc reconciliation.
    92	
    93	## 5. Watermark table
    94	
    95	See [`lmdb/watermarks.md`](lmdb/watermarks.md) for full layout. Row shape (CBOR):
    96	
    97	```rust
    98	struct WatermarkRow {
    99	  filter_hash: [u8; 32],     // BLAKE3 of canonicalised filter (see watermarks.md §3)
   100	  relay_url: String,
   101	  synced_up_to: u64,         // unix seconds
   102	  last_sync_method: SyncMethod, // Negentropy | ReqScan | Manual
   103	  last_negentropy_state: Option<Vec<u8>>, // engine-opaque resume blob
   104	  bytes_saved_vs_req: u64,
   105	  updated_at: u64,
   106	}
   107	```
   108	
   109	Key: `filter_hash[32] || relay_url_bytes` (no length prefix needed — relay URL is the variable suffix; lookup uses exact key). Populated by M4 (NIP-77) and consulted by M2's planner (cache-coverage check before issuing backfill REQ). Survives restarts; loaded into the actor on startup as a `HashMap<(filter_hash, relay_url), WatermarkRow>` for hot lookups, with all writes going through `EventStore` for durability.
   110	
   111	## 6. Migration plumbing
   112	
   113	See [`lmdb/watermarks.md`](lmdb/watermarks.md) §4. A `DomainModule` (per `kernel-substrate.md` §2) declares `const NAMESPACE: &'static str` and `const SCHEMA_VERSION: u32` plus `fn migrations() -> Vec<DomainMigration>`. The store assigns one LMDB sub-database per `(namespace, "data")`, plus one per `(namespace, index_name)` for each declared index. A `_meta` sub-database tracks `(namespace, current_version)`.
   114	
   115	The current `ModuleRegistry` (`crates/nmp-core/src/substrate/mod.rs:41`) discards the concrete `M: DomainModule` type after `register_domain::<M>()` returns — only the `ModuleDescriptor` is retained. The store cannot get from a namespace string back to `M::SCHEMA_VERSION` or `M::migrations()` at runtime. M3 adds a `DomainFactories { schema_version: fn() -> u32, migrations: fn() -> Vec<DomainMigration>, indexes: fn() -> Vec<DomainIndex> }` struct attached per descriptor, populated by capturing the `M::*` consts and fns in `fn`-pointer closures at register time. This matches the existing `key_fn: fn(&[u8]) -> Option<Vec<u8>>` pattern in `DomainIndex` (`crates/nmp-core/src/substrate/domain.rs:18`) — no `Box<dyn DomainModule>` and no new trait object-safety constraints on `DomainModule`. The change is additive to the substrate module surface. See [`lmdb/watermarks.md`](lmdb/watermarks.md) §4.1 for the registry-side code shape.
   116	
   117	On startup:
   118	
   119	1. For every registered `DomainModule`, read its row from `_meta`.
   120	2. If absent, treat current as 0 and run all migrations from 0 to `SCHEMA_VERSION` in one `RwTxn` per step.
   121	3. If present and less than `SCHEMA_VERSION`, run the missing steps.
   122	4. If greater, refuse to start (downgrade not supported); surface as `Effect::DomainSchemaTooNew { namespace }`.
   123	
   124	Each `DomainMigration::apply` receives a `MigrationTx` with put/get/delete + index rebuild helpers. Rollback semantics: each migration step is its own LMDB write transaction; failure aborts the step cleanly. If migration N succeeds and N+1 fails, the store stays at version N — the actor refuses to start the affected module and the rest of the kernel runs in degraded mode (the module's actions return `ActionRejection::ModuleUnavailable`).
   125	
   126	## 7. GC working-set policy
   127	
   128	See [`lmdb/gc.md`](lmdb/gc.md) for the eviction algorithm. Formal statement (matches ADR-0003):
   129	
   130	```
   131	hot_resident = {e | e is in claim_pinned}
   132	             ∪ {e | e is in open_view_cover}
   133	             ∪ {e | e is among the ≤10k most-recently-touched events}
   134	
   135	cold = stored_events \ hot_resident
   136	```
   137	
   138	`hot_resident` lives in a `lru::LruCache<EventId, Arc<Event>>` capped at the configured hot ceiling (default 10,000) plus an unbounded pinned overlay holding events with non-zero claim count. `cold` lives only on disk; lookup pays one LMDB `get` (memory-mapped — typically already in OS page cache for recently-evicted items).
   139	
   140	**Eviction algorithm.** On any insert that pushes the LRU over its ceiling, the oldest non-pinned entry is dropped. `gc_step()` is called periodically by the actor (default every 60 s and on memory pressure callbacks from `MemoryWarningCapability`): it (a) reaps NIP-40 expired events using `idx_expires`, (b) trims the LRU to `target_hot_size`, (c) deletes tombstones older than `tombstone_retention` (default 90 days) whose target event is absent from the store, (d) returns a `GcReport` for diagnostics.
   141	
   142	ADR-0003's numbers are preserved as the M3 exit gate (§11 below): ≤ 100 MB working-set at 100 active views / 10k hot events / 1M cached on disk.
   143	
   144	## 8. Replaceable + tombstone semantics
   145	
   146	The `insert()` path implements exactly the §7.1 invariants:
   147	
   148	- **Replaceable (kinds 0, 3, 10000–19999).** Look up the existing event for `(pubkey, kind)` in `idx_author_kind` (most recent suffix). If incoming `created_at` is newer, replace; if equal, keep lexicographically smallest `id`; else drop. Replacement deletes the old primary row and all secondary entries in the same `RwTxn`.
   149	- **Parameterized replaceable (30000–39999).** Same algorithm keyed on `(pubkey, kind, d-tag)` via `idx_kind_dtag` (which holds `event_id` as value so we don't need a separate `idx_author_kind_dtag`; the dtag prefix is unique per author by Nostr semantics — see [`lmdb/keys.md`](lmdb/keys.md) §3.2 for the per-author scoping note).
   150	- **Kind:5 self-delete.** Verify signature, scan referenced `e` and `a` tags, for each target `e_id` that is authored by the deleter or whose `a` address matches `(deleter_pubkey, kind, d-tag)`: delete the primary + all secondaries + write the tombstone row. Tombstone timestamp = `max(existing.deleted_at, kind5.created_at)`. Re-insert of the deleted event id is suppressed at insert time by a `tombstones.contains(event_id)` check.
   151	- **Foreign kind:5.** A kind:5 referencing events not authored by the kind:5's `pubkey` is ignored (per spec) — the event is *still stored* as a kind:5 (so other clients can render it / dedup it), but it has no side effect on the targets. The tombstone row is **not** written.
   152	- **NIP-40 expiration.** On insert, parse `expiration` tag; if present, write `idx_expires`. On `gc_step()`, scan `idx_expires` for keys with `expires_at_be ≤ now`, delete them like kind:5 (full primary + secondaries + tombstone marker noting `kind: Expired`).
   153	
   154	The tombstone schema is in [`lmdb/keys.md`](lmdb/keys.md) §4.
   155	
   156	## 9. Provenance: per-row sidecar sub-database
   157	
   158	**Decision: separate `provenance` sub-database keyed by `event_id[32]`.** Value is CBOR `ProvenanceRow { sources: Vec<ProvenanceEntry> }` where `ProvenanceEntry = { relay_url, first_seen_ms, last_seen_ms, primary: bool }`.
   159	
   160	Rejected: stuffing provenance into the `Event` row. That requires re-serializing the full `Event` on every relay redelivery (high write amplification — popular events arrive 5–20× from the relay fan-out) and forks the `nostr-lmdb` row format, which we explicitly want to keep upstream-compatible. The sidecar is appended cheaply with a single CBOR re-encode of the (typically small) `sources` vector.
   161	
   162	On duplicate-id insert (§7.1 row 2), `insert()` does not touch the primary; it only updates the provenance sidecar (`last_seen_ms` bump on the matching `ProvenanceEntry`, or append). The "primary relay" — for outbox-routing scoring (M2) and ADR-0007 diagnostics — is deterministically the first relay observed (`sources[0]` after sort by `first_seen_ms`).
   163	
   164	The export format (§ next) includes the provenance row alongside each event so a `nmp dump` round-trip restores it.
   165	
   166	## 10. Backup / export format
   167	
   168	`nmp dump` writes line-delimited JSON to stdout (or a file). Each line is a single tagged record:
   169	
   170	```json
   171	{"type":"event","event": {...nostr event...},"provenance":[{"relay_url":"wss://relay.primal.net","first_seen_ms":1747000000000,"last_seen_ms":1747001234567,"primary":true}]}
   172	{"type":"tombstone","target_id":"abc...","kind5_event_id":"def...","deleter_pubkey":"...","deleted_at":1747000000,"sources":["wss://..."]}
   173	{"type":"watermark","filter_hash":"hex32","relay_url":"wss://...","synced_up_to":1747000000,"last_sync_method":"Negentropy","bytes_saved_vs_req":12345,"updated_at":1747000123}
   174	{"type":"domain","namespace":"twitter.drafts","schema_version":1,"key_hex":"...","value_b64":"..."}
   175	```
   176	
   177	JSONL is the chosen format because (a) it streams (no holding the full dump in memory; cold-events page in as scanned), (b) it diffs cleanly (one record per line), (c) any line is independently parsable for partial recovery, (d) `jq` works out of the box. Binary CBOR is faster but loses ad-hoc inspectability — JSONL is the right tradeoff for an export format.
   178	
   179	`nmp restore` is symmetric: read JSONL, replay through `insert()` for events (so all secondaries are rebuilt from scratch — provenance is restored separately by writing the sidecar row directly after each event), `write_watermark` for watermarks, `DomainHandle::put` for domain rows. Restore is idempotent: replaying the same dump twice produces the same store.
   180	
   181	## 11. Test plan
   182	
   183	See [`lmdb/tests.md`](lmdb/tests.md) for the full mapping of every spec §7.1 invariant to a concrete test file under `crates/nmp-testing/tests/`. Highlights:
   184	
   185	| Invariant (§7.1) | Test file |
   186	|---|---|
   187	| Insert API single path | `crates/nmp-testing/tests/store_insert_path.rs` |
   188	| Signature verification before persist | `crates/nmp-testing/tests/store_invalid_sig.rs` |
   189	| Duplicate id → merge provenance, keep earliest received_at | `crates/nmp-testing/tests/store_provenance_merge.rs` |
   190	| Replaceable supersession | `crates/nmp-testing/tests/store_replaceable.rs` |
   191	| Parameterized replaceable supersession | `crates/nmp-testing/tests/store_param_replaceable.rs` |
   192	| Kind:5 self-delete persists as tombstone | `crates/nmp-testing/tests/store_kind5_tombstone.rs` |
   193	| Foreign kind:5 ignored | `crates/nmp-testing/tests/store_kind5_foreign.rs` |
   194	| NIP-40 expiration scheduled + reaped | `crates/nmp-testing/tests/store_nip40_expiration.rs` |
   195	| Watermarks survive restart, authoritative cache-miss | `crates/nmp-testing/tests/store_watermarks.rs` |
   196	| Claim register / release; GC drops un-claimed cold | `crates/nmp-testing/tests/store_gc_claims.rs` |
   197	| `nmp dump` round-trip is byte-identical for second dump | `crates/nmp-testing/tests/store_dump_roundtrip.rs` |
   198	| Migration v0→v1 success; rollback on N+1 failure | `crates/nmp-testing/tests/store_domain_migration.rs` |
   199	| Domain isolation: module A cannot read module B's sub-db | `crates/nmp-testing/tests/store_domain_isolation.rs` |
   200	| Working-set ≤ 100 MB at 100 views / 10k hot / 1M cached | `crates/nmp-testing/bin/reactivity-bench` (extended scenario) |
   201	
   202	## 12. Performance budget
   203	
   204	| Gate | Budget | Measurement |
   205	|---|---|---|
   206	| Cold-start time-to-first-painted-timeline on iPhone 12 (primed LMDB, last session's events on disk) | ≤ 1.5 s p99 | `firehose-bench live cold_start --device iphone12` |
   207	| Cold-start time-to-first-painted-timeline on simulator | ≤ 800 ms p99 (looser than device because no thermal envelope) | same harness, simulator scenario |
   208	| Working-set memory at 100 active views / 10k hot / 1M on disk | ≤ 100 MB resident | Instruments Allocations + `reactivity-bench` working-set scenario |
   209	| Single `insert()` for an unseen kind:1 with 4 secondaries | ≤ 250 µs p99 on iPhone 12 | criterion bench in `crates/nmp-testing/benches/store_insert.rs` |
   210	| `scan_by_author_kind` returning 200 newest events | ≤ 5 ms p99 | criterion bench in `crates/nmp-testing/benches/store_scan.rs` |
   211	| `gc_step()` work-batch ceiling (single call) | ≤ 50 ms total wall time | bounded by `GcBudget { max_events, max_duration_ms }` |
   212	| `nmp dump` of 1M events | sustained ≥ 50k events/sec on M-series Mac | wall-clock measurement in dump-roundtrip test |
   213	
   214	Each gate is measurable; any miss revises the design via an ADR before M3 is declared complete (per `plan.md` §1.6 "no silent endings").
   215	
   216	## 13. Open questions for ADR after review
   217	
   218	1. **`nostr-lmdb` LMDB environment sharing.** Can we open the same `lmdb::Environment` for both `NostrLMDB`'s sub-databases and our own NMP sub-databases (provenance, watermarks, claims, domain rows)? If yes, we get atomic cross-sub-db transactions for free (a single `RwTxn` covers event + provenance + secondary indexes). If `nostr-lmdb` insists on opening its own `Environment`, we lose that and the insert path needs a two-phase write with crash-recovery logic. Investigate before implementation — may require an upstream PR exposing `Environment` access.
   219	2. **Watermark `filter_hash` canonicalisation.** Two `Filter`s that are semantically identical but field-ordered differently must hash the same. The canonicalisation rule (likely: sort all tag-value arrays, sort kinds, sort authors, lexicographic field order before BLAKE3) needs to be specified once and shared with the planner so cache-coverage lookups hit. Candidate: a single `fn canonical_filter_hash(&Filter) -> [u8; 32]` in `nmp-core::store::watermarks`.
   220	3. **Projection cache durability.** Currently in-memory in the existing kernel (`kernel/mod.rs:293` `profiles: HashMap`). Do we persist projection caches as a `DomainModule` or rebuild from events at cold-start? Rebuild is simpler and avoids cache-staleness bugs but adds startup cost; persistence is faster but requires invalidation logic on kind:0 replacement. Recommended default: rebuild on cold-start, measure, decide whether to add the persistence layer in M3.x or M4.
   221	4. **Domain-module per-record encoding.** CBOR via `serde_cbor` vs serde-json vs bincode. CBOR is upstream-compatible (matches `nostr` crate); bincode is faster but stratifies the format. Default: CBOR for cross-language readability; revisit if benchmarks show >5% insert-time cost.
   222	5. **iOS keychain-stored encryption-at-rest key for LMDB.** Out of scope for M3 (mentioned for M6 keychain work) but the schema must not assume cleartext-on-disk forever; reserve a `meta` row for `encryption_version: u32` so a future migration can wrap pages.
   223	6. **`ModuleRegistry::register_domain` API stability.** Adding `DomainFactories` to `ModuleDescriptor` is a non-breaking additive change to the public substrate API (existing callers using only the generic `register_domain::<M>()` continue to compile), but it commits us to keeping `DomainModule::SCHEMA_VERSION` and `DomainModule::migrations` as compile-time-resolvable items rather than object-safe methods. Confirm this with the substrate maintainer before M3 lands — if `DomainModule` is expected to support runtime composition (e.g., plugin loading), we need option (c): the actor passes the live `&[Box<dyn DomainModule>]` to `EventStore::open` instead. Recommended default: stay with `fn`-pointer factories; revisit if a plugin-loading use case appears.
   224	
   225	## 14. Citations to current code
   226	
   227	- In-memory event store: `crates/nmp-core/src/kernel/mod.rs:294` (`events: HashMap<String, StoredEvent>`), `kernel/mod.rs:46` (`StoredEvent` struct).
   228	- Insert path under wrap: `crates/nmp-core/src/kernel/ingest.rs:166` (`ingest_profile`), `ingest.rs:235` (`ingest_timeline_event`), `ingest.rs:209` (`ingest_relay_list`).
   229	- Replaceable supersession (current scattered logic to be centralised in `EventStore::insert`): `kernel/ingest.rs:166-185` (profile replace by `(pubkey, kind)`), `ingest.rs:218-233` (NIP-65 list replace by `(pubkey, 10002)`).
   230	- Profile claim refcounting (current in-memory analogue of `EventStore::claim/release`): `kernel/mod.rs:315` (`profile_claims: HashMap<String, BTreeSet<String>>`), `kernel/requests.rs:202` (`claim_profile`), `requests.rs:239` (`release_profile`).
   231	- Substrate `DomainModule` trait the store backs: `crates/nmp-core/src/substrate/domain.rs:1` (current shape, lines 1–49).
   232	- Module registry the store consumes at startup: `crates/nmp-core/src/substrate/mod.rs:41` (`ModuleRegistry::register_domain`).
   233	
   234	The M3 implementation deletes none of the existing files outright — the kernel's `events: HashMap` and `profiles: HashMap` are replaced by calls to `Box<dyn EventStore>` held inside the `Kernel` struct, and the per-field tests (`kernel/tests.rs`) shift to the new trait via `MemEventStore`. No public FFI surface changes.

exec
/bin/zsh -lc "nl -ba docs/aim.md | sed -n '210,245p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
   210	│   └── chat-web/
   211	├── justfile
   212	└── flake.nix
   213	```
   214	
   215	The core crate compiles as `cdylib + staticlib + rlib`. Desktop and CLI consumers link the rlib directly (no FFI). iOS links the staticlib via xcframework. Android links the cdylib via cargo-ndk. Web compiles to wasm32-unknown-unknown via the wasm crate. **One source of truth, four delivery paths.**
   216	
   217	---
   218	
   219	## 6. Doctrine — the rules the API must make hard to violate
   220	
   221	These rules are the framework's identity. They derive from the RMP bible and from the protocol-correctness lessons of the libraries we are synthesizing:
   222	
   223	1. **One event store per application.** Singleton enforced at the FFI boundary.
   224	2. **All reads through the store.** No "fetch from relay, return to caller" API exists. Relay results land in the store; callers subscribe to the store.
   225	3. **All writes through actions.** No "build event, sign, publish" sequence the developer assembles manually.
   226	4. **Replaceable-event invariants enforced on insert.** Stale kind-0/3/10002/parameterized-replaceable events are impossible to retain.
   227	5. **Outbox routing automatic.** Manual relay selection is the opt-out, not the default.
   228	6. **Subscriptions auto-group, auto-close, auto-dedup, auto-buffer.** The developer never writes grouping/dedup/cleanup code.
   229	7. **Sessions are state, switching is an action.** No imperative "log out, then log in, then reload" dance.
   230	8. **No errors cross FFI.** All operational failure surfaces as state fields.
   231	9. **No business logic in native code.** Enforced by docs, examples, and an architectural lint where feasible.
   232	10. **Provenance preserved.** Every event in the store remembers which relays delivered it; private events cannot be accidentally republished to public relays.
   233	11. **Capabilities, not callbacks.** Native↔Rust interactions go through bounded, idempotent capability bridges modeled exactly on the RMP bible's pattern.
   234	12. **Snapshots by default, granular updates as optimization.** Start with `AppUpdate::FullState`; add granular `AppUpdate::*` variants only where profiling demands.
   235	
   236	---
   237	
   238	## 7. Open design questions (must resolve before substantive coding)
   239	
   240	1. **State granularity across FFI.** Full-state snapshots are clean but expensive for large stores. Where do we draw the line, and what granular update variants are needed (e.g. `EventAdded`, `ViewChanged { view_id }`, `SessionSwitched`)?
   241	2. **Where do views live?** (a) Materialized in `AppState`, (b) lazy with `ViewHandle` opaque references the UI subscribes to, (c) computed in platform code. Bible rules out (c). Pick between (a) and (b) — leaning (b) for efficiency, but it complicates the FFI surface.
   242	3. **Reactive cross-FFI subscription protocol.** UniFFI gives callback interfaces, not native reactive streams. Swift wants `@Observable`, Kotlin wants `Flow`, JS wants Observables/Promises. Define a single `Subscription` opaque handle + reconciler-style callback that adapts cleanly per platform.
   243	4. **NIP-46 bunker as a capability bridge.** Long-lived, stateful, involves user approval on another device. Needs careful design as an RMP-style capability bridge.
   244	5. **Background notification decryption.** iOS Notification Service Extensions and Android background workers must call into the Rust core for NIP-17 decryption without spinning up the full actor. Likely a smaller "decrypt-only" surface area in a sibling crate.
   245	6. **Frozen offline action queue.** Actions dispatched while offline must persist and replay on reconnect, with correct ordering and timestamping. Where does the queue live — in the actor, in SQLite, in a separate durable channel?

codex
A few likely consistency issues are emerging: key-layout drift between the master doc and `keys.md`, trait methods named in summaries but absent from the actual trait, and a possible D2/working-set leak around the “open view cover” being outside the trait contract. I’m checking those against the exact sub-doc lines now.
exec
/bin/zsh -lc "nl -ba docs/design/lmdb/trait.md | sed -n '1,340p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# LMDB sub-design: `EventStore` trait
     2	
     3	> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). This file fixes the trait surface; the master doc fixes the decision.
     4	
     5	## 1. Crate placement
     6	
     7	`crates/nmp-core/src/store/events.rs` (filename note: `trait` is a Rust keyword, so the file is named `events.rs` and exposes `pub trait EventStore`). Re-exported from `nmp_core::store::EventStore`. The actor (`crates/nmp-core/src/actor.rs`) holds the store as `store: Box<dyn EventStore>`; backends are constructed by the factory in `store/mod.rs::open_event_store(&AppConfig) -> Result<Box<dyn EventStore>, StoreError>`.
     8	
     9	## 2. Supporting types
    10	
    11	```rust
    12	use std::sync::Arc;
    13	
    14	pub type EventId = [u8; 32];
    15	pub type PubKey = [u8; 32];
    16	pub type RelayUrl = String;
    17	
    18	#[derive(Clone, Debug)]
    19	pub struct StoredEvent {
    20	    pub raw: Arc<nostr::Event>,         // upstream nostr crate type
    21	    pub received_at_ms: u64,            // wall-clock first arrival across all relays
    22	}
    23	
    24	#[derive(Clone, Debug)]
    25	pub struct ProvenanceEntry {
    26	    pub relay_url: RelayUrl,
    27	    pub first_seen_ms: u64,
    28	    pub last_seen_ms: u64,
    29	    pub primary: bool,                  // first observed relay (deterministic)
    30	}
    31	
    32	#[derive(Clone, Debug)]
    33	pub enum InsertOutcome {
    34	    /// Fresh insert; secondary indexes written.
    35	    Inserted { id: EventId, sources_after: u32 },
    36	    /// Duplicate id; provenance updated, primary untouched.
    37	    Duplicate { id: EventId, sources_after: u32 },
    38	    /// Replaceable supersession: this event replaced an older one.
    39	    Replaced { new_id: EventId, replaced_id: EventId },
    40	    /// Replaceable supersession: incoming was older, dropped.
    41	    Superseded { id: EventId, current_id: EventId },
    42	    /// Suppressed because target is tombstoned.
    43	    Tombstoned { id: EventId, target_kind5_id: EventId },
    44	    /// Signature / delegation / structural validity failed.
    45	    Rejected { id: EventId, reason: RejectReason },
    46	    /// Ephemeral kind: delivered to live consumers, not stored.
    47	    Ephemeral { id: EventId },
    48	}
    49	
    50	#[derive(Clone, Debug)]
    51	pub enum RejectReason {
    52	    BadSignature,
    53	    BadDelegation(String),
    54	    Malformed(String),
    55	    ExpiredOnArrival,                   // NIP-40 expiration already in the past
    56	}
    57	
    58	#[derive(Clone, Debug)]
    59	pub struct TombstoneRow {
    60	    pub target_id: EventId,
    61	    pub kind5_event_id: Option<EventId>, // None for NIP-40 expiry tombstones
    62	    pub deleter_pubkey: Option<PubKey>,
    63	    pub deleted_at: u64,                 // unix seconds
    64	    pub sources: Vec<RelayUrl>,
    65	    pub origin: TombstoneOrigin,
    66	}
    67	
    68	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
    69	pub enum TombstoneOrigin { Kind5, NIP40Expiry, AdminPurge }
    70	
    71	#[derive(Clone, Debug)]
    72	pub struct WatermarkKey {
    73	    pub filter_hash: [u8; 32],
    74	    pub relay_url: RelayUrl,
    75	}
    76	
    77	#[derive(Clone, Debug)]
    78	pub struct WatermarkRow {
    79	    pub key: WatermarkKey,
    80	    pub synced_up_to: u64,               // unix seconds
    81	    pub last_sync_method: SyncMethod,
    82	    pub last_negentropy_state: Option<Vec<u8>>,
    83	    pub bytes_saved_vs_req: u64,
    84	    pub updated_at: u64,
    85	}
    86	
    87	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
    88	pub enum SyncMethod { Negentropy, ReqScan, Manual }
    89	
    90	#[derive(Clone, Copy, Debug)]
    91	pub struct ClaimerId(pub u64);           // opaque view-handle id from the actor
    92	
    93	#[derive(Clone, Copy, Debug)]
    94	pub struct GcBudget {
    95	    pub max_events_per_step: usize,
    96	    pub max_duration_ms: u32,
    97	}
    98	
    99	#[derive(Clone, Debug, Default)]
   100	pub struct GcReport {
   101	    pub expired_reaped: usize,
   102	    pub lru_evicted: usize,
   103	    pub tombstones_purged: usize,
   104	    pub duration_ms: u32,
   105	}
   106	
   107	#[derive(Clone, Copy, Debug)]
   108	pub enum DumpFormat { Jsonl, Cbor }
   109	
   110	#[derive(Clone, Debug, Default)]
   111	pub struct DumpStats {
   112	    pub events: u64,
   113	    pub tombstones: u64,
   114	    pub watermarks: u64,
   115	    pub domain_rows: u64,
   116	    pub bytes_written: u64,
   117	}
   118	
   119	#[derive(Debug, thiserror::Error)]
   120	pub enum StoreError {
   121	    #[error("backend i/o: {0}")] Io(String),
   122	    #[error("backend corruption: {0}")] Corrupt(String),
   123	    #[error("encoding: {0}")] Encoding(String),
   124	    #[error("schema too new: {namespace} on-disk={on_disk} expected={expected}")]
   125	    SchemaTooNew { namespace: String, on_disk: u32, expected: u32 },
   126	    #[error("schema migration failed: {namespace} v{from}->v{to}: {reason}")]
   127	    MigrationFailed { namespace: String, from: u32, to: u32, reason: String },
   128	    #[error("unknown namespace: {0}")] UnknownNamespace(String),
   129	}
   130	```
   131	
   132	The store iterates lazily for scans:
   133	
   134	```rust
   135	pub trait EventIter: Iterator<Item = Result<StoredEvent, StoreError>> + Send {}
   136	impl<T: Iterator<Item = Result<StoredEvent, StoreError>> + Send> EventIter for T {}
   137	```
   138	
   139	`StoredEvent::raw` is `Arc<nostr::Event>` so the hot LRU can hold reference-counted copies without cloning the event body on each `get_by_id`.
   140	
   141	## 3. The trait
   142	
   143	```rust
   144	pub trait EventStore: Send + Sync {
   145	    // ─────── Reads ───────
   146	
   147	    /// Primary lookup. Returns Ok(None) if absent; tombstones do not count as "present".
   148	    fn get_by_id(&self, id: &EventId) -> Result<Option<StoredEvent>, StoreError>;
   149	
   150	    /// `idx_author_kind` scan, newest-first. `kinds` empty = any kind.
   151	    fn scan_by_author_kind<'a>(
   152	        &'a self,
   153	        author: &PubKey,
   154	        kinds: &[u32],
   155	        since: Option<u64>,
   156	        until: Option<u64>,
   157	        limit: usize,
   158	    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
   159	
   160	    /// `idx_kind_dtag` lookup. Returns the current authoritative parameterized
   161	    /// replaceable for `(pubkey, kind, d_tag)`, or Ok(None).
   162	    fn get_param_replaceable(
   163	        &self,
   164	        pubkey: &PubKey,
   165	        kind: u32,
   166	        d_tag: &[u8],
   167	    ) -> Result<Option<StoredEvent>, StoreError>;
   168	
   169	    /// `idx_etag_time` scan, newest-first. Used by reaction / repost / thread views.
   170	    fn scan_by_etag<'a>(
   171	        &'a self,
   172	        target: &EventId,
   173	        kinds: &[u32],
   174	        limit: usize,
   175	    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
   176	
   177	    /// `idx_ptag_time` scan, newest-first. Used by notifications / mention views.
   178	    fn scan_by_ptag<'a>(
   179	        &'a self,
   180	        target: &PubKey,
   181	        kinds: &[u32],
   182	        limit: usize,
   183	    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
   184	
   185	    /// `idx_kind_time` scan, newest-first. Used by timeline backfills.
   186	    /// `kinds` empty = any kind (parity with `scan_by_author_kind`).
   187	    fn scan_by_kind_time<'a>(
   188	        &'a self,
   189	        kinds: &[u32],
   190	        since: Option<u64>,
   191	        until: Option<u64>,
   192	        limit: usize,
   193	    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
   194	
   195	    /// `idx_expires` scan, ascending — used by the NIP-40 reaper.
   196	    fn scan_expiring_before<'a>(
   197	        &'a self,
   198	        unix_seconds: u64,
   199	        limit: usize,
   200	    ) -> Result<Box<dyn EventIter + 'a>, StoreError>;
   201	
   202	    /// Tombstones referencing a target id (typically one row).
   203	    fn tombstones_for(&self, target: &EventId) -> Result<Vec<TombstoneRow>, StoreError>;
   204	
   205	    /// Iterate all tombstones (used by `nmp dump`).
   206	    fn list_tombstones<'a>(&'a self)
   207	        -> Result<Box<dyn Iterator<Item = Result<TombstoneRow, StoreError>> + Send + 'a>, StoreError>;
   208	
   209	    /// Provenance sidecar for an event.
   210	    fn provenance_for(&self, id: &EventId) -> Result<Vec<ProvenanceEntry>, StoreError>;
   211	
   212	    // ─────── Writes ───────
   213	
   214	    /// The single insert path. `source` is the relay that delivered this copy.
   215	    /// Verifies signature/delegation, applies §7.1 invariants, updates secondaries
   216	    /// + provenance + tombstones atomically. Returns InsertOutcome per §7.1.
   217	    fn insert(&self, event: nostr::Event, source: &RelayUrl, received_at_ms: u64)
   218	        -> Result<InsertOutcome, StoreError>;
   219	
   220	    /// Delete by a NMP-internal filter — for admin / GC / kind:5 application.
   221	    /// Returns the number of primary rows removed.
   222	    fn delete_by_filter(&self, filter: DeleteFilter) -> Result<usize, StoreError>;
   223	
   224	    // ─────── Watermarks ───────
   225	
   226	    fn read_watermark(&self, key: &WatermarkKey) -> Result<Option<WatermarkRow>, StoreError>;
   227	    fn write_watermark(&self, row: WatermarkRow) -> Result<(), StoreError>;
   228	    fn list_watermarks_for_relay<'a>(
   229	        &'a self,
   230	        relay_url: &str,
   231	    ) -> Result<Box<dyn Iterator<Item = Result<WatermarkRow, StoreError>> + Send + 'a>, StoreError>;
   232	
   233	    // ─────── Hot-set / claims (GC) ───────
   234	
   235	    /// Register a claim: caller pins `ids` against eviction until `release`.
   236	    fn claim(&self, claimer: ClaimerId, ids: &[EventId]) -> Result<(), StoreError>;
   237	    fn release(&self, claimer: ClaimerId) -> Result<(), StoreError>;
   238	
   239	    /// Soft hint: keep these in hot LRU on a best-effort basis.
   240	    fn hot_set_hint(&self, ids: &[EventId]) -> Result<(), StoreError>;
   241	
   242	    /// One bounded GC pass — reap expired, trim LRU, purge old tombstones.
   243	    fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError>;
   244	
   245	    // ─────── Domain rows (per-DomainModule typed namespace) ───────
   246	
   247	    fn domain_open(&self, namespace: &'static str) -> Result<DomainHandle<'_>, StoreError>;
   248	    fn run_migrations(&self, namespace: &'static str, target_version: u32,
   249	                      migrations: &[crate::substrate::DomainMigration])
   250	        -> Result<(), StoreError>;
   251	
   252	    // ─────── Export ───────
   253	
   254	    fn dump(&self, out: &mut dyn std::io::Write, format: DumpFormat)
   255	        -> Result<DumpStats, StoreError>;
   256	}
   257	```
   258	
   259	`DeleteFilter` mirrors the limited subset of admin operations the kernel needs (by-relay-only events, by-author, by-id-list, by-kind range); it is **not** a pass-through to `nostr::Filter` — we intentionally do not expose arbitrary remote filters as a delete vector.
   260	
   261	## 4. `DomainHandle`
   262	
   263	```rust
   264	pub struct DomainHandle<'env> {
   265	    pub(crate) namespace: &'static str,
   266	    pub(crate) inner: DomainHandleInner<'env>,  // backend-specific
   267	}
   268	
   269	impl<'env> DomainHandle<'env> {
   270	    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError>;
   271	    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;
   272	    pub fn delete(&self, key: &[u8]) -> Result<bool, StoreError>;
   273	    pub fn scan_prefix<'a>(&'a self, prefix: &[u8])
   274	        -> Result<Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>, StoreError>;
   275	    pub fn scan_index<'a>(&'a self, index: &'static str, key_prefix: &[u8])
   276	        -> Result<Box<dyn Iterator<Item = Result<(Vec<u8>, Vec<u8>), StoreError>> + 'a>, StoreError>;
   277	}
   278	```
   279	
   280	A handle is module-scoped; the kernel does not give a `DraftsModule` handle to `SettingsModule` (per `kernel-substrate.md` §8 "Domain stores are isolated"). The handle is `'env`-bounded so it cannot outlive the LMDB environment.
   281	
   282	## 5. Error semantics (doctrine D3)
   283	
   284	The trait returns `Result<T, StoreError>`. The actor's wrapper functions map them as:
   285	
   286	- `Io / Corrupt` at startup → panic (we cannot run without a store; surfaces to platform shell as a process restart).
   287	- `Io / Corrupt` mid-run → `Effect::StoreDegraded { details }` published on the diagnostics bridge (ADR-0007); the affected operation returns the closest-fit graceful default (empty iterator, drop-write); the next gc_step retries.
   288	- `Encoding` → `tracing::error!` with the offending key/namespace; the action that triggered it fails with a `toast: Some("internal storage error; please restart")` per D3.
   289	- `SchemaTooNew` at startup → publish `Effect::DomainSchemaTooNew { namespace }`, the affected module starts in degraded mode (its actions reject with `ActionRejection::ModuleUnavailable`), rest of the kernel runs.
   290	- `MigrationFailed` → same as above, plus a one-time toast on first action attempt.
   291	- `UnknownNamespace` → programming error; assert in debug, log + drop in release.
   292	
   293	No `StoreError` ever crosses FFI. The `AppUpdate` carries only successful state + optional `toast: Option<String>`.
   294	
   295	## 6. Two backends in v1
   296	
   297	```rust
   298	// In-memory backend, kept for tests + web-pre-M15.
   299	pub struct MemEventStore { /* HashMap-backed; preserves the current kernel state */ }
   300	
   301	// Production backend on iOS / Android / Desktop.
   302	pub struct LmdbEventStore { /* wraps nostr_lmdb::NostrLMDB + NMP sub-dbs */ }
   303	
   304	pub fn open_event_store(cfg: &AppConfig) -> Result<Box<dyn EventStore>, StoreError> {
   305	    match cfg.storage_backend {
   306	        StorageBackend::Memory => Ok(Box::new(MemEventStore::new())),
   307	        StorageBackend::Lmdb { ref path } => Ok(Box::new(LmdbEventStore::open(path)?)),
   308	    }
   309	}
   310	```
   311	
   312	`MemEventStore` implements every method using `HashMap` / `BTreeMap`. The same test suite runs against both backends with `#[cfg(feature = "lmdb-backend")]` gating only the LMDB-specific edge tests (corruption recovery, oversized values).

exec
/bin/zsh -lc "nl -ba docs/design/lmdb/keys.md | sed -n '1,180p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# LMDB sub-design: key encoding
     2	
     3	> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Fixes the byte layout of every sub-database the NMP store opens. Primary event storage is owned by `nostr-lmdb`; everything below is NMP-owned and lives in NMP-managed sub-databases under the same `lmdb::Environment` (per open question 1 in the master doc).
     4	
     5	## 1. LMDB environment layout
     6	
     7	One `lmdb::Environment` per app data directory. Sub-databases:
     8	
     9	| Sub-db | Owner | Key shape | Value | Notes |
    10	|---|---|---|---|---|
    11	| (multiple) | `nostr-lmdb` | upstream | upstream | event primary, internal filter indexes, kind:5 suppression |
    12	| `idx_author_kind` | NMP | `pubkey[32] ‖ kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` | empty | newest-first scans for `(author, kinds[])` |
    13	| `idx_kind_dtag` | NMP | `kind_be[4] ‖ pubkey[32] ‖ dtag_len_be[2] ‖ dtag_bytes` | `event_id[32]` | parameterized replaceable address lookup |
    14	| `idx_etag_time` | NMP | `target_event_id[32] ‖ created_at_desc_be[8] ‖ event_id[32]` | `kind_be[4]` | reaction/reply/thread view scans |
    15	| `idx_ptag_time` | NMP | `target_pubkey[32] ‖ created_at_desc_be[8] ‖ event_id[32]` | `kind_be[4]` | mentions / notifications |
    16	| `idx_kind_time` | NMP | `kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` | empty | global-by-kind backfills |
    17	| `idx_expires` | NMP | `expires_at_be[8] ‖ event_id[32]` | empty | NIP-40 reaper |
    18	| `tombstones` | NMP | `target_event_id[32]` | CBOR `TombstoneRow` | persists past delete |
    19	| `provenance` | NMP | `event_id[32]` | CBOR `ProvenanceRow` | per-relay sidecar (master doc §9) |
    20	| `watermarks` | NMP | `filter_hash[32] ‖ relay_url_bytes` | CBOR `WatermarkRow` | M4 NIP-77 sync state |
    21	| `claims_meta` | NMP | `claimer_id_be[8]` | CBOR `Vec<EventId>` | pinned set per ClaimerId; rebuilt on restart from open views |
    22	| `domain_<ns>_data` | NMP, per `DomainModule` | module-defined | module-defined | one sub-db per registered namespace |
    23	| `domain_<ns>_idx_<name>` | NMP, per `DomainModule` index | `index_key ‖ primary_key` | empty | secondary indexes per `DomainIndex` |
    24	| `_meta` | NMP | string namespace | `{ schema_version: u32, opened_with_nmp_version: String }` | migration tracking |
    25	
    26	Sub-databases are opened lazily on first access and cached on the `LmdbEventStore`.
    27	
    28	## 2. Endian + ordering conventions
    29	
    30	- All integers in keys are **big-endian** so LMDB's byte-wise comparator matches numeric order.
    31	- `created_at_desc_be = (u64::MAX - created_at).to_be_bytes()` so a forward scan returns newest-first without `MDB_PREV` gymnastics.
    32	- All pubkeys / event ids are fixed-width 32 bytes; the `nostr` crate's `EventId` and `PublicKey` give us byte arrays directly.
    33	
    34	## 3. Secondary index details
    35	
    36	### 3.1 `idx_author_kind`
    37	
    38	Key: `pubkey[32] ‖ kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` → empty value.
    39	
    40	Scan recipes:
    41	
    42	- *Newest N events by author* — `range(pubkey ‖ 0u32_be ‖ ..)` (kind=0 lower bound) up to `pubkey ‖ u32::MAX_be ‖ ..`, take N.
    43	- *Newest N events by `(author, kind=1)`* — `range(pubkey ‖ 1u32_be ‖ ..)` up to `pubkey ‖ 1u32_be ‖ u64::MAX_be`, take N.
    44	- *All kind:0 for author* — `range(pubkey ‖ 0u32_be ‖ ..)`, take 1 (because the replaceable index ensures only one).
    45	
    46	Replaceable supersession (§7.1): on insert of a new kind in [0, 3, 10000–19999], find existing row via this index with `(pubkey, kind)` prefix, compare `created_at`, if incoming wins delete old + write new. Both deletes happen in the same `RwTxn` as the new write so there is no half-state visible to readers.
    47	
    48	### 3.2 `idx_kind_dtag` (parameterized replaceable)
    49	
    50	Key: `kind_be[4] ‖ pubkey[32] ‖ dtag_len_be[2] ‖ dtag_bytes` → `event_id[32]`.
    51	
    52	The d-tag bytes go last so two events with the same `(kind, pubkey)` but different `d` tags don't collide; the explicit length prefix avoids `d="foo"` vs `d="foob"` aliasing under prefix scans. Lookup is exact-key: `get_param_replaceable(pubkey, kind, d_tag)` builds the key and reads.
    53	
    54	The value is the `event_id`; the primary event itself lives in the `nostr-lmdb` events sub-db. On supersession, the old event-id is fetched from this row, both primary and old `idx_*` rows are deleted, and the value is overwritten with the new id.
    55	
    56	### 3.3 `idx_etag_time` and `idx_ptag_time`
    57	
    58	Key: `target[32] ‖ created_at_desc_be[8] ‖ event_id[32]` → `kind_be[4]`.
    59	
    60	The value holds the kind so a reactions view can filter `(kinds == 7)` during scan without a primary-row fetch per candidate. Bookmark / repost / thread views similarly avoid the `get_by_id` round trip until they need the body.
    61	
    62	On insert, the kernel walks the event's `tags`: every `e` tag value goes into `idx_etag_time` and every `p` tag value goes into `idx_ptag_time`. Tag values must be 32-byte hex (validated at insert time); non-conformant tags are silently skipped from indexing (they are still stored in the event body).
    63	
    64	### 3.4 `idx_kind_time`
    65	
    66	Key: `kind_be[4] ‖ created_at_desc_be[8] ‖ event_id[32]` → empty.
    67	
    68	Used by *global-by-kind* backfills (e.g. "recent kind:0 across all authors" during diagnostics). Heavy index — populated for **all** kinds by default but the implementation may skip kinds in a configurable deny-list to keep write amplification down (default deny-list: kind:1 if config flag `index_kind1_globally=false`, which it is by default; M2's planner does not need a global kind:1 scan).
    69	
    70	### 3.5 `idx_expires`
    71	
    72	Key: `expires_at_be[8] ‖ event_id[32]` → empty.
    73	
    74	Populated **only** for events that have an `expiration` tag at insert (NIP-40). `gc_step()` opens a read cursor at `expires_at = 0`, walks forward up to the configured budget, and reaps any keys whose `expires_at ≤ now_unix_seconds()`. Each reaped event triggers a tombstone-of-origin `NIP40Expiry` write so re-insertions (from a re-sync) don't resurrect it.
    75	
    76	## 4. Tombstones
    77	
    78	Key: `target_event_id[32]` → CBOR `TombstoneRow`:
    79	
    80	```rust
    81	#[derive(Serialize, Deserialize)]
    82	struct TombstoneRow {
    83	    target_id: [u8; 32],
    84	    origin: TombstoneOrigin,             // Kind5 | NIP40Expiry | AdminPurge
    85	    kind5_event_id: Option<[u8; 32]>,    // None for non-Kind5 origins
    86	    deleter_pubkey: Option<[u8; 32]>,    // None for NIP40Expiry / AdminPurge
    87	    deleted_at: u64,                     // max observed across kind:5 redeliveries
    88	    sources: Vec<String>,                // relay urls that delivered the kind:5
    89	}
    90	```
    91	
    92	Insert pre-check: before any new event hits the primary store, `tombstones.contains_key(event.id)` is consulted. A hit yields `InsertOutcome::Tombstoned { target_kind5_id }` and the event is dropped. This is the "later re-insertion is suppressed" behavior of §7.1.
    93	
    94	Foreign kind:5 (where the kind:5 author did not author all targets) is **stored** as an ordinary event (so other clients can render the delete intent) but **does not** write a `TombstoneRow` for any of its targets — per §7.1 "foreign kind:5 ignored". The kind:5 event itself goes through the normal insert path including secondaries.
    95	
    96	## 5. Watermarks
    97	
    98	Key: `filter_hash[32] ‖ relay_url_bytes` — variable-length, exact-key lookups only. `filter_hash` is BLAKE3 of the canonical filter encoding (see `lmdb/watermarks.md` §3 for the canonicalisation algorithm).
    99	
   100	Value: CBOR `WatermarkRow` (same shape as the trait type in [`trait.md`](trait.md) §2).
   101	
   102	## 6. Provenance
   103	
   104	Key: `event_id[32]` → CBOR `ProvenanceRow { sources: Vec<ProvenanceEntry> }`. On duplicate insert: read, mutate (append or bump `last_seen_ms`), write back. Bounded growth — the kernel caps `sources.len()` at 32 (the 33rd unique relay overwrites the oldest non-primary entry); for nearly all events this is non-binding. The `primary: bool` flag is deterministic: `sources[0]` after sorting by `(first_seen_ms, relay_url)`.
   105	
   106	## 7. Domain rows (per `DomainModule`)
   107	
   108	For each `DomainModule` with namespace `"foo.bar"`:
   109	
   110	- `domain_foo.bar_data` — primary data sub-db. Module owns key + value encoding.
   111	- `domain_foo.bar_idx_<index>` — one sub-db per `DomainIndex` (per `crates/nmp-core/src/substrate/domain.rs:16`). Key = `index_key_fn(data_value) ‖ primary_key`; value = empty. The index is rewritten on every put (delete-old, write-new).
   112	
   113	The actor exposes them only via `DomainHandle` (see [`trait.md`](trait.md) §4); modules never see the sub-db handles directly. Module isolation per `kernel-substrate.md` §8 is preserved: the handle factory checks the caller's registered namespace.
   114	
   115	## 8. `_meta` sub-database
   116	
   117	Key: namespace string (e.g. `"twitter.drafts"`, `"_kernel"`). Value: CBOR `{ schema_version: u32, opened_with_nmp_version: String, last_migration_at_ms: u64 }`. Read at startup by the migration runner; written after every successful migration step.
   118	
   119	The reserved `_kernel` namespace tracks the LMDB store's own schema version (currently 1). A bumped `_kernel` version triggers store-wide migrations (e.g. re-encoding all `ProvenanceRow` values when the format changes).
   120	
   121	## 9. Worked example: inserting a kind:1 from `pablof7z` arriving from `wss://relay.primal.net`
   122	
   123	```
   124	event_id   = a3f1...   (32 bytes)
   125	pubkey     = 0461...   (32 bytes)
   126	kind       = 1
   127	created_at = 1747000000
   128	tags       = [["e","b21c...","","root"], ["p","0488..."]]
   129	```
   130	
   131	Inside one `RwTxn`:
   132	
   133	1. `tombstones.get(&event_id)` → None ⇒ proceed.
   134	2. `nostr_lmdb.save_event(&event)` → SaveEventStatus::Success.
   135	3. `idx_author_kind.put(0461... ‖ 0x00000001 ‖ desc(1747000000) ‖ a3f1..., &[])`.
   136	4. `idx_kind_time.put(0x00000001 ‖ desc(1747000000) ‖ a3f1..., &[])` (only if `index_kind1_globally`; default off).
   137	5. For `e:b21c...` → `idx_etag_time.put(b21c... ‖ desc(1747000000) ‖ a3f1..., 1u32_be)`.
   138	6. For `p:0488...` → `idx_ptag_time.put(0488... ‖ desc(1747000000) ‖ a3f1..., 1u32_be)`.
   139	7. `provenance.put(a3f1..., cbor({sources:[{relay:"wss://relay.primal.net", first_seen_ms:T, last_seen_ms:T, primary:true}]}))`.
   140	
   141	Total LMDB writes: 1 primary (delegated to upstream) + 3 NMP secondaries + 1 provenance = ~5 page writes for a typical kind:1. Within the 250 µs p99 budget (master doc §12) on iPhone 12 NAND.
   142	
   143	A second arrival of the same id from `wss://nos.lol`:
   144	
   145	1. `tombstones.get(&a3f1...)` → None.
   146	2. `nostr_lmdb.save_event` → SaveEventStatus::Duplicate (we don't re-process).
   147	3. Skip steps 3–6 (secondaries unchanged).
   148	4. `provenance.get(a3f1...)` → existing row; append `{relay:"wss://nos.lol", first_seen_ms:T2, last_seen_ms:T2, primary:false}`; put back.
   149	
   150	One read + one write. Returns `InsertOutcome::Duplicate { sources_after: 2 }`.

exec
/bin/zsh -lc "nl -ba docs/design/lmdb/gc.md | sed -n '1,230p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# LMDB sub-design: GC working-set policy
     2	
     3	> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Formalises the hot-resident / cold-on-disk split required by ADR-0003 (`docs/decisions/0003-working-set-memory.md`).
     4	
     5	## 1. Definitions
     6	
     7	```
     8	stored_events = every event currently in `events` (primary), not tombstoned
     9	
    10	claim_pinned  = ⋃ { ids | ids ∈ claims[claimer] for each registered claimer }
    11	                where each `claimer` is an open ViewHandle / open ActionHandle
    12	
    13	open_view_cover = ⋃ { dependency_target_ids(spec)
    14	                       | (view_id, spec) ∈ open_views }
    15	                  computed from the composite reverse-index per ADR-0001
    16	
    17	recently_touched = top-N by `last_touched_ms` (default N = 10,000)
    18	
    19	hot_resident = claim_pinned ∪ open_view_cover ∪ recently_touched
    20	cold         = stored_events \ hot_resident
    21	```
    22	
    23	`last_touched_ms` is bumped on every `get_by_id`, on every secondary scan that *materialises* the event body, and on `insert` for a fresh row. Scans that only return ids/timestamps (e.g., the early-filter pass in a view's planner) do **not** bump it — only the construction of a `Delta` payload that needs the body does.
    24	
    25	`hot_resident` is stored in memory; `cold` lives only on disk. The store still **knows** about every cold event via secondaries — the reverse index covers both per ADR-0003: "The reverse index indexes both hot and cold events. Lookup returns view ids immediately; event bodies for delta construction load lazily and synchronously via the storage backend."
    26	
    27	## 2. Hot data structure
    28	
    29	```rust
    30	pub(crate) struct HotSet {
    31	    // LRU bounded by `target_hot_size` (default 10,000), evicts non-pinned.
    32	    lru: lru::LruCache<EventId, Arc<nostr::Event>>,
    33	    // Strong-pin overlay; refcounted by ClaimerId.
    34	    pinned: HashMap<EventId, u32>,                   // event_id → refcount
    35	    // Reverse map for cheap release().
    36	    by_claimer: HashMap<ClaimerId, SmallVec<[EventId; 8]>>,
    37	    target_hot_size: usize,
    38	}
    39	
    40	impl HotSet {
    41	    pub fn claim(&mut self, c: ClaimerId, ids: &[EventId]) {
    42	        for id in ids {
    43	            *self.pinned.entry(*id).or_insert(0) += 1;
    44	        }
    45	        self.by_claimer.entry(c).or_default().extend_from_slice(ids);
    46	    }
    47	
    48	    pub fn release(&mut self, c: ClaimerId) {
    49	        if let Some(ids) = self.by_claimer.remove(&c) {
    50	            for id in ids {
    51	                if let Some(rc) = self.pinned.get_mut(&id) {
    52	                    *rc = rc.saturating_sub(1);
    53	                    if *rc == 0 { self.pinned.remove(&id); }
    54	                }
    55	            }
    56	        }
    57	    }
    58	
    59	    pub fn touch(&mut self, id: EventId, e: Arc<nostr::Event>) {
    60	        self.lru.put(id, e);                          // bumps LRU
    61	        self.trim();
    62	    }
    63	
    64	    fn trim(&mut self) {
    65	        while self.lru.len() > self.target_hot_size {
    66	            // pop_lru returns oldest; skip pinned ones until we find an evictable.
    67	            // (LruCache::pop_lru doesn't take a predicate; we rotate.)
    68	            let mut skipped: SmallVec<[(EventId, Arc<nostr::Event>); 8]> = SmallVec::new();
    69	            let evicted = loop {
    70	                match self.lru.pop_lru() {
    71	                    Some((id, e)) if self.pinned.contains_key(&id) => skipped.push((id, e)),
    72	                    Some(pair) => break Some(pair),
    73	                    None => break None,
    74	                }
    75	            };
    76	            for (id, e) in skipped.drain(..) { self.lru.put(id, e); }
    77	            if evicted.is_none() { break; }           // every entry is pinned
    78	        }
    79	    }
    80	}
    81	```
    82	
    83	`target_hot_size` is set from `AppConfig::hot_event_ceiling` (default 10,000) and may be lowered by `MemoryWarningCapability` events (iOS app suspend or low-memory warning → halve the ceiling, run `gc_step()` once, restore after the warning clears).
    84	
    85	## 3. `gc_step()` algorithm
    86	
    87	```rust
    88	pub fn gc_step(&self, budget: GcBudget) -> Result<GcReport, StoreError> {
    89	    let start = Instant::now();
    90	    let now_s = unix_now();
    91	    let mut report = GcReport::default();
    92	
    93	    // 3.1 — NIP-40 expired reaper.
    94	    let to_reap = self.scan_expiring_before(now_s, budget.max_events_per_step)?
    95	        .collect::<Result<Vec<_>, _>>()?;
    96	    for ev in to_reap {
    97	        if start.elapsed().as_millis() as u32 >= budget.max_duration_ms { break; }
    98	        self.reap_one(ev.raw.id.into(), TombstoneOrigin::NIP40Expiry, now_s)?;
    99	        report.expired_reaped += 1;
   100	    }
   101	
   102	    // 3.2 — Trim LRU back to target.
   103	    let lru_before = self.hot.lock().lru.len();
   104	    self.hot.lock().trim();
   105	    report.lru_evicted = lru_before.saturating_sub(self.hot.lock().lru.len());
   106	
   107	    // 3.3 — Purge old tombstones whose target event is absent.
   108	    let cutoff = now_s.saturating_sub(self.cfg.tombstone_retention_secs);
   109	    report.tombstones_purged = self.purge_old_tombstones(cutoff,
   110	        budget.max_events_per_step.saturating_sub(report.expired_reaped))?;
   111	
   112	    report.duration_ms = start.elapsed().as_millis() as u32;
   113	    Ok(report)
   114	}
   115	```
   116	
   117	Single `gc_step()` is bounded by `GcBudget { max_events_per_step, max_duration_ms }`. Defaults: `max_events_per_step = 2000`, `max_duration_ms = 50`. The actor calls `gc_step()`:
   118	
   119	- Every 60 seconds (cooperative; runs on the actor thread between mailbox messages).
   120	- On `MemoryWarningCapability::Pressure` (iOS / Android low-memory signals).
   121	- On any single `insert()` that observes `hot.lru.len() > 2 * target_hot_size` (safety net).
   122	
   123	`gc_step()` is **never** invoked from an FFI call path — it runs on the actor's own schedule so any latency it introduces is invisible to the platform.
   124	
   125	## 4. Claim / release wiring
   126	
   127	The kernel actor holds `view_claims: HashMap<ViewId, ClaimerId>`. On `open_view(spec)`:
   128	
   129	1. The view module's `dependencies(spec)` is consulted (per `kernel-substrate.md` §3).
   130	2. The composite reverse-index resolves the dependency set to a (small, bounded) set of currently-known event ids — the *view cover*.
   131	3. `store.claim(claimer_id, &cover_ids)` pins those events in hot.
   132	4. As events arrive matching the dependency, the actor calls `store.claim(claimer_id, &[new_id])` incrementally (claim is idempotent under increment).
   133	
   134	On `close_view(view_id)`:
   135	
   136	1. `store.release(claimer_id)` drops every pin in one call.
   137	2. The view module's `state` is dropped; its claim refcounts decay; the next `gc_step()` evicts any newly-unpinned cold from LRU.
   138	
   139	Restart recovery: `claims_meta` sub-db ([`keys.md`](keys.md) §1) holds the persisted per-`ClaimerId` pin set. On startup the actor rebuilds active views first (per the diagnostics replay sequence), then re-claims; entries in `claims_meta` whose `ClaimerId` is not associated with a re-opened view are dropped from the persisted map. This means the cold-start path always re-derives claims from open-view state, but the persistence is what lets the store survive an actor restart without losing hot-set protection mid-shutdown.
   140	
   141	## 5. Memory accounting (the ADR-0003 gate)
   142	
   143	The relevant figure for the M3 exit gate is **working-set RSS at the configuration described in ADR-0003 §Decision**: 100 active views, 10k hot events, 1M cached on disk, ≤ 100 MB.
   144	
   145	Components measured:
   146	
   147	| Source | Approx bytes | Notes |
   148	|---|---|---|
   149	| Hot LRU (10k × Arc<Event>) | ~30 MB | average kind:1 event with content ~800 B, profile/contacts can be 4–8 KB each; mix-weighted average ~3 KB; the `Arc` is shared with view module payloads so the same body isn't duplicated |
   150	| Claim refcount maps (10k entries) | ~0.5 MB | `HashMap<EventId, u32>` + reverse `by_claimer` |
   151	| Reverse index in-memory (composite keys for 100 views) | ~5 MB | from ADR-0001 — bounded by `~broad_axes_guardrail` per ADR-0001 |
   152	| Projection caches (author display, reaction counts) | ~10 MB | LRU-bounded by referenced-view count per ADR-0003 |
   153	| LMDB page cache (kernel-owned, *not* counted toward RSS budget) | 0 | OS-paged, evicted under pressure; counts against system memory but not app working set |
   154	| Watermarks (loaded as `HashMap` for hot lookups) | ~2 MB | M4 — assuming O(10k) watermarks (one per `(filter, relay)` pair) |
   155	| Tombstone bloom filter (if added — see open questions) | ~1 MB | accelerates the `tombstones.contains_key()` check on insert |
   156	| Action ledger in-flight rows | ~1 MB | bounded by spec §7.5 |
   157	| Slack / Rust allocator overhead | ~20 MB | empirical from reactivity-bench |
   158	| **Total target** | **~70 MB** | leaves ~30 MB headroom against the 100 MB gate |
   159	
   160	The 1M-events-on-disk dimension does **not** appear in the budget because LMDB does not page them into our heap; they exist in mmap'd pages the OS may evict at will. This is the design intent of ADR-0003.
   161	
   162	## 6. Failure modes and degraded behavior
   163	
   164	| Failure | Detection | Response |
   165	|---|---|---|
   166	| LMDB env out of space | LMDB `MDB_MAP_FULL` on a write | Run an emergency `gc_step()` with relaxed budget; if still full, surface `Effect::StoreOutOfSpace`, refuse new inserts, allow reads + deletes |
   167	| LRU evicted a still-pinned event (bug) | `trim()` would have skipped it; if observed, log + invariant violation | Pin reinstated from `claims_meta`; fire `tracing::error!`; flagged as critical bug class to investigate |
   168	| `gc_step()` over-budget | `start.elapsed() > max_duration_ms` mid-loop | Break out of current loop early; remaining work picked up next call (no state corruption — every reaped event is its own transaction) |
   169	| `release()` called for unknown `ClaimerId` | `by_claimer.remove` returns None | Silent no-op; logged at debug; not a bug (idempotent close) |
   170	| Memory warning during heavy insert burst | iOS `didReceiveMemoryWarning` → `MemoryWarningCapability` event | Actor lowers `target_hot_size` to 5k, runs `gc_step({max_events_per_step:5000, max_duration_ms:200})` once; restored after the warning clears |
   171	
   172	## 7. Diagnostics integration (ADR-0007)
   173	
   174	The store exposes a `StoreHealth` snapshot for the diagnostics bridge:
   175	
   176	```rust
   177	pub struct StoreHealth {
   178	    pub primary_event_count: u64,
   179	    pub tombstone_count: u64,
   180	    pub hot_lru_size: usize,
   181	    pub claim_pinned_count: usize,
   182	    pub watermark_count: usize,
   183	    pub on_disk_bytes: u64,
   184	    pub last_gc: Option<GcReport>,
   185	    pub last_gc_at_ms: Option<u128>,
   186	}
   187	```
   188	
   189	Surfaced in the diagnostics screen alongside relay status (ADR-0007 §1). The Phase 1a.7 proof app already has the rendering scaffold (`ios/NmpStress/NmpStress/DiagnosticsView.swift`); M3 adds the StoreHealth row to it.
   190	
   191	## 8. Why not a periodic full sweep?
   192	
   193	A full sweep is `O(stored_events)`. With 1M events on disk the LMDB scan alone is 100–500 ms wall-time on iPhone 12 NAND — well outside the actor's single-message budget. The bounded `gc_step()` with explicit budget is therefore the only correct shape; it composes with LMDB's natural mmap eviction model and never blocks the mailbox for long.
   194	
   195	A periodic vacuum/compact pass (LMDB's equivalent of `VACUUM`) **is** scheduled — once per app launch, at idle, after the first 30 seconds of quiescence. It is *not* part of `gc_step`'s budget envelope and runs as a separate low-priority actor message that yields between LMDB page boundaries.

exec
/bin/zsh -lc "nl -ba docs/design/lmdb/watermarks.md | sed -n '1,230p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# LMDB sub-design: watermarks, provenance, migrations
     2	
     3	> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Covers the three NMP-owned LMDB sub-databases that hold non-event durable state: `watermarks`, `provenance`, and the per-`DomainModule` sub-databases tracked by `_meta`.
     4	
     5	## 1. Watermark table
     6	
     7	Sub-db: `watermarks`. Populated by the M4 NIP-77 negentropy engine and consulted by the M2 subscription planner (per `docs/product-spec/subsystems.md` §7.2 "coverage-aware backfill").
     8	
     9	Key: `filter_hash[32] ‖ relay_url_bytes` — exact-key lookups only; no scans by `filter_hash` alone are needed (the planner always knows the relay it's about to query). The relay url is appended raw (no length prefix) because lookups are exact-key and the trailing bytes have no semantic role beyond identification.
    10	
    11	Value: CBOR `WatermarkRow`:
    12	
    13	```rust
    14	#[derive(Serialize, Deserialize, Clone, Debug)]
    15	pub struct WatermarkRow {
    16	    pub key: WatermarkKey,
    17	    pub synced_up_to: u64,                 // unix seconds
    18	    pub last_sync_method: SyncMethod,      // Negentropy | ReqScan | Manual
    19	    pub last_negentropy_state: Option<Vec<u8>>, // engine-opaque resume blob (M4)
    20	    pub bytes_saved_vs_req: u64,           // cumulative; for diagnostics
    21	    pub updated_at: u64,                   // unix seconds
    22	}
    23	```
    24	
    25	`last_negentropy_state` is an opaque byte blob written by the NIP-77 engine (M4) — the store does not interpret it. Empty for `ReqScan` / `Manual` rows.
    26	
    27	### 1.1 Authoritative cache-miss semantics
    28	
    29	Per §7.1 of the spec: "A cache-miss query against a fully-synced `(filter, relay)` pair is **authoritative**." The store implements this via the read path:
    30	
    31	```rust
    32	pub fn coverage(&self, key: &WatermarkKey, now_s: u64) -> Coverage {
    33	    match self.read_watermark(key) {
    34	        Ok(Some(row)) if row.synced_up_to >= now_s.saturating_sub(self.cfg.coverage_staleness_secs) =>
    35	            Coverage::CompleteAsOf(row.synced_up_to),
    36	        Ok(Some(row)) => Coverage::PartialUpTo(row.synced_up_to),
    37	        Ok(None) => Coverage::Unknown,
    38	        Err(_) => Coverage::Unknown,  // degraded; do not lie about completeness
    39	    }
    40	}
    41	```
    42	
    43	`coverage_staleness_secs` defaults to 300 s — a row that hasn't been re-confirmed in 5 minutes is treated as partial. The planner uses this signal to decide whether a missing-event query is "doesn't exist" (CompleteAsOf) or "need to fetch" (PartialUpTo / Unknown).
    44	
    45	### 1.2 Restart hydration
    46	
    47	On `LmdbEventStore::open()`, the store reads all `watermarks` rows and builds an in-memory `HashMap<WatermarkKey, WatermarkRow>` for hot lookups. Every `write_watermark` updates both the in-memory map and the LMDB row in a single `RwTxn`. Restart re-derives the map; we don't need a separate cache file.
    48	
    49	For installations with O(100k+) watermarks (an edge case — typical apps see O(100)–O(10k)), the in-memory map switches to a lazy-loaded variant that pages on demand. Threshold and switching logic deferred to M4 when the negentropy engine's real-world row count is measured.
    50	
    51	## 2. Provenance
    52	
    53	Sub-db: `provenance`. Per-event sidecar; the master doc §9 justifies the split-table choice.
    54	
    55	Key: `event_id[32]`. Value: CBOR `ProvenanceRow`:
    56	
    57	```rust
    58	#[derive(Serialize, Deserialize, Clone, Debug)]
    59	pub struct ProvenanceRow {
    60	    pub sources: SmallVec<[ProvenanceEntry; 4]>,    // bounded at 32; see master doc §9
    61	}
    62	
    63	#[derive(Serialize, Deserialize, Clone, Debug)]
    64	pub struct ProvenanceEntry {
    65	    pub relay_url: String,
    66	    pub first_seen_ms: u64,
    67	    pub last_seen_ms: u64,
    68	    pub primary: bool,
    69	}
    70	```
    71	
    72	The `primary: bool` is set deterministically: after every mutation, the `sources` vec is sorted by `(first_seen_ms, relay_url)` and the head element gets `primary = true`, all others `false`. This produces a stable "first observed relay" identifier regardless of the order in which redeliveries are processed.
    73	
    74	### 2.1 Mutation hot path
    75	
    76	For a duplicate-id insert, the per-event provenance write is the **only** LMDB write (per [`keys.md`](keys.md) §9 worked example). The store reads the existing row, mutates the matching entry's `last_seen_ms` (or appends), re-sorts + recomputes `primary`, and writes it back. Total cost: 1 read + 1 write on a 4-element CBOR row — well under 50 µs on iPhone 12 NAND.
    77	
    78	The bound of 32 distinct relays per event is empirical: in practice an event is delivered by 1–6 relays; events propagated to "everywhere" (popular kind:0 / kind:3) might hit 15–25. The 32nd entry overwrites the oldest non-primary slot, preserving the primary anchor.
    79	
    80	### 2.2 Reuse in outbox routing (M2)
    81	
    82	The M2 subscription planner consults `provenance_for(id)` to learn which relays have already delivered an event when scoring per-relay coverage in `LogicalInterestStatus::relay_urls`. This avoids re-fetching the same event from relays we already know carry it. The provenance sidecar is also part of the ADR-0007 diagnostics bridge — the diagnostics screen shows per-event source counts in the firehose tap view.
    83	
    84	## 3. Filter canonicalisation (for `filter_hash`)
    85	
    86	The `filter_hash` field in `WatermarkKey` is BLAKE3 of the canonical filter encoding. Canonicalisation rules:
    87	
    88	1. Within each tag-value array (`#e`, `#p`, `#a`, etc.), sort ascending bytewise.
    89	2. Sort the `kinds` array ascending numerically.
    90	3. Sort the `authors` array ascending bytewise.
    91	4. Sort the `ids` array ascending bytewise.
    92	5. Encode the filter as CBOR with map keys in this lexicographic order: `authors`, `ids`, `kinds`, `since`, `until`, `limit`, `search`, then `#<tag>` keys in ascending tag-letter order.
    93	6. BLAKE3-hash the resulting bytes.
    94	
    95	This produces a deterministic hash that is stable across `Filter` field-order variations and across Rust HashMap ordering randomness. The implementation lives at `crates/nmp-core/src/store/watermarks.rs::canonical_filter_hash(&Filter) -> [u8; 32]` and is the single source of truth for the planner + sync engine + dump format.
    96	
    97	A filter with `limit: Some(N)` produces a *different* hash than the same filter without `limit` — because their cache-coverage semantics genuinely differ. A planner that wants to share a watermark across "limit=200" and "limit=500" requests of the same shape strips `limit` before hashing (this is a planner-side optimisation, not a store-side one).
    98	
    99	## 4. Migration plumbing
   100	
   101	Per `kernel-substrate.md` §2: `DomainModule` declares `SCHEMA_VERSION` and `migrations()`. The store handles applying them at startup.
   102	
   103	### 4.1 Registry extension required
   104	
   105	The existing `ModuleRegistry` (`crates/nmp-core/src/substrate/mod.rs:36-79`) stores only `ModuleDescriptor { namespace, family, rust_type }` — the concrete `M: DomainModule` type is consumed by the generic `register_domain::<M>()` call and not retained, so the store has no runtime path from a namespace string back to `M::SCHEMA_VERSION` or `M::migrations()`. M3 extends `ModuleDescriptor` for the Domain family with two `fn`-pointer factories — matching the existing `DomainIndex::key_fn: fn(&[u8]) -> ...` pattern (`substrate/domain.rs:18`):
   106	
   107	```rust
   108	// Added in M3 — substrate/domain.rs
   109	pub struct DomainFactories {
   110	    pub schema_version: fn() -> u32,
   111	    pub migrations: fn() -> Vec<DomainMigration>,
   112	    pub indexes: fn() -> Vec<DomainIndex>,
   113	}
   114	
   115	// ModuleRegistry::register_domain becomes:
   116	pub fn register_domain<M: DomainModule>(&mut self) {
   117	    let factories = DomainFactories {
   118	        schema_version: || M::SCHEMA_VERSION,
   119	        migrations: M::migrations,
   120	        indexes: M::indexes,
   121	    };
   122	    self.push_domain::<M>(M::NAMESPACE, factories);
   123	}
   124	```
   125	
   126	The store reads these factories at open time. No `Box<dyn DomainModule>` is required, no trait object-safety constraints are imposed on `DomainModule`, and the change is additive to the existing trait.
   127	
   128	### 4.2 Startup sequence
   129	
   130	```rust
   131	pub fn open(path: &Path, modules: &ModuleRegistry) -> Result<Self, StoreError> {
   132	    let env = open_lmdb_environment(path)?;
   133	    let meta = env.open_db(Some("_meta"))?;
   134	    let mut store = Self::bootstrap(env)?;
   135	
   136	    // _kernel schema version
   137	    store.migrate_kernel_schema(&meta)?;
   138	
   139	    // each registered DomainModule
   140	    for (namespace, factories) in modules.domain_factories() {
   141	        let current = store.read_meta_schema_version(namespace)?;
   142	        let target = (factories.schema_version)();
   143	        let mut applied = current;
   144	        let mut steps = (factories.migrations)();
   145	        steps.retain(|m| m.from_version >= current && m.to_version <= target);
   146	        steps.sort_by_key(|m| m.from_version);
   147	        for step in steps {
   148	            store.run_migration_step(namespace, step)?;
   149	            applied = step.to_version;
   150	            store.write_meta_schema_version(namespace, applied)?;
   151	        }
   152	        if applied < target {
   153	            return Err(StoreError::MigrationFailed { /* missing step */ });
   154	        }
   155	        if applied > target {
   156	            return Err(StoreError::SchemaTooNew { /* downgrade */ });
   157	        }
   158	    }
   159	    Ok(store)
   160	}
   161	```
   162	
   163	Each `run_migration_step` opens its own `RwTxn`, calls `step.apply(&mut migration_tx)`, drains `migration_tx.writes()` into the relevant sub-db, and commits. Either the whole step lands atomically or LMDB rolls it back on commit failure.
   164	
   165	### 4.3 Rollback semantics
   166	
   167	LMDB does not support cross-process downgrade; once `_meta.<namespace>.schema_version` is bumped, there is no "undo." Therefore:
   168	
   169	- If migration step N fails: `_meta` is **not** bumped; module starts in degraded mode (per [`trait.md`](trait.md) §5); user-visible diagnostic surfaces the failure.
   170	- If migration step N succeeds but N+1 fails: `_meta` is at N (the highest successful step). The module is "partly migrated"; the same degraded-mode handling applies; on next startup the runner retries from N → N+1.
   171	- If the user actually needs to downgrade (a forensics use case), they delete the sub-db and re-sync from relays. The `nmp dump` format is the supported escape hatch.
   172	
   173	### 4.4 Cross-module migration coordination (deferred — see open questions)
   174	
   175	`kernel-substrate.md` §12 already flags "cross-module migration coordination" as an open question. M3 implements the per-module path; cross-module coordination is deferred to a future module that needs it. The trait surface does not preclude adding it later: a future `Vec<NamespaceDependency>` field on `DomainMigration` would let the runner topologically order modules' migrations.
   176	
   177	## 5. The kernel store's own schema
   178	
   179	The kernel's internal LMDB layout (provenance + secondaries + tombstones + watermarks) is itself versioned via the reserved `_kernel` namespace in `_meta`. Current version: `1`. A future schema change (e.g., a new secondary index) bumps this and runs a one-time rebuild from primary events.
   180	
   181	The schema migration for `_kernel` v1 → v2 is privileged: it can read every sub-db and rewrite as needed. It runs **before** any `DomainModule` migration so module code never sees a stale kernel schema.
   182	
   183	## 6. Diagnostics surface
   184	
   185	`WatermarkRow` and `ProvenanceRow` are read by the ADR-0007 diagnostics bridge:
   186	
   187	- `LogicalInterestStatus.cache_coverage` resolves from `coverage()` (§1.1 above) for the relevant `(filter_hash, relay_url)` pair.
   188	- The firehose-tap diagnostic surfaces per-event provenance: which relays contributed each row, with `primary` flagged.
   189	- `StoreHealth.watermark_count` (per [`gc.md`](gc.md) §7) summarises the global count.
   190	
   191	These are read-mostly paths; the only contention is between the actor's `write_watermark` calls (driven by M4 sync completions) and the diagnostics query, which is bounded by LMDB's MVCC reader semantics — no lock contention.

exec
/bin/zsh -lc "nl -ba docs/design/lmdb/tests.md | sed -n '1,260p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	# LMDB sub-design: test plan
     2	
     3	> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Maps every insert invariant in `docs/product-spec/subsystems.md` §7.1 to a concrete test in `crates/nmp-testing/tests/`. Each test exists for both `MemEventStore` (always) and `LmdbEventStore` (under `#[cfg(feature = "lmdb-backend")]`).
     4	
     5	## 1. Test harness shape
     6	
     7	```rust
     8	// crates/nmp-testing/src/store_harness.rs
     9	pub struct StoreHarness {
    10	    pub store: Box<dyn EventStore>,
    11	    pub tmp: tempfile::TempDir,
    12	    pub keys: nostr::Keys,
    13	}
    14	
    15	impl StoreHarness {
    16	    pub fn mem() -> Self { /* MemEventStore */ }
    17	    pub fn lmdb() -> Self { /* LmdbEventStore in tmp dir */ }
    18	
    19	    pub fn insert(&self, builder: EventBuilder, source: &str) -> InsertOutcome { /* ... */ }
    20	    pub fn assert_present(&self, id: &EventId);
    21	    pub fn assert_tombstoned(&self, id: &EventId);
    22	    pub fn restart(&mut self);   // close + reopen the store; LMDB only
    23	}
    24	
    25	// Tests use a macro to run against both backends.
    26	macro_rules! for_each_backend {
    27	    ($name:ident, $body:expr) => {
    28	        #[test] fn $name() { let mut h = StoreHarness::mem(); $body(&mut h); }
    29	        #[cfg(feature = "lmdb-backend")]
    30	        #[test] fn paste::paste!([<$name _lmdb>])() {
    31	            let mut h = StoreHarness::lmdb(); $body(&mut h);
    32	        }
    33	    };
    34	}
    35	```
    36	
    37	The harness lives in `crates/nmp-testing/src/` so per-test files are short and declarative.
    38	
    39	## 2. Invariant → test mapping
    40	
    41	Every row of the §7.1 table:
    42	
    43	### 2.1 Insert API single path (§7.1 row "Insert API")
    44	
    45	File: `crates/nmp-testing/tests/store_insert_path.rs`
    46	
    47	```rust
    48	for_each_backend!(insert_returns_insert_outcome, |h: &mut StoreHarness| {
    49	    let event = h.signed(EventBuilder::text_note("hello", &[]));
    50	    let outcome = h.store.insert(event.clone(), &"wss://t/".into(), 0).unwrap();
    51	    assert!(matches!(outcome, InsertOutcome::Inserted { .. }));
    52	    assert!(h.store.get_by_id(&event.id.to_bytes()).unwrap().is_some());
    53	});
    54	```
    55	
    56	Plus a static-assertion-style test ensuring no other public function on `EventStore` writes to the primary store (compile-time check by inspecting trait method list via a build script — deferred to v1.x; v1 covers via review).
    57	
    58	### 2.2 Signature verification (§7.1 row "Signature/delegation validity")
    59	
    60	File: `crates/nmp-testing/tests/store_invalid_sig.rs`
    61	
    62	Builds an event, mutates the signature, inserts. Expects `InsertOutcome::Rejected { reason: RejectReason::BadSignature }` and no row in primary, secondaries, provenance, or tombstones. Also tests a malformed NIP-26 delegation tag (rejects with `BadDelegation`).
    63	
    64	### 2.3 Duplicate id → provenance merge (§7.1 row "Duplicate id")
    65	
    66	File: `crates/nmp-testing/tests/store_provenance_merge.rs`
    67	
    68	```rust
    69	for_each_backend!(duplicate_merges_provenance_keeps_earliest, |h| {
    70	    let ev = h.signed(EventBuilder::text_note("x", &[]));
    71	    let o1 = h.store.insert(ev.clone(), &"wss://a/".into(), 1000).unwrap();
    72	    let o2 = h.store.insert(ev.clone(), &"wss://b/".into(), 2000).unwrap();
    73	    assert!(matches!(o1, InsertOutcome::Inserted { .. }));
    74	    assert!(matches!(o2, InsertOutcome::Duplicate { sources_after: 2, .. }));
    75	    let p = h.store.provenance_for(&ev.id.to_bytes()).unwrap();
    76	    assert_eq!(p.len(), 2);
    77	    let primary = p.iter().find(|e| e.primary).unwrap();
    78	    assert_eq!(primary.relay_url, "wss://a/");
    79	    assert_eq!(primary.first_seen_ms, 1000); // earliest preserved
    80	});
    81	```
    82	
    83	### 2.4 Replaceable supersession (§7.1 row "Replaceable kinds")
    84	
    85	File: `crates/nmp-testing/tests/store_replaceable.rs`
    86	
    87	Inserts two kind:0 from same pubkey, second with later `created_at`. Asserts: `get_by_id(first_id)` returns None; `scan_by_author_kind(pk, &[0], None, None, 10)` returns one row; the row's id is the second. Tie-break test: two kind:0 with same `created_at` — keep the lexicographically smaller id.
    88	
    89	### 2.5 Parameterized replaceable (§7.1 row "Parameterized replaceable")
    90	
    91	File: `crates/nmp-testing/tests/store_param_replaceable.rs`
    92	
    93	Insert two kind:30023 with same `(pubkey, d=foo)`, second newer; assert only the second is returned by `get_param_replaceable(pk, 30023, b"foo")`. Insert a third with same kind+pubkey but `d=bar` — assert both `foo` and `bar` are independently retrievable. Assert that a kind:30024 with `d=foo` (different kind) does not collide with the kind:30023.
    94	
    95	### 2.6 Kind:5 self-delete + tombstone persistence (§7.1 row "Kind 5")
    96	
    97	File: `crates/nmp-testing/tests/store_kind5_tombstone.rs`
    98	
    99	- Insert kind:1 by Alice.
   100	- Insert kind:5 by Alice referencing the kind:1 via `e` tag.
   101	- Assert kind:1 gone from primary; tombstone row exists with `target_id == kind1.id`, `origin == Kind5`.
   102	- Insert the same kind:1 again — assert `InsertOutcome::Tombstoned`, no primary row created.
   103	- Restart store; repeat the re-insertion — assert tombstone persists across restart.
   104	
   105	### 2.7 Foreign kind:5 ignored (§7.1 row "Kind 5" — foreign clause)
   106	
   107	File: `crates/nmp-testing/tests/store_kind5_foreign.rs`
   108	
   109	- Insert kind:1 by Alice.
   110	- Insert kind:5 by Bob referencing Alice's kind:1.
   111	- Assert: kind:1 is still present in primary (Bob can't delete Alice's event); the kind:5 event itself is stored (so other clients can see it); no tombstone row was written.
   112	
   113	### 2.8 NIP-40 expiration scheduling (§7.1 row "NIP-40 expiration")
   114	
   115	File: `crates/nmp-testing/tests/store_nip40_expiration.rs`
   116	
   117	- Insert kind:1 with `expiration` tag at `now + 1 second`.
   118	- Assert `scan_expiring_before(now + 5, 10)` returns the event.
   119	- Call `gc_step(GcBudget { max_events_per_step: 10, max_duration_ms: 100 })` at `now + 2`.
   120	- Assert primary row gone; tombstone written with `origin == NIP40Expiry`.
   121	- Insert same event again — assert `InsertOutcome::Tombstoned`.
   122	- Insert an event with `expiration` already in the past — assert `InsertOutcome::Rejected { reason: ExpiredOnArrival }`.
   123	- Restart store; insert new event with `expiration` at `now + 1`; assert the reaper picks it up after restart (the `idx_expires` cursor scan is the source of truth — no separate timer needs to survive restart).
   124	
   125	### 2.9 Watermarks (§7.1 "Sync watermarks")
   126	
   127	File: `crates/nmp-testing/tests/store_watermarks.rs`
   128	
   129	- Write a watermark; read it back; assert equal.
   130	- Restart store; read again; assert preserved.
   131	- Test `coverage()`: row with `synced_up_to = now - 60s` → `Coverage::CompleteAsOf` (under default 300s staleness); row with `synced_up_to = now - 600s` → `Coverage::PartialUpTo`; missing row → `Coverage::Unknown`.
   132	- `list_watermarks_for_relay("wss://a/")` returns only rows for that relay.
   133	- Concurrent writes to the same key (simulated): last-writer-wins, no row corruption.
   134	
   135	### 2.10 Claims + GC (§7.1 "GC")
   136	
   137	File: `crates/nmp-testing/tests/store_gc_claims.rs`
   138	
   139	- Insert 100 events; all in hot LRU (under default 10k ceiling).
   140	- Claim 10 of them under `ClaimerId(1)`.
   141	- Configure `target_hot_size = 50`; insert another 50 events; call `gc_step`.
   142	- Assert: 10 claimed events still present in hot (`store.get_by_id` is a fast in-memory hit — measurable via a counter exposed for the test); 40 unclaimed events evicted from LRU but still readable from disk.
   143	- Release `ClaimerId(1)`; insert another 20 events; call `gc_step`.
   144	- Assert: previously claimed events now subject to LRU eviction.
   145	
   146	### 2.11 Dump round-trip (master doc §10)
   147	
   148	File: `crates/nmp-testing/tests/store_dump_roundtrip.rs`
   149	
   150	- Build a populated store: 1000 events, 50 tombstones, 100 watermarks, 200 domain rows across 3 namespaces.
   151	- `dump(&mut buf1, DumpFormat::Jsonl)`.
   152	- Open a fresh store; replay every line; `dump(&mut buf2, ...)`.
   153	- Assert `buf1 == buf2` byte-for-byte (sort by stable key first — the dump iterates sub-dbs in a deterministic order documented in the dump module).
   154	
   155	### 2.12 Domain migration success + failure (master doc §6)
   156	
   157	File: `crates/nmp-testing/tests/store_domain_migration.rs`
   158	
   159	- Register `TestModuleV1` with `SCHEMA_VERSION = 1` and no migrations; open store; assert `_meta.test_module.schema_version == 1`.
   160	- Close store; register `TestModuleV2` with `SCHEMA_VERSION = 2` and one migration v1→v2 that writes one key; open store; assert migration ran and key exists.
   161	- Close; register `TestModuleV3` with `SCHEMA_VERSION = 3` and a deliberately failing migration v2→v3; open store; assert `Effect::DomainSchemaTooNew { namespace: "test_module" }` (under degraded-mode rules) and `_meta` still at v2.
   162	- Close; remove the failing migration; reopen — assert successful catch-up to v3 (idempotent retry).
   163	
   164	### 2.13 Domain isolation (`kernel-substrate.md` §8)
   165	
   166	File: `crates/nmp-testing/tests/store_domain_isolation.rs`
   167	
   168	- Open `DomainHandle` for module A; write key `K`.
   169	- Open `DomainHandle` for module B; read key `K` — assert returns `None`.
   170	- Module B's `scan_prefix(b"")` returns only module B's rows.
   171	
   172	### 2.14 Cold-start performance (master doc §12)
   173	
   174	Scenario in `crates/nmp-testing/bin/firehose-bench/src/scenarios/cold_start.rs` (already exists in M1; extended here):
   175	
   176	- Pre-populate an LMDB store with a representative session (~20k events: 10k kind:1, 8k kind:0, 2k kind:3 / 10002).
   177	- Tar + ship the file with the test fixture.
   178	- Measure: open store, register modules, run the bootstrap sequence that the actor runs on app launch, until the first `AppUpdate::FullState` is emitted with non-empty timeline.
   179	- Gate: ≤ 1.5 s on iPhone 12 hardware; ≤ 800 ms on iPhone 16 Pro simulator.
   180	
   181	### 2.15 Working-set memory (ADR-0003)
   182	
   183	Scenario in `crates/nmp-testing/bin/reactivity-bench` — extended with a new `--scenario working_set_lmdb` mode:
   184	
   185	- Insert 1M synthetic events into the store.
   186	- Open 100 view subscriptions covering 10k events.
   187	- Run for 60 seconds with light churn (insert 10 events / sec).
   188	- Sample RSS every 5 seconds via `/proc/self/status` on Linux / `mach_task_basic_info` on iOS.
   189	- Gate: max RSS ≤ 100 MB over the run.
   190	
   191	### 2.16 Restart preserves replaceable semantics (`plan.md` §M3 exit gate)
   192	
   193	File: `crates/nmp-testing/tests/store_replaceable_restart.rs`
   194	
   195	- Write kind:0 v1; assert present.
   196	- Write kind:0 v2 (newer); assert v1 gone, v2 present.
   197	- Restart store; assert v2 still present, v1 still gone.
   198	- Write kind:0 v0 (older than v2); assert no change (`InsertOutcome::Superseded`).
   199	
   200	## 3. Property tests
   201	
   202	In `crates/nmp-testing/tests/store_props.rs` using `proptest`:
   203	
   204	- **Insert is total under random valid events.** Generate a vec of valid signed events, insert in any order, assert the store's `get_by_id` agrees with the model (a `HashMap` reference impl).
   205	- **Replaceable convergence.** For any sequence of replaceable inserts for the same `(pubkey, kind, [d])` key, the final stored event is the (max created_at, min id) winner regardless of insertion order.
   206	- **Provenance commutativity.** For any two relay sources `r1, r2` and identical event, the post-state of provenance is identical to inserting `r2` first then `r1`.
   207	- **`nmp dump` is a fixed point.** Round-trip equality after N random operations.
   208	
   209	## 4. Cross-test invariants (asserted in a `teardown` hook for every test)
   210	
   211	Every test ends with `harness.assert_invariants()`:
   212	
   213	1. Every event in the primary store has a `provenance` row with ≥ 1 entry.
   214	2. Every secondary index entry's `event_id` resolves to an existing primary row.
   215	3. Every tombstone's `target_id` does **not** exist in the primary store.
   216	4. The `_meta._kernel.schema_version` is at the latest version the binary knows.
   217	5. The hot LRU contains only events that exist in the primary store.
   218	
   219	Violation of any invariant fails the test with a precise diff of which sub-db is out of sync.
   220	
   221	## 5. CI integration
   222	
   223	`cargo test --workspace --features lmdb-backend` becomes part of the pre-merge gate from M3 onward (`plan.md` §6 will be updated). The criterion benches in `crates/nmp-testing/benches/store_*.rs` run nightly with regression checks against the previous week's median (>5% regression on any p99 fails the nightly).

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/substrate/domain.rs | sed -n '1,120p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	pub trait DomainModule: Send + Sync + 'static {
     2	    const NAMESPACE: &'static str;
     3	    const SCHEMA_VERSION: u32;
     4	
     5	    fn migrations() -> Vec<DomainMigration>;
     6	    fn indexes() -> Vec<DomainIndex>;
     7	    fn register(registry: &mut DomainRegistry);
     8	}
     9	
    10	pub struct DomainMigration {
    11	    pub from_version: u32,
    12	    pub to_version: u32,
    13	    pub apply: fn(&mut MigrationTx) -> Result<(), String>,
    14	}
    15	
    16	pub struct DomainIndex {
    17	    pub name: &'static str,
    18	    pub key_fn: fn(&[u8]) -> Option<Vec<u8>>,
    19	}
    20	
    21	#[derive(Default)]
    22	pub struct MigrationTx {
    23	    writes: Vec<(Vec<u8>, Vec<u8>)>,
    24	}
    25	
    26	impl MigrationTx {
    27	    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
    28	        self.writes.push((key, value));
    29	    }
    30	
    31	    pub fn writes(&self) -> &[(Vec<u8>, Vec<u8>)] {
    32	        &self.writes
    33	    }
    34	}
    35	
    36	#[derive(Default)]
    37	pub struct DomainRegistry {
    38	    records: Vec<&'static str>,
    39	}
    40	
    41	impl DomainRegistry {
    42	    pub fn register_record<T: 'static>(&mut self) {
    43	        self.records.push(std::any::type_name::<T>());
    44	    }
    45	
    46	    pub fn records(&self) -> &[&'static str] {
    47	        &self.records
    48	    }
    49	}

exec
/bin/zsh -lc "nl -ba crates/nmp-core/src/substrate/mod.rs | sed -n '1,130p'" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
     1	mod action;
     2	mod capability;
     3	mod domain;
     4	mod identity;
     5	mod view;
     6	
     7	pub use action::{
     8	    ActionContext, ActionId, ActionInput, ActionModule, ActionPlan, ActionRejection, ActionStatus,
     9	    ActionTransition,
    10	};
    11	pub use capability::{CapabilityEnvelope, CapabilityModule, CapabilityRequest};
    12	pub use domain::{DomainIndex, DomainMigration, DomainModule, DomainRegistry, MigrationTx};
    13	pub use identity::{
    14	    BoxFuture, IdentityContext, IdentityError, IdentityId, IdentityModule, IdentityScopeKind,
    15	    SignedEvent, SigningError, UnsignedEvent,
    16	};
    17	pub use view::{EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule};
    18	
    19	#[derive(Clone, Debug, Eq, PartialEq)]
    20	pub struct ModuleDescriptor {
    21	    pub namespace: &'static str,
    22	    pub family: ModuleFamily,
    23	    pub rust_type: &'static str,
    24	}
    25	
    26	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
    27	pub enum ModuleFamily {
    28	    Domain,
    29	    View,
    30	    Action,
    31	    Capability,
    32	    Identity,
    33	}
    34	
    35	#[derive(Clone, Debug, Default, Eq, PartialEq)]
    36	pub struct ModuleRegistry {
    37	    descriptors: Vec<ModuleDescriptor>,
    38	}
    39	
    40	impl ModuleRegistry {
    41	    pub fn register_domain<M: DomainModule>(&mut self) {
    42	        self.push::<M>(M::NAMESPACE, ModuleFamily::Domain);
    43	    }
    44	
    45	    pub fn register_view<M: ViewModule>(&mut self) {
    46	        self.push::<M>(M::NAMESPACE, ModuleFamily::View);
    47	    }
    48	
    49	    pub fn register_action<M: ActionModule>(&mut self) {
    50	        self.push::<M>(M::NAMESPACE, ModuleFamily::Action);
    51	    }
    52	
    53	    pub fn register_capability<M: CapabilityModule>(&mut self) {
    54	        self.push::<M>(M::NAMESPACE, ModuleFamily::Capability);
    55	    }
    56	
    57	    pub fn register_identity<M: IdentityModule>(&mut self) {
    58	        self.push::<M>(M::NAMESPACE, ModuleFamily::Identity);
    59	    }
    60	
    61	    pub fn descriptors(&self) -> &[ModuleDescriptor] {
    62	        &self.descriptors
    63	    }
    64	
    65	    fn push<M: 'static>(&mut self, namespace: &'static str, family: ModuleFamily) {
    66	        if self
    67	            .descriptors
    68	            .iter()
    69	            .any(|existing| existing.namespace == namespace && existing.family == family)
    70	        {
    71	            return;
    72	        }
    73	        self.descriptors.push(ModuleDescriptor {
    74	            namespace,
    75	            family,
    76	            rust_type: std::any::type_name::<M>(),
    77	        });
    78	    }
    79	}

exec
/bin/zsh -lc 'rg -n "TODO|FIXME|XXX|HACK|todo!|unimplemented!|panic!|unwrap'"\\(\\)|stub|placeholder|deferred|open question|for later|TBD\" docs/design/lmdb-schema.md docs/design/lmdb/gc.md docs/design/lmdb/keys.md docs/design/lmdb/tests.md docs/design/lmdb/trait.md docs/design/lmdb/watermarks.md" in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
docs/design/lmdb/watermarks.md:49:For installations with O(100k+) watermarks (an edge case — typical apps see O(100)–O(10k)), the in-memory map switches to a lazy-loaded variant that pages on demand. Threshold and switching logic deferred to M4 when the negentropy engine's real-world row count is measured.
docs/design/lmdb/watermarks.md:173:### 4.4 Cross-module migration coordination (deferred — see open questions)
docs/design/lmdb/watermarks.md:175:`kernel-substrate.md` §12 already flags "cross-module migration coordination" as an open question. M3 implements the per-module path; cross-module coordination is deferred to a future module that needs it. The trait surface does not preclude adding it later: a future `Vec<NamespaceDependency>` field on `DomainMigration` would let the runner topologically order modules' migrations.
docs/design/lmdb/gc.md:155:| Tombstone bloom filter (if added — see open questions) | ~1 MB | accelerates the `tombstones.contains_key()` check on insert |
docs/design/lmdb/keys.md:3:> Part of [`docs/design/lmdb-schema.md`](../lmdb-schema.md). Fixes the byte layout of every sub-database the NMP store opens. Primary event storage is owned by `nostr-lmdb`; everything below is NMP-owned and lives in NMP-managed sub-databases under the same `lmdb::Environment` (per open question 1 in the master doc).
docs/design/lmdb/tests.md:50:    let outcome = h.store.insert(event.clone(), &"wss://t/".into(), 0).unwrap();
docs/design/lmdb/tests.md:52:    assert!(h.store.get_by_id(&event.id.to_bytes()).unwrap().is_some());
docs/design/lmdb/tests.md:56:Plus a static-assertion-style test ensuring no other public function on `EventStore` writes to the primary store (compile-time check by inspecting trait method list via a build script — deferred to v1.x; v1 covers via review).
docs/design/lmdb/tests.md:71:    let o1 = h.store.insert(ev.clone(), &"wss://a/".into(), 1000).unwrap();
docs/design/lmdb/tests.md:72:    let o2 = h.store.insert(ev.clone(), &"wss://b/".into(), 2000).unwrap();
docs/design/lmdb/tests.md:75:    let p = h.store.provenance_for(&ev.id.to_bytes()).unwrap();
docs/design/lmdb/tests.md:77:    let primary = p.iter().find(|e| e.primary).unwrap();

exec
/bin/zsh -lc 'git status --short && git rev-parse --short HEAD && git show --stat --oneline --decorate --no-renames 9fead0e' in /Users/pablofernandez/Work/nostr-multi-platform
 succeeded in 0ms:
?? .claire/
?? .claude/
?? docs/perf/codex-reviews/031fc07.md
?? docs/perf/codex-reviews/9fead0e.md
75d18ff
9fead0e design(m3): LMDB schema + EventStore trait + GC policy
 docs/design/lmdb-schema.md     | 234 +++++++++++++++++++++++++++++++
 docs/design/lmdb/gc.md         | 195 ++++++++++++++++++++++++++
 docs/design/lmdb/keys.md       | 150 ++++++++++++++++++++
 docs/design/lmdb/tests.md      | 223 +++++++++++++++++++++++++++++
 docs/design/lmdb/trait.md      | 312 +++++++++++++++++++++++++++++++++++++++++
 docs/design/lmdb/watermarks.md | 191 +++++++++++++++++++++++++
 6 files changed, 1305 insertions(+)

codex
**Findings**

- High: [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:91) promises primary + secondary atomicity in one `RwTxn`, but [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:218) leaves shared `nostr-lmdb` environment access unresolved. Fix: make env/txn sharing a blocking ADR before M3 implementation, or specify crash-recovery/two-phase semantics.

- High: [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:235) exposes only raw `claim(claimer, ids)`, while [gc.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/gc.md:13) relies on bounded `open_view_cover` from the composite reverse index. That does not enforce D2 through the trait. Fix: add claim budget/cover validation or a view-cover registration API that rejects/truncates over-broad covers and tests it.

- High: [gc.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/gc.md:138) explicitly allows an unbounded pinned overlay. A broad view can pin beyond the 10k hot bound and blow the working-set gate. Fix: define per-view/global pinned ceilings and failure behavior.

- High: [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:150) supports kind:5 `a` tags, but [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:78) tombstones only by `target_event_id`. Deletes by address that arrive before the target, or before a later replacement, will not suppress reinsert. Fix: add address tombstones keyed by `(pubkey, kind, d-tag)` and tests for `a`-tag delete-before-event.

- High: [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:43) makes `Tombstoned` require `target_kind5_id: EventId`, but [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:85) allows NIP-40/Admin tombstones with no kind:5 id. Fix: use `kind5_event_id: Option<EventId>` plus `origin`, or return the tombstone row.

- Medium: [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:82) says `idx_author_kind` uses `created_at_be`, while [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:12) says `created_at_desc_be`. [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:83) also orders `idx_kind_dtag` differently than [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:13). Fix: make `keys.md` the source of truth and update the master doc.

- Medium: [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:67) names `scan_by_kind_dtag` and authoritative cache-coverage queries, but [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:162) only has `get_param_replaceable`, and [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:226) has no `coverage()` method. Fix: align the trait and docs; add `Coverage` to the trait if planner completeness depends on it.

- Medium: [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:150) and [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:185) define empty `kinds` as “any kind, newest-first,” but [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:38) and [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:66) order by kind before timestamp. Fix: disallow empty kinds, merge per-kind cursors, or add author/time and global/time indexes.

- Medium: [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:20) keys watermarks as `filter_hash || relay`, but [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:228) requires `list_watermarks_for_relay`. That scan is O(all watermarks). Fix: add a relay-first secondary or change the primary key order.

- Medium: [watermarks.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/watermarks.md:148) writes migration data, then [watermarks.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/watermarks.md:150) writes `_meta` separately. Crash between them reruns a committed migration. Fix: commit migration writes and meta version in the same LMDB transaction; add crash/retry tests.

- Medium: [tests.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/tests.md:56) defers the “single insert path” enforcement test to v1.x. That is an M3 invariant. Fix: make it an M3 gate or remove the claim.

- Low: [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:1) is 312 LOC, over the 300-line soft limit, and [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:61) claims every file is ≤300. Fix: split support types into `types.md` or shorten.

No `TODO`/`FIXME`/`unimplemented!` markers in the six new docs. The `nostr-lmdb` choice is directionally honest, but only if the atomic transaction/open-env question is resolved before implementation.
tokens used
107,658
**Findings**

- High: [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:91) promises primary + secondary atomicity in one `RwTxn`, but [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:218) leaves shared `nostr-lmdb` environment access unresolved. Fix: make env/txn sharing a blocking ADR before M3 implementation, or specify crash-recovery/two-phase semantics.

- High: [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:235) exposes only raw `claim(claimer, ids)`, while [gc.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/gc.md:13) relies on bounded `open_view_cover` from the composite reverse index. That does not enforce D2 through the trait. Fix: add claim budget/cover validation or a view-cover registration API that rejects/truncates over-broad covers and tests it.

- High: [gc.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/gc.md:138) explicitly allows an unbounded pinned overlay. A broad view can pin beyond the 10k hot bound and blow the working-set gate. Fix: define per-view/global pinned ceilings and failure behavior.

- High: [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:150) supports kind:5 `a` tags, but [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:78) tombstones only by `target_event_id`. Deletes by address that arrive before the target, or before a later replacement, will not suppress reinsert. Fix: add address tombstones keyed by `(pubkey, kind, d-tag)` and tests for `a`-tag delete-before-event.

- High: [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:43) makes `Tombstoned` require `target_kind5_id: EventId`, but [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:85) allows NIP-40/Admin tombstones with no kind:5 id. Fix: use `kind5_event_id: Option<EventId>` plus `origin`, or return the tombstone row.

- Medium: [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:82) says `idx_author_kind` uses `created_at_be`, while [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:12) says `created_at_desc_be`. [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:83) also orders `idx_kind_dtag` differently than [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:13). Fix: make `keys.md` the source of truth and update the master doc.

- Medium: [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:67) names `scan_by_kind_dtag` and authoritative cache-coverage queries, but [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:162) only has `get_param_replaceable`, and [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:226) has no `coverage()` method. Fix: align the trait and docs; add `Coverage` to the trait if planner completeness depends on it.

- Medium: [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:150) and [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:185) define empty `kinds` as “any kind, newest-first,” but [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:38) and [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:66) order by kind before timestamp. Fix: disallow empty kinds, merge per-kind cursors, or add author/time and global/time indexes.

- Medium: [keys.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/keys.md:20) keys watermarks as `filter_hash || relay`, but [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:228) requires `list_watermarks_for_relay`. That scan is O(all watermarks). Fix: add a relay-first secondary or change the primary key order.

- Medium: [watermarks.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/watermarks.md:148) writes migration data, then [watermarks.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/watermarks.md:150) writes `_meta` separately. Crash between them reruns a committed migration. Fix: commit migration writes and meta version in the same LMDB transaction; add crash/retry tests.

- Medium: [tests.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/tests.md:56) defers the “single insert path” enforcement test to v1.x. That is an M3 invariant. Fix: make it an M3 gate or remove the claim.

- Low: [trait.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb/trait.md:1) is 312 LOC, over the 300-line soft limit, and [lmdb-schema.md](/Users/pablofernandez/Work/nostr-multi-platform/docs/design/lmdb-schema.md:61) claims every file is ≤300. Fix: split support types into `types.md` or shorten.

No `TODO`/`FIXME`/`unimplemented!` markers in the six new docs. The `nostr-lmdb` choice is directionally honest, but only if the atomic transaction/open-env question is resolved before implementation.
