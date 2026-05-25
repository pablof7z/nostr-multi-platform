# Plan — Indexer-Republish Pipeline (default-on, opt-out)

**Status:** design only — not yet on a branch. Awaiting user confirmation on the open questions in §10.

---

## 1. Summary

When NMP receives a NIP-01 replaceable Nostr event from a non-indexer relay, forward the same event to every currently-connected indexer relay. The forward is a plain `["EVENT", <event>]` frame: no re-signing, no tag mutation. This is a passive contribution to the gossip network — if we fetched a fresher kind:0 from an author's own write relay, we keep the indexer (purplepag.es, etc.) honest by pushing the newer copy.

**Why it pays:** indexers are how unrelated apps find a pubkey's profile / contact list / inbox-relay list. A stale indexer answer poisons cold-start for every app that depends on it. NMP already opens a connection to indexers (lane discriminator `RelayRole::Indexer`, `crates/nmp-network/src/role.rs:15`); we are paying the socket cost; the marginal cost of one extra `EVENT` frame per replaceable arrival is near-zero.

**Scope discipline:** this is a passive replication contribution. It is NOT a substitute for outbox publishing (`PublishEngine`), it does NOT track acks, it does NOT retry on `OK: false`. Fire and forget.

---

## 2. Kinds in scope

The exact set NIP-01 calls "replaceable":

| Kind | Why |
|---|---|
| 0 | Profile metadata. Universal indexer payload. |
| 3 | Contact list. Used by indexers + outbox resolvers everywhere. |
| 10000–19999 | NIP-01 replaceable range. kind:10002 (NIP-65 mailbox) and kind:10050 (NIP-17 DM-relay list) are the two production-relevant members today; new NIPs land in this range. |

**Predicate already exists** — `RawEvent::is_replaceable()` at `crates/nmp-store/src/types/events.rs:50`:
```rust
self.kind == 0 || self.kind == 3 || (10_000..20_000).contains(&self.kind)
```
Reuse this; do not re-encode.

**Parameterized replaceable (30000–39999) is NOT in v1.** Long-form (kind:30023), labeled lists (kind:30000) etc. are user content with per-`d` addressability; the gossip story is different (indexers don't uniformly mirror them, and the dedup key needs `d` tag normalization). Defer to a v2 follow-up if telemetry shows demand.

**Ephemeral (20000–29999) is NOT in scope.** By NIP definition the relay does not store it; republishing is pointless.

---

## 3. Architecture

### 3.1 Where the hook lives

**Hook seam:** the existing **raw signed-event observer** registry at `crates/nmp-core/src/actor/commands/raw_event_observer.rs:60`. It already:

- fires only after the kernel's Schnorr + id-hash gate (`verify_and_persist` at `crates/nmp-core/src/kernel/ingest/mod.rs:488`),
- gates on `raw_tap_should_fire` (`Inserted | Replaced | Duplicate | Ephemeral` — `kernel/ingest/mod.rs:60`),
- delivers `(raw: &RawEvent, relay_url: &str)` — both the verified event AND the delivering relay URL,
- supports a `KindFilter` so a registration can request only kinds 0/3/10000–19999,
- is D0-clean (no protocol nouns; generic capability).

**Do NOT modify `kernel/ingest/mod.rs`'s wildcard arm or `handle_event`.** Republish is networking policy, not substrate state. Putting it in `nmp-core` violates D0 — and the codebase has a fresh precedent for ripping exactly that kind of leak out (WIP.md 2026-05-25 kind:10002 ingest leak deletion: `kernel/ingest/relay_list.rs` was moved to a substrate parser registered by `nmp-app-template`).

### 3.2 Where the code lives

A new module under `crates/nmp-app-template/src/indexer_republish.rs`, registered in `register_defaults()` (same file the `Kind10002Parser` and DM-inbox projection are wired from — `crates/nmp-app-template/src/lib.rs:223`).

**Why app-template and not a new crate:** the feature is composition, not protocol. It composes three already-public seams (`raw_event_observer`, `IndexerRelaysSlot`, `Pool`). A standalone `nmp-indexer-republish` crate is also defensible — see §10 Q1. The default recommendation is app-template module because it carries zero new crate ceremony and is delete-able as one file if it turns out to be a bad idea.

### 3.3 The Pool-access problem

The observer fires on the actor thread but the observer struct has no path to the `Pool` today — `Pool` is local to `run_actor` (`crates/nmp-core/src/actor/mod.rs:1086`).

**Solution: a new `PoolSlot` typed slot,** following the existing pattern (`RoutingTraceSlot` at `crates/nmp-core/src/slots.rs:50`). The actor publishes its `Pool` clone (it's `Arc<Mutex<PoolInner>>` inside, cheap to clone — see `crates/nmp-network/src/pool/mod.rs:92`) into the slot right after construction, in the same block as the routing-trace publication (`actor/mod.rs:1127`). `NmpApp` gains a `pool_handle()` accessor; the republish observer reads it lazily on each fire.

Rejected alternatives:

- **`ActorCommand::RepublishVerbatim { event_json, exclude_url }`** — heavier, adds a command variant for a single observer use case, requires an `ActorCommand::Protocol(...)` extension. Not warranted for a fire-and-forget passive feature.
- **Reuse `PublishEngine`** — wrong abstraction. Engine is for OWN events with retries, ack tracking, NIP-65 outbox resolution. Verbatim forward needs none of those.

### 3.4 Verbatim is structurally approximate

The original UTF-8 bytes received from the upstream relay are gone at `serde_json::from_str` (`kernel/ingest/mod.rs:142`). What we forward is `serde_json::to_string(&raw_event)` over the verified `RawEvent`. The Schnorr signature still verifies (id-hash is over canonical NIP-01 serialization), but byte-for-byte identity with the upstream relay's wire payload is lost.

**This is fine** — what matters cryptographically is that the indexer can verify the signature, which it can. Flagged in §10 Q5 in case the user expected literal-byte preservation.

---

## 4. Relay role model — no struct changes needed

Indexer identity is already a first-class concept:

- `RelayRole::Indexer` enum variant — `crates/nmp-network/src/role.rs:15`.
- `IndexerRelaysSlot` typed slot — `crates/nmp-core/src/slots.rs:178` (re-exported from `kernel::relay_projection`).
- Slot is populated from `RelayEditRow.role` strings via `crate::actor::has_role(&r.role, "indexer")` at `crates/nmp-core/src/kernel/identity_state.rs:343-353`. Sole writer is the kernel reducer (D4); readers `.lock()` + `.as_slice()`.
- The composite role token `"both,indexer"` is supported (`crates/nmp-core/src/actor/relay_roles.rs:24`), so a single relay can be both content + indexer.

The republish observer reads `IndexerRelaysSlot` on each fire, snapshots the URL list, and uses that as both (a) the set to forward TO and (b) the membership test for "did this arrive from an indexer?" (loop-prevention §7).

---

## 5. Deduplication

Per-session, in-memory, bounded.

**Data structure:** `Mutex<lru::LruCache<(EventId, RelayUrl), ()>>` with a cap of **4096 entries** (~280 KB peak: 32B id + ~40B url + overhead). Insert-on-republish; LRU evicts the oldest on overflow.

**Why LRU and not bloom:** the dedup goal is "don't waste sockets on within-session resends"; perfect dedup is unnecessary. LRU gives O(1) lookup, exact membership, and a hard memory cap. Bloom would add false-negative risk for no real benefit (false-positives are tolerable but indistinguishable from genuine eviction).

**Why not persist:** the cost of a duplicate forward across process restarts is one wasted `EVENT` frame per (event, indexer) pair. Indexers de-dup on `id`. Persistence buys nothing and would couple the feature to the LMDB store.

**Key shape:** `(EventId, RelayUrl)` is the natural unit — we may republish event X to indexer A but skip indexer B if B sent us X originally (cross-indexer disabled by default — see §7).

---

## 6. Config / feature flag

A new field on whatever app-template config struct `register_defaults` already consumes — if no struct exists today, a single `IndexerRepublishConfig` co-located with the module:

```rust
pub struct IndexerRepublishConfig {
    /// Master switch. Default true.
    pub enabled: bool,
    /// Allow indexer→indexer propagation. Default false.
    /// When true, an event received from indexer A is forwarded to
    /// indexers B, C, … (still never back to A).
    pub allow_cross_indexer: bool,
    /// Per-kind opt-out. Empty = republish all kinds in scope.
    pub disabled_kinds: BTreeSet<u32>,
}
```

**Defaults:** `enabled: true`, `allow_cross_indexer: false`, `disabled_kinds: {}`.

**Why not `dispatch_action` runtime toggle:** the feature is a boot-time composition concern, not a user-intent action. D11 reserves `dispatch_action` for verbs the user triggers; a config flag the host sets at registration time is the right grain. If telemetry later motivates a runtime kill-switch, add a `dispatch_action("nmp.republish.set_enabled", { bool })` then — it's additive.

---

## 7. Loop prevention — exact rules

On each observer fire `(raw, delivering_relay_url)`:

1. **Filter by kind**: `raw.is_replaceable()` AND `!config.disabled_kinds.contains(&raw.kind)`. Otherwise skip.
2. **Snapshot indexers**: read `IndexerRelaysSlot` once. Let `I = HashSet<RelayUrl>`.
3. **Decide source class**:
    - If `delivering_relay_url ∈ I` → source is indexer. If `!config.allow_cross_indexer` → **skip entirely**. Otherwise the forward set is `I \ {delivering_relay_url}`.
    - If `delivering_relay_url ∉ I` → source is non-indexer. Forward set is `I` in full.
4. **Dedup check**: for each `target ∈ forward_set`, skip if `(raw.id, target) ∈ dedup_lru`. Otherwise insert and forward.

The rule "never republish to the source URL" is structural: the source URL is excluded from the forward set BEFORE the dedup check. This guarantees indexer→same-indexer is impossible regardless of `allow_cross_indexer`.

---

## 8. Implementation steps (in order, each one PR)

Total surface estimate: ~250–350 added LOC across all PRs, plus tests.

### PR 1 — Substrate slot for `Pool` (≤80 LOC + tests)

- Add `PoolSlot` type alias and constructor in `crates/nmp-core/src/slots.rs` (mirrors `RoutingTraceSlot`).
- Wire publication in `crates/nmp-core/src/actor/mod.rs:1127` block (right after the routing-trace publication).
- Add `NmpApp::pool_handle()` accessor in `crates/nmp-core/src/app.rs` (and re-export shape; verify against `nmp-ffi` to confirm no FFI surface change — this is Rust-only).
- Doctrine-lint: `PoolSlot` is substrate-grade (`nmp_network::Pool` is a transport-pool primitive, no protocol nouns).
- **No behaviour change** in this PR; just the new accessor.
- Tests: a doctrine-lint smoke + a unit test that a registered Rust observer can read `pool_handle()` from inside its callback.

### PR 2 — `indexer_republish` module + default registration (≤220 LOC + tests)

- New file `crates/nmp-app-template/src/indexer_republish.rs` with:
    - `IndexerRepublishConfig` struct,
    - `IndexerRepublishObserver` implementing `RawEventObserver`, holding `pool: PoolSlot`, `indexer_relays: IndexerRelaysSlot`, `dedup: Mutex<LruCache<…>>`, `config: IndexerRepublishConfig`,
    - the loop-prevention logic from §7 exactly.
- Wire in `crates/nmp-app-template/src/lib.rs:223` `register_defaults` with `KindFilter::from_kinds([0, 3])` plus the 10000–19999 range expanded — verify `KindFilter` supports ranges; if not, register two filters or extend `KindFilter`.
- Add `lru` crate to `nmp-app-template/Cargo.toml` (small dep — ~700 LOC, no transitive deps).
- Tests:
    - unit: forward set excludes source when source ∈ indexers,
    - unit: forward set is full indexer list when source ∉ indexers,
    - unit: dedup blocks second republish of same `(id, target)`,
    - unit: `allow_cross_indexer = false` short-circuits indexer source,
    - unit: disabled kind is skipped,
    - integration in `nmp-testing`: synthetic ingest of kind:0 from non-indexer URL → assert one `pool.send` per indexer URL with `["EVENT", …]` payload.

### PR 3 — host config plumbing (≤80 LOC)

- Expose `IndexerRepublishConfig` through whatever app-template config surface exists today (or accept a `Default::default()` if there isn't one yet — then this is a follow-up when the config struct lands).
- Add chirp-side config knob (probably in `apps/chirp/nmp-app-chirp` — verify where Chirp wires `register_defaults`). No UI change for v1; flag is `true` by default.
- Doc update in `docs/BACKLOG.md` post-v1 list: note that the runtime `dispatch_action` toggle is an additive follow-up.

---

## 9. What NOT to do

- **Do not** add the republish logic inside `kernel/ingest/mod.rs`. That is exactly the D0 violation the 2026-05-25 kind:10002 cleanup deleted (see WIP.md). The kernel does not name "indexer" as a routing policy; it names "indexer" only as a transport lane discriminator.
- **Do not** add a polling loop / timer that scans the store for "events to republish". Doctrine D8 forbids polling; the observer push is the only legal path.
- **Do not** route the forward through `PublishEngine`. Engine is for OWN events; verbatim forward must skip outbox resolution, ack tracking, and retries.
- **Do not** persist the dedup set. Per-session in-memory only — restart-cost is one wasted frame per pair; indexers de-dup on `id`.
- **Do not** create a new bespoke `nmp_app_*` C-ABI symbol. The deprecation calendar (PD-039) freezes the surface; this feature ships entirely in Rust composition and needs no FFI hop.
- **Do not** name the feature in `nmp-core`. `crates/nmp-core/` must contain zero references to "indexer republish" or "republish"; the substrate has no opinion about whether the host re-emits events.
- **Do not** re-sign or mutate tags. The whole point is verbatim forwarding; any mutation breaks the upstream signature.
- **Do not** forward kind:5 (deletion request) — out of scope. Out of scope by definition: not in the replaceable predicate.

---

## 10. Open questions for the user

**Q1.** App-template module vs. standalone `nmp-indexer-republish` crate?
- Module: less ceremony, deletable as one file, lives where the composition already is.
- Crate: more honest framework-thesis-wise (per-feature Layer-4 crate per `docs/architecture/crate-boundaries.md`), but no other app would import it.
- **Recommendation:** module in `nmp-app-template`. Promote to crate only if a second consumer appears.

**Q2.** Kind:3 traffic concern. Active timelines see kind:3 arrivals from many relays per minute. Republishing every kind:3 to every indexer multiplies that load. Options:
- (a) Accept it — indexers dedupe on `id`; the cost is socket bandwidth on the originating client.
- (b) Per-(author, kind:3) rate limit (e.g. one republish per author per hour, regardless of which indexer).
- (c) Opt-out kind:3 by default (`disabled_kinds: {3}`).
- **Recommendation:** start with (a); revisit if telemetry on chirp-tui's wire log shows it dominates outbound traffic.

**Q3.** Should `allow_cross_indexer` default to `true` or `false`?
- `false` (recommendation): safer, no risk of building cross-indexer loops if two indexers somehow chain back to us.
- `true`: better gossip behavior, but trusts the local dedup set absolutely.
- **Recommendation:** `false` default; flip after one release with telemetry showing dedup catches cross-indexer cycles.

**Q4.** `raw_event_observer` doesn't carry `InsertOutcome` — so we can't distinguish `Replaced` (we have a NEWER event than what the indexer might have — definitely worth forwarding) from `Duplicate` (we already had this; the indexer probably does too — wasted send). Two paths:
- (a) Accept the duplicate sends; let dedup-LRU absorb most; indexer-side dedup handles the rest.
- (b) Extend the observer signature to `(raw, relay_url, outcome)` — touches `raw_event_observer.rs` substrate seam, adds breaking change to one consumer (Chirp's existing observer, verify count).
- **Recommendation:** (a). Cleaner, no substrate churn.

**Q5.** "Verbatim" means "re-serialized canonical NIP-01 JSON of the verified `RawEvent`", not "byte-for-byte the upstream relay's wire bytes" (those are gone at decode time). Confirm this is acceptable. (Signature still verifies; behavior is indistinguishable to the indexer.)

**Q6.** kind:30000–39999 parameterized replaceable (long-form, labeled lists, etc.) — defer to v2 or include now? Recommendation: defer. The dedup key needs `d`-tag normalization, and the indexer-relevance argument is weaker (many indexers don't store parameterized replaceables).

---

## 11. Verification plan

- `cargo test -p nmp-app-template` — unit + integration tests added in PR 2.
- `cargo test -p nmp-testing --test doctrine_lint_smoke` — confirm no D0 token leak into `nmp-core`.
- `cargo test -p nmp-core --lib raw_event_observer` — confirm the existing observer-slot tests still pass after PR 1 adds `PoolSlot`.
- Manual: run chirp-tui against `wss://relay.damus.io` (content) + `wss://purplepag.es` (indexer); fetch a known kind:0 from damus; observe one `EVENT` frame to purplepag.es in `chirp-repl wire-log` (verify the wire-log surface name — substitute the equivalent observability hook if the name differs).
- No new bespoke FFI symbol; the existing `ffi-surface-freeze.sh` gate is a no-op for this feature.

---

## 12. Out of scope

- Runtime dispatch_action toggle (additive follow-up).
- Parameterized replaceables (30000–39999) — see Q6.
- Cross-session persistent dedup — Q4 / Q5 recommendation.
- Telemetry on republish counts (add only if Q2 / Q3 follow-up motivates it).
- Indexer auto-discovery (today: indexers come from `RelayEditRow.role = "indexer"` configured by the host; this plan does not change discovery).
