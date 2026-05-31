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
- **is_parameterized_replaceable:** kinds 20000–29999, 30000–39999 — keyed by `(kind, pubkey, d_tag)`

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
- `Kernel::claim_replaceable(kind: u32, pubkey: &str, d_tag: Option<&str>) -> Result<...>`
- `Kernel::check_again_after(kind: u32, pubkey: &str, d_tag: Option<&str>) -> Option<u64>` (test introspection)

**FFI (public, for apps):**
- `void nmp_app_refresh_replaceable(NmpApp* app, uint32_t kind, const char* pubkey, const char* d_tag_or_null)`
  - Sets `check_again_after = 0` → forces immediate re-verification
  - Called from UI when user explicitly requests "refresh profile"

**Thin wrapper:**
- `nmp_app_claim_profile(...)` is now a thin wrapper over `nmp_app_refresh_replaceable(app, 0, pubkey, NULL)`

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
- **LMDB atomic writes:** `check_again_after` write and event insert are in the same `RwTxn`
- **Sub-db shared environment:** `replaceable_freshness` lives in the same `lmdb::Environment` as the event store for transaction atomicity (ADR-0011)
- **Bounded in-flight:** `pending_reverify` is a `VecDeque<ReplaceableKey>` with no artificial ceiling; natural load-shedding via interest-dropping at EOSE (planner closes subs when views close)

## FFI ABI stability

`nmp_app_refresh_replaceable` is a new symbol added to the kernel FFI surface. It completes the `nmp_app_claim_*` family for replaceable events:

- `nmp_app_claim_profile` (existing) → now routes through `claim_replaceable(0, pubkey, None)`
- `nmp_app_refresh_replaceable` (new) → force-refresh by zeroing `check_again_after`

No breaking changes to existing symbols; `NmpCore.h` updated.

## Testing

All ingestion paths (insert, replace, duplicate, foreign kind:5) exercise freshness update via the `replaceable_freshness` store in unit tests under `crates/nmp-testing/tests/store_replaceable_freshness.rs`.

Integration test: `framework_magic_contract.rs` contains a framework-magic bullet (C0) asserting that a `claim_replaceable` call after TTL-expiry triggers a REQ and updates the kernel's cached TTL on EOSE.
