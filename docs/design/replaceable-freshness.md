# Design: Replaceable event freshness (F-TTL)

> **Status:** Implementation complete on `feature/f-ttl-impl`.
> **Audience:** kernel implementers, framework builders.
> **Related files:** `docs/builder-guide/21-framework-magic.md` (C0 framework magic contract), `docs/design/lmdb-schema.md` (storage schema).

## Overview

F-TTL (freshness time-to-live) is a lazy re-verification system for replaceable Nostr events. Instead of requiring apps to manually refresh profile data or relay lists at fixed intervals, the framework automatically manages when each replaceable event should be re-fetched from the network.

The core primitive is **`check_again_after`** — a per-replaceable-key timestamp that tracks "when should this kind:0/kind:10002/parameterized replaceable event be re-verified?"

## Lifecycle and TTL rules

**On request dispatch:**
- `check_again_after = now + INFLIGHT_GUARD_MS (3_600_000 = 1h)`
- Optimistic: if a REQ is already in-flight, do not issue another until 1 hour passes

**On EOSE (end-of-sync from relay):**
- `check_again_after = now + ttl_for_kind(kind)`
- Event is freshly verified; defer next check by the kind's configured TTL

**On insert/replace/duplicate (at event ingest):**
- `check_again_after = now + ttl_for_kind(kind)`
- Newly arrived or updated event; reset freshness timer

**On `claim_replaceable()` (app calls to re-fetch):**
- If `now > check_again_after`, enqueue the kind/pubkey/d_tag in the `pending_reverify` queue
- Caller sets `check_again_after = now`, triggering immediate re-verification REQ

## Replaceable kind ranges (NIP-01)

- **is_replaceable:** kinds 0, 3, 10000–19999 — keyed by `(kind, pubkey)`
- **is_addressable:** kinds 30000–39999 — keyed by `(kind, pubkey, d_tag)`

## Default TTLs

`ReplaceableTtlConfig` crate: `{ per_kind: BTreeMap<u32, Duration>, default: Duration }`

| kind | default TTL | reason |
|------|---|---|
| 0 (profile) | 1 hour | user metadata changes infrequently |
| 10002 (mailbox) | 6 hours | relay lists change moderately often |
| others (fallback) | 6 hours | conservative; apps can override per-kind |

TTL is stateless and configured at kernel startup; apps can customize via `NmpAppBuilder::with_replaceable_ttl_config()`.

## Storage: LMDB `replaceable_freshness` sub-database

**Key:** `kind[4B BE] || pubkey_bytes[32B] || d_tag_utf8` (d_tag omitted for regular replaceables)

**Value:** CBOR-encoded `{ check_again_after: u64 }` — unix milliseconds

**In-memory cache:** `HashMap<ReplaceableKey, u64>` loaded at startup; all writes go through the store

**Rationale:** Fast cache-hit for the hot path (`claim_replaceable`); disk persistence survives restarts; per-key granularity allows independent TTL management for each profile/mailbox/parameterized event

## The `pending_reverify` queue

When a `claim_replaceable(kind, pubkey, d_tag?)` call finds `now > check_again_after`:

1. Enqueue the key to `pending_reverify: VecDeque<ReplaceableKey>`
2. Write `check_again_after = now` (prevents duplicate REQs in the same second)
3. On next `pending_view_requests` tick, route the queued keys as REQs

**REQ structure:**

- **Regular replaceable:** `{ kinds: [kind], authors: [pubkey], limit: 1 }`
- **Parameterized replaceable:** `{ kinds: [kind], authors: [pubkey], limit: 1, #d: [d_tag] }`

**Sub ID mapping:** `reverify_subs: HashMap<String, Vec<ReplaceableKey>>` — tracks which keys are tied to which sub_id so EOSE updates the right TTL

## API surface

**Rust (internal):**
- `Kernel::claim_replaceable(kind: u32, pubkey: [u8; 32], d_tag: Option<String>, force: bool)`
  - `force == true` treats the stored `check_again_after` as `0` → always enqueues a re-verification
- `get_check_again_after(&key)` on the `EventStore` handle (test introspection)

**FFI (public, for apps):** force-refresh is a `force` argument on profile claims; event references use `nmp_app_resolve_ref` with a `shape` selector instead — there is no standalone refresh symbol.
- `void nmp_app_claim_profile(NmpApp* app, const char* pubkey, const char* consumer_id, int force)`
  - `force != 0` → forces immediate re-verification of the cached kind:0 profile
  - Pass `1` when the user explicitly opens / navigates to a profile or pulls to refresh; pass `0` for background / `.onAppear` claims
- `void nmp_app_resolve_ref(NmpApp* app, int namespace, const char* key, const char* consumer_id, int shape, int liveness)` (namespace=1 for events)
  - `key` is a lowercase 64-hex event-id, `"kind:pubkey:d"` naddr coordinate, or `"i:<external-id>"` NIP-73 ref — **not** a `nostr:` URI
  - `shape`: `2`=event.embed, `3`=event.raw
  - For cached `naddr` (addressable) identities the TTL gate runs automatically; for immutable event-id keys there is no TTL record

## Doctrine

Complies with all D-series constraints:

- **D1** (type-safe inserts): replaceable key format is compile-time verified
- **D3** (reactive routing): EOSE recomputes TTL; on-demand refresh re-routes
- **D4** (account scoping): TTL is per `(kind, pubkey, d_tag)` — account-switch invalidates old account's keys automatically
- **D6** (error closure): store errors mapped to diagnostics/logs, not FFI
- **D8** (observability): no hidden state; `check_again_after` is inspectable via kernel handle
- **D9** (testability): in-memory clock injection; no bare `SystemTime::now()` in kernel code

## Implementation notes

- **No polling:** the `pending_reverify` queue drains only during `pending_view_requests` ticks (event-driven)
- **Freshness writes go through the `EventStore` trait** (`get_check_again_after` / `set_check_again_after`, `&self` interior-mutability — the kernel holds `Arc<dyn EventStore>`). The LMDB override opens its own write transaction, commits, and updates the in-memory cache **only after** the commit succeeds (no cache/DB divergence on abort). `MemEventStore` mirrors the same contract over an in-memory map, so the kernel's TTL gate behaves identically on both backends. (The freshness stamp is therefore a separate committed transaction from the event insert, not batched into the insert's `RwTxn` — the unavoidable cost of the clean trait boundary.)
- **Sub-db shared environment:** `replaceable_freshness` lives in the same `lmdb::Environment` as the event store (ADR-0011)
- **Bounded in-flight:** `pending_reverify` is a `VecDeque<ReplaceableKey>` with no artificial ceiling; natural load-shedding via interest-dropping at EOSE (planner closes subs when views close)

## FFI ABI stability

F-TTL adds **no new C-ABI symbol**. Force-refresh is a trailing `force: int`
argument on `nmp_app_claim_profile`. The event-ref entry point is the unified
`nmp_app_resolve_ref(namespace=1, key, consumer_id, shape, liveness)`:

- `nmp_app_claim_profile(app, pubkey, consumer_id, force)` → cached kind:0 → `claim_replaceable(0, pubkey, None, force)`
- `nmp_app_resolve_ref(app, 1, key, consumer_id, shape, liveness)` → cached `naddr` coordinate → `claim_replaceable(kind, pubkey, Some(d_tag), false)` (TTL gate runs automatically)

The earlier standalone event-claim and replaceable-refresh symbols were removed.
`nmp_app_resolve_ref` is the unified replacement (ADR-0063 Lane D). See
[ADR-0016 — F-TTL FFI surface](../decisions/0016-f-ttl-ffi-surface.md) for the
current ABI rule.

## Testing

All ingestion paths (insert, replace, duplicate, foreign kind:5) exercise freshness update via the `replaceable_freshness` store in unit tests under `crates/nmp-testing/tests/store_replaceable_freshness.rs`.

Integration test: `framework_magic_contract.rs` contains a framework-magic bullet (C0) asserting that a `claim_replaceable` call after TTL-expiry triggers a REQ and updates the kernel's cached TTL on EOSE.
