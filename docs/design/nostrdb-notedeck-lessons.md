# Design Note: Lessons from nostrdb + notedeck

> **Status:** Draft
> **Date:** 2026-05-18
> **Scope:** Architecture and implementation lessons from Damus's `nostrdb` (LMDB-backed Nostr event store, ~strfry-derived) and `notedeck` (multi-column Rust Nostr client built on nostrdb). Companion to `docs/design/ndk-applesauce-lessons.md` which covers the TypeScript NDK + Applesauce projects.

## 1. Purpose

NDK and Applesauce taught NMP what a well-shaped reactive Nostr client API looks like. nostrdb and notedeck teach NMP what a battle-tested **Rust** Nostr client runtime looks like — specifically, a Rust client that has already faced the multi-account, multi-relay, multi-window, compaction, retry, and discovery problems we are about to face.

Both Damus codebases are production. nostrdb is LMDB-backed and intentionally a port of strfry's storage layout (the fastest Nostr relay). Notedeck is the TweetDeck-style desktop/Android client that consumes nostrdb directly. Combined ≈ 30k LOC of Rust focused on exactly the substrate we are building.

This note distills what to **adopt directly**, what to **consider and possibly defer**, what to **intentionally diverge on**, and a few specific decisions for the milestones in `docs/plan.md`.

## 2. Lessons from nostrdb (storage)

### 2.1 Zero-copy mmap'd note layout

nostrdb stores events in a custom in-memory representation that enables zero-copy and O(1) access to all fields, then memory-maps the events inside LMDB. The layout is intentionally similar to flatbuffers but custom for Nostr events. The README's "unfairly fast" framing is not hyperbole — this is the primary reason nostrdb dominates alternatives.

**Lesson for NMP M3 (LMDB):** when we wire LMDB, do not naively `bincode::serialize(&event)` into the value bytes. Reads will dominate, and a packed-layout-with-offsets pays for itself almost immediately. The engineering cost is non-trivial but well-bounded.

The simpler alternative — and the one I'd recommend for v1 — is to **depend on `nostrdb-rs` (the Rust binding) directly** as our `EventStore` backend, rather than reimplementing strfry's layout. nostrdb is BSD-licensed, mature, and the team that built it is responsive. We get the performance without owning the storage engine. See §5 for the decision question.

### 2.2 Separable mutable metadata table

Events themselves are immutable once written, but per-note metadata (reaction counts, reply counts, thread-aggregate counts, thread-root totals, NIP-04 conversation pointers) is stored in a separate fixed-format TV (tag, value) table next to the event. Updates are race-safe inplace `memcpy` + atomic poke, sorted for binary-search lookups.

**Lesson for NMP:** our `Projections` cache (per `reactivity.md` §6 — author_display, reaction_summary, zap_total, reply_count) plays exactly this role in memory. nostrdb persists the equivalent. Persisting projections lets us survive restarts without recomputing aggregates from scratch.

**Roadmap impact:** M3 should include "persisted projection cache" alongside LMDB. Specifically, `Projections` becomes a sub-database keyed by `(namespace, key)` with serialized payload. Restart loads it; new events update it inplace. This is small additional work and removes a startup-cost cliff.

### 2.3 Streaming queries via visitor callbacks (`ndb_query_visit`)

Instead of `query(filter) -> Vec<Note>`, nostrdb provides `ndb_query_visit(filter, visitor)` where `visitor(result) -> Continue | Stop`. No result buffer is allocated; the visitor can stop early; the same query engine drives both APIs. Pure CPU win for large filtered scans.

**Lesson for NMP:** our `ViewModule::open()` does an initial `recompute_full` reading from the store. For large initial scans (a Timeline view over 1000 authors), this should be visitor-based so we can stop at the view's `limit` without ever materializing the full result.

**Roadmap impact:** M3 EventStore trait should expose both `query(filter) -> Vec` and `query_visit(filter, visitor)`. Visitor variant becomes the default for view-internal scans. Cheap, almost-free design win.

### 2.4 Plain subscription primitive

nostrdb's subscription API is intentionally low-level: `ndb_subscribe(filters) -> subid`, `ndb_wait_for_notes(subid, capacity)`, `ndb_unsubscribe(subid)`. No reactive layer; the consumer polls. Higher-level abstractions live in notedeck.

**Lesson for NMP:** our `ViewModule` abstraction sits much higher than this. But the underlying store-to-actor wakeup mechanism should be similar: when an event is inserted, any subscriptions whose filter matches get a one-shot "you have new notes" signal. Today our reverse index does this synchronously; nostrdb's queue-based approach is essentially the same shape decoupled.

### 2.5 The build-vs-depend question for the event store

nostrdb already exists, is fast, is Rust-callable via `nostrdb-rs`, and is being actively maintained by Damus. It would be reasonable for NMP to:

- **Option A:** Depend on `nostrdb-rs` as the LMDB-backed `EventStore` implementation. Get strfry-class storage performance for free. Lose tight control over storage semantics; gain a high-quality dependency.
- **Option B:** Implement our own LMDB-backed `EventStore` from scratch using `heed` or `lmdb-zero`. Full control. Reinvent strfry's wheel.
- **Option C:** Use `nostr-lmdb` from the `rust-nostr` workspace, which is more naive than nostrdb but already in the dependency tree we depend on.

**Recommendation:** revisit this at the start of M3. Option A (nostrdb-rs) is probably the right move for v1 — we'd be picking up battle-tested code from a team that has demonstrated they care about performance. The risk is API surface mismatch with our `EventStore` trait, but nostrdb's API is shape-compatible (filters, subscriptions, inserts).

This is a deferred decision; it does not block earlier milestones.

## 3. Lessons from notedeck (subscription runtime)

Notedeck has solved most of the problems we're about to solve in M2 (subscription compilation), M8 (multi-session), and M5 (NIP-42 auth lifecycle). The patterns below are worth porting almost verbatim.

### 3.1 `SubKey`: hashed typed stable identity

```rust
pub struct SubKey(u64);

impl SubKey {
    pub fn new(value: impl Hash) -> Self { /* hash anything */ }
    pub fn builder(seed: impl Hash) -> SubKeyBuilder { /* incremental */ }
}

pub struct SubKeyBuilder { /* keeps a hasher */ }
impl SubKeyBuilder {
    pub fn with(mut self, part: impl Hash) -> Self { /* fold in another part */ }
    pub fn finish(self) -> SubKey { /* finalize */ }
}
```

Stable identity for a logical subscription, constructed by hashing typed tuples. Pattern is borrowed from `egui::Id`. No string formatting, no allocation, comparable, hashable.

**Lesson for NMP:** our `ViewId` is currently runtime-allocated by the platform. Adopting hashed stable keys gives identity stability across restarts and across processes — useful for the action ledger ("this ledger row's view is the timeline I had open before reboot"), for diagnostics ("this `SubKey` corresponds to the thread for event X"), and for deterministic tests.

**Roadmap impact:** M2 should introduce `SubKey` (or `ViewKey`, name TBD) as the stable identity primitive alongside the runtime `ViewId`. `ViewId` stays as the FFI token; `SubKey` is the semantic identity.

### 3.2 `(owner, key, scope)` triple with multi-owner sharing

```rust
pub struct ScopedSubIdentity {
    pub owner: SubOwnerKey,   // UI lifecycle anchor (one per route/view instance)
    pub key: SubKey,          // logical subscription identity
    pub scope: SubScope,      // SubScope::Account | Global
}
```

Multiple owners (UI instances) can attach to the same `(scope, key)`. The runtime keeps the live wire sub alive while any owner is attached. Drops the wire sub when the last owner leaves.

**Lesson for NMP:** our ADR-0005 refcounted wrappers do this implicitly — the platform wrapper increments a refcount per pubkey, dispatches `OpenView`/`CloseView`. Notedeck makes ownership explicit with a named `SubOwnerKey`. This explicit ownership is genuinely useful for:

- **Diagnostics:** "which UI is keeping this subscription alive?"
- **Tests:** simulate owner lifecycle without an actual UI.
- **Hot-restart in dev:** if the UI reloads, owners attached to the old session are cleaned up explicitly.

**Roadmap impact:** M2 should adopt the `(owner, key, scope)` triple as the kernel's subscription identity, with explicit named owners visible in ADR-0007 diagnostics.

### 3.3 `set_sub` (upsert) vs `ensure_sub` (create-if-absent)

Different APIs for different semantics:

- `set_sub(identity, config)` — upsert: replace the desired state for this identity. Use when filters can change mid-life (search query updates as user types).
- `ensure_sub(identity, config)` — create-if-absent: do nothing if it already exists. Use when the filter is stable for the lifetime (a thread view by root id).

**Lesson for NMP:** our current `OpenView` is implicitly upsert. Adding a `dispatch(EnsureView { id, spec })` variant prevents accidental filter replacement on re-mount (a real bug class — re-mounting an avatar component should not clobber the existing profile subscription).

**Roadmap impact:** M2 adds the upsert / ensure distinction in the `OpenView` action.

### 3.4 `SubScope::Account | Global` with switch-away/restore

```rust
pub enum SubScope {
    Account,   // resolved to a concrete pubkey at runtime
    Global,    // not tied to any account
}
```

On account switch: live subs for the old account are unsubscribed; their desired state is **retained**. On switch back: live subs are restored from the retained desired state. If owners are dropped while away, nothing is restored.

**Lesson for NMP:** this is the M8 (multi-session) behavior we need. Notedeck has worked out the details — desired-state retention is the GC root, ownership is the lifetime root, switch-away cleans wire state but not desired state. We should adopt this verbatim.

**Roadmap impact:** M8 implements `SubScope` and the switch-away/restore lifecycle. Add account-switch tests covering: owner dropped while away (no restore), owner held while away (restore on switch-back), owner held + new owner attached while away (still no live sub, but no double-restore on switch-back).

### 3.5 Subscription compaction at the relay layer

`enostr/src/relay/compaction.rs` (~1340 LOC) packs multiple logical OutboxSubscriptions into as few wire REQs as possible per relay, respecting:

- per-relay `max_subs` capacity (negotiated or default).
- per-relay `json_limit` (max REQ JSON size).
- `SubPassGuardian` capacity tokens (free passes available; compaction frees more by combining).
- A queue when capacity is exhausted.
- Revocation when capacity changes (e.g., relay reconnects with different limits).
- Cost-ordered downgrade for limit shrinks.

**Lesson for NMP M2 (subscription compilation):** compaction is the wire-side counterpart to subscription compilation. Our planner compiles logical interests → per-relay plans; compaction packs per-relay plans → wire REQs. Notedeck does both; the compaction implementation is heavyweight but proven.

**Roadmap impact:** M2 should include a compaction layer between the planner and the WebSocket. Initial implementation can be naive ("one REQ per logical sub, no packing"); compaction lands when we hit relay `max_subs` limits in real use. The architecture must accommodate it from the start.

### 3.6 Transparent retry layer: request legs

`enostr/src/relay/transparent.rs` (~1040 LOC) tracks **active legs** per logical request. A "leg" is one relay's instance of a request. Failure on one relay doesn't fail the logical request — another leg can serve it. Each `OutboxSubId` may be queued for retry, active on a relay, or absent (but never both queued and active).

**Lesson for NMP:** we have three concepts that should not collapse:

| Concept | Lives in | Purpose |
|---|---|---|
| Logical subscription | kernel actor | "the timeline of these authors" |
| Wire subscription | per-relay layer | "the REQ I sent on this socket" |
| Request leg | retry layer | "this attempt to serve the logical sub on this relay" |

**Roadmap impact:** when we add retries (M2 reconnect-resume; expanded in M5 NIP-42 backoff), the leg abstraction is the right shape. Don't try to model retry inside the wire-sub layer.

### 3.7 `RelaySpec` with explicit NIP-65 markers

```rust
pub struct RelaySpec {
    pub url: NormRelayUrl,
    pub has_read_marker: bool,
    pub has_write_marker: bool,
}

impl RelaySpec {
    pub fn is_readable(&self) -> bool { !self.has_write_marker } // only "write" relays are not readable
    pub fn is_writable(&self) -> bool { !self.has_read_marker } // only "read" relays are not writable
}
```

Per NIP-65: both markers set → both off (the relay is for both purposes). Notedeck encodes this directly. Set arithmetic uses URL equality only (markers are metadata).

**Lesson for NMP:** trivially adopt this exact shape in `nmp-nip65` (M2).

### 3.8 Typed publish APIs (Accounts vs Explicit)

```rust
ExplicitPublishApi::publish_note(&note, relays)        // explicit relay set
AccountsPublishApi::publish_note(&note)                 // selected account's write relays
```

Two APIs, two intents. No way to accidentally mix them — there is no "publish_note(note, optional_relays)" footgun. The compiler enforces intent.

**Lesson for NMP:** matches our doctrine D3 perfectly. Our `nmp-nip01::SendNoteActionModule` should split similarly: the default `SendNote { content }` action routes to the account's write relays (and `p`-tagged inboxes); an `SendNoteToRelays { content, relays }` variant exists for explicit overrides and is debug-flagged in the action ledger.

**Roadmap impact:** M6 (write path) should adopt the typed split from day one.

### 3.9 OneshotApi vs ScopedSubApi (transient vs durable)

Two top-level APIs:

- **OneshotApi.oneshot(filters)** — fire a one-shot REQ that CLOSEs on EOSE. For transient reads (a single profile lookup, a one-shot search). No durable identity.
- **ScopedSubApi.set_sub / ensure_sub / clear_sub / drop_owner_slot** — durable scoped subscriptions with the `(owner, key, scope)` model from §3.2.

**Lesson for NMP:** different lifetimes, different ergonomics. Our current model has only durable subscriptions (every `OpenView` is durable until `CloseView`). Adding a oneshot API for transient operations — "fetch this profile once" — is a small, useful addition.

**Roadmap impact:** M2 adds a `dispatch(OneshotRead { filter, callback_id })` primitive alongside the durable `OpenView`. Used internally by the fallback loader and by certain actions.

### 3.10 `UnknownIds` + oneshot discovery for missing references

When a note arrives referencing a pubkey or event id we don't have cached, notedeck records it. A periodic batched one-shot REQ goes out to fill the gaps. Per-id deduplication; tracking first-seen / last-updated; user-clearable.

**Lesson for NMP:** this is exactly the "fallback loader" pattern from our spec. Notedeck shows the production shape: separate `UnknownIds` state, batched discovery, deduplicated.

**Roadmap impact:** M2 implements `UnknownIds` as a kernel sub-system; the planner consults it on view-open and triggers periodic batched discovery via the oneshot API.

### 3.11 `Nip51SetCache` and `UnifiedSubscription { local, remote }`

NIP-51 lists (mute lists, pinned-event lists, follow sets) use a per-cache type combining:

- A local nostrdb subscription (cache contents at this moment).
- A remote outbox subscription (new updates from relays).

Both live in a single `UnifiedSubscription { local, remote }` wrapper.

**Lesson for NMP:** our `ViewModule` + planner abstraction does this implicitly — a view opens, the planner sends REQs, results land in the store, the view rebuilds. Notedeck names the pair explicitly. Names matter for diagnostics.

**Roadmap impact:** ADR-0007 diagnostics should distinguish "local cache hit served this view" from "remote subscription is delivering events to this view" — both are live, but they're different facts.

### 3.12 `TimeCached<T>` generic TTL primitive

```rust
pub struct TimeCached<T> {
    last_update: Instant,
    expires_in: Duration,
    value: Option<T>,
    refresh: Rc<dyn Fn() -> T>,
}

impl<T> TimeCached<T> {
    pub fn needs_update(&self) -> bool { /* ... */ }
    pub fn update(&mut self) { /* ... */ }
    pub fn get_mut(&mut self) -> &T { if self.needs_update() { self.update(); } /* ... */ }
}
```

Generic TTL cache. Holds a value, a refresh closure, an expiration. Auto-refreshes on read past expiry.

**Lesson for NMP:** view warmth is one specific use of this pattern (30s grace). NIP-05 verification results, relay capability probes, NIP-77 watermark refreshes — all want similar TTL semantics. A small reusable primitive is worth having.

**Roadmap impact:** add `TimeCached<T>` to `nmp-core::util` early. Use it in M2 (capability probes), M3 (NIP-05 verification), M4 (watermark refresh).

### 3.13 Per-account subscription set: `AccountNdbSubs`

When an account is loaded, a per-account set of nostrdb subscriptions is built: profile, contacts, mailboxes, mutes, etc. — the "active account's reads" cluster. Lives on the account record; switches with the account.

**Lesson for NMP M8:** this is the data structure to clone for our per-account view-spec set. When the active account switches, the per-account subscription set is reattached as the active one; the prior account's set sleeps until switched back to.

## 4. Items to adopt directly (concrete roadmap impacts)

Concrete commitments by milestone:

### M2 (outbox + subscription compilation)

- **Adopt `SubKey` hashed typed identity** (§3.1) alongside the runtime `ViewId`.
- **Adopt `(owner, key, scope)` triple** with explicit named owners (§3.2).
- **Adopt `set_sub` vs `ensure_sub` semantics** (§3.3) in the OpenView action.
- **Adopt `SubScope::{Account, Global}`** (§3.4) even before full multi-session lands in M8; account-scope behavior degenerates trivially when there's one account.
- **Adopt typed publish APIs** (§3.8) from M2's compose-bound work; carry forward to M6.
- **Implement `OneshotApi`** (§3.9) for transient reads.
- **Implement `UnknownIds`** (§3.10) for missing-reference discovery.
- **Adopt explicit `RelaySpec` with NIP-65 markers** (§3.7) in `nmp-nip65`.
- **Architect compaction layer** (§3.5) into the per-relay stack, even if the initial implementation is naive ("one REQ per sub, no packing"). The layer must exist so packing lands as an optimization, not a rewrite.
- **Architect request-leg abstraction** (§3.6) at the retry layer.

### M3 (LMDB + persistence)

- **Decide nostrdb-rs vs heed-from-scratch vs nostr-lmdb** (§2.5). Recommendation: nostrdb-rs.
- **Persist projection cache** alongside events (§2.2).
- **EventStore trait exposes `query_visit`** (§2.3) for visitor-based queries.

### M4 (negentropy)

- **`TimeCached<T>` for relay-capability probes** (§3.12).

### M6 (write path)

- **Typed publish APIs** (§3.8) — already adopted in M2 architecturally.
- **`UnifiedSubscription { local, remote }`** naming for diagnostics (§3.11).

### M8 (multi-session)

- **Per-account subscription set** modeled on `AccountNdbSubs` (§3.13).
- **`SubScope::Account` switch-away/restore semantics** (§3.4) fully exercised with tests.

## 5. Items to consider, possibly defer or skip

| Item | Status | Note |
|---|---|---|
| Custom packed event layout (§2.1) | Defer | Only worth it if we DON'T pick nostrdb-rs. If we do, we get this for free. |
| nostrdb's metadata table on-disk format (§2.2) | Defer / partial adopt | Persist projections, but don't necessarily copy the exact TV format. |
| nostrdb visitor C-ABI shape (§2.3) | Adopt the semantic, not the ABI | Visitor pattern in our Rust trait, not C function pointer. |
| egui-style `SubKeyBuilder` exact API (§3.1) | Adopt verbatim | Trivial; just port it. |

## 6. Items where NMP intentionally diverges

| Item | Notedeck approach | NMP approach | Why |
|---|---|---|---|
| UI framework | egui (immediate mode, host-Rust) | SwiftUI / Compose / iced / wasm-bound (retained mode, native) | D5: native UX quality is the invariant. egui in production iOS would be a doctrine violation. |
| State ownership | App holds nostrdb txn + state; egui re-renders synchronously | Actor owns all state; native gets snapshots / deltas via FFI | Per ADR-0009 + ADR-0005. Notedeck is single-process Rust; NMP crosses FFI to native UIs. |
| Subscription dispatch | Direct `set_sub` calls in app code | `dispatch(OpenView)` action through the kernel | Per doctrine D0: native code is dumb; intent crosses as actions. |
| Cross-platform | One Rust binary with egui everywhere | One Rust kernel + four native shells | Per the framing concern. Notedeck chose simplicity; we chose native UX. |
| Persistence | nostrdb (LMDB-backed, may adopt as backend) | EventStore trait with pluggable backends | Different consumers want different backends (in-memory for tests, LMDB native, IndexedDB web). |

These aren't disagreements with notedeck — they're consequences of different framing concerns. We're optimizing for the same correctness invariants via different mechanisms.

## 7. Summary

nostrdb gives us a strong recommendation to **depend on it, not reimplement it**, for M3's LMDB layer. The performance is real, the API is shape-compatible with our `EventStore` trait, and we get free maintenance from a team that cares.

Notedeck gives us **a proof-of-concept Rust implementation** of most of what M2 (subscription compilation), M5 (NIP-42 retry layer), M6 (typed publish), and M8 (multi-session) need. Several specific patterns (`SubKey`, `(owner, key, scope)`, `set_sub` vs `ensure_sub`, `SubScope::Account` switch-away, compaction-and-transparent-retry split, `UnknownIds`, `TimeCached`) should be adopted as-is.

Combined, this lets M2–M8 ship with significantly less invention than the prior plan implied. The architectural commitments in our ADRs are still ours — we are not adopting notedeck's egui or single-process model — but the runtime mechanics underneath them have prior art we can lean on.
