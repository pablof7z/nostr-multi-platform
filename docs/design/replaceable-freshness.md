# Replaceable Event Freshness — `check_again_after`

> Design doc for F-TTL. Covers lazy TTL re-verification and force-refresh for all
> replaceable Nostr event kinds. See `WIP.md` for current implementation status.

## 1. Problem

Replaceable Nostr events — kind:0 (profiles), kind:3 (contact lists), kind:10002 (relay
lists), kind:30023 (articles), and any replaceable kind including unspecced ones such as
kind:10331 and kind:39999 — are currently fetched once and cached in LMDB indefinitely.

There is no mechanism to re-verify freshness after initial fetch. A user who updates their
kind:0 profile will not be seen by NMP clients until the app restarts or an incidental
event (e.g. a new kind:10002) coincidentally triggers a re-fetch. This is incorrect
behaviour for any app that shows user profiles, contact metadata, or article content.

## 2. Goals

1. **Lazy TTL re-fetch.** When a replaceable event is accessed/claimed and its cached copy
   is older than a configurable per-kind TTL, trigger a background re-verification REQ.
   Serve the cached value immediately (D6: no blank flash); refine in place if newer data
   arrives (D1).

2. **Force "check now".** An explicit app API to demand immediate re-verification,
   regardless of TTL state. Intended for use when the user navigates to a profile page or
   article detail view.

3. **Generic across all replaceable kinds.** The mechanism is not kind:0-specific. It
   works identically for any replaceable kind number, whether specced or not, using NIP-01
   range rules to classify replaceability.

## 3. Non-Goals

- No preemptive polling or background timers. TTL is evaluated lazily, only on access.
- No cache invalidation. Cached event data is never cleared — fresh data refines in place.
- No change to non-replaceable event handling (kind:1, ephemeral kinds, etc.).

## 4. Core primitive — `check_again_after`

A single `u64` (unix milliseconds) per replaceable identity. Meaning: "do not re-verify
this event before this timestamp."

### Lifecycle

| Event | New value |
|---|---|
| REQ dispatched for re-verification | `now + INFLIGHT_GUARD` (fixed ~1h constant) |
| EOSE received for re-verify sub | `now + ttl_for_kind(kind)` |
| Event confirmed from relay (Inserted, Replaced, **or** Duplicate) | `now + ttl_for_kind(kind)` |

Setting `check_again_after = now + INFLIGHT_GUARD` at REQ dispatch time serves as the
debounce: if the relay is slow, unreachable, or returns the same event we already have,
we wait at least 1h before re-trying. EOSE and event-confirmation then reset the clock
to the correct per-kind TTL.

Updating on **Duplicate** outcomes is load-bearing: if the relay returns the exact event
already in the store, that is still a confirmation that our cached copy is current. Without
this update, a re-verify REQ that returns an unchanged event would expire immediately on
the next access and loop.

### Force-refresh

Setting `check_again_after = 0` (past epoch) on any key causes the next access to
immediately trigger re-verification. This is how the "check now" API works.

## 5. Replaceable identity

Per NIP-01:

| Kind range | Type | Key |
|---|---|---|
| 0–9999 | Regular replaceable | `(kind, pubkey)` |
| 10000–19999 | Regular replaceable | `(kind, pubkey)` |
| 20000–29999 | Parameterized replaceable | `(kind, pubkey, d_tag)` |
| 30000–39999 | Parameterized replaceable | `(kind, pubkey, d_tag)` |

The codebase should expose two utilities:

```rust
pub fn is_replaceable(kind: u32) -> bool
pub fn is_parameterized_replaceable(kind: u32) -> bool
```

covering the full NIP-01 ranges. Any kind not in these ranges is non-replaceable.

## 6. LMDB persistence

### Sub-db: `replaceable_freshness`

Part of the `_kernel` schema (not a `DomainModule`).

**Key:** `kind[4B big-endian] ‖ pubkey_bytes[32B] ‖ d_tag_utf8[variable]`

The `d_tag` segment is omitted entirely for regular (non-parameterized) replaceables. No
length prefix is needed because lookups are always exact-key (the caller always knows kind,
pubkey, and d_tag).

**Value:** CBOR `{ check_again_after: u64 }` (unix ms).

Minimal by design — the only semantic field needed. Additional fields (e.g. last sync
method) can be added in a future schema version if diagnostics require them.

**In-memory cache:** hot-loaded into a `HashMap<ReplaceableKey, u64>` on
`LmdbEventStore::open()`. Expected cardinality is O(100)–O(10k) for typical apps (one
entry per distinct replaceable event ever seen). Every write updates both the in-memory
map and LMDB in the same `RwTxn`.

**Clock discipline (D9):** the store never reads the clock. Callers supply the timestamp;
the store writes it. Comparison (`now > check_again_after`) happens in the kernel.

## 7. TTL configuration

```rust
pub struct ReplaceableTtlConfig {
    pub per_kind: BTreeMap<u32, Duration>,
    pub default: Duration,
}
```

Stored as a field on `Kernel`. Lookup: `per_kind.get(&kind).copied().unwrap_or(default)`.

### Defaults

| Kind | TTL |
|---|---|
| 0 (profile metadata) | 1 hour |
| 10002 (relay list) | 6 hours |
| fallback (all other replaceable kinds) | 6 hours |

App-side override via a Rust setter (`kernel.set_replaceable_ttl(cfg)`) called before
`nmp_app_start`. FFI surface deferred — Rust-side defaults are sufficient for v1.

## 8. General ingestion hook

The `check_again_after` stamp fires in the **general replaceable event ingestion
dispatcher** in `kernel/ingest/mod.rs`, not inside any kind-specific handler (not
`ingest_profile`, not any future `ingest_contact_list`, etc.).

The hook fires for **all three store outcomes** — Inserted, Replaced, and Duplicate —
for any event whose kind is classified as replaceable. It reads the kernel clock, computes
`now + ttl_for_kind(event.kind)`, and writes the new `check_again_after` to the store
(and in-memory map) in the same transaction as the event write where possible.

Kind-specific handlers (`ingest_profile`, etc.) run after the general hook and are
unmodified.

## 9. Re-verification REQ flow

### Trigger

On any `claim_replaceable(kind, pubkey, d_tag?)` call (or the existing
`claim_profile` wrapper), after serving the cached value:

1. Look up `check_again_after` for the key.
2. If `now > check_again_after` and the key is not already in `pending_reverify`:
   - Enqueue key in `pending_reverify`.
   - Write `check_again_after = now + INFLIGHT_GUARD` to store (debounce).

### REQ shape

Regular replaceable: `{"kinds":[k],"authors":[pubkey],"limit":1}`
Parameterized replaceable: `{"kinds":[k],"authors":[pubkey],"#d":[d_tag],"limit":1}`

Routed via `route_outbox_subscription_relays` (outbox direction — author's NIP-65 write
relays, with indexer fallback for cold-start authors).

### EOSE handling

The kernel maintains a `sub_id → Vec<ReplaceableKey>` map for all in-flight re-verify
subs. On EOSE, for each key in the map: write `check_again_after = now + ttl_for_kind`.
The map entry is then removed.

Batching: multiple keys with the same kind can share one REQ (one `authors` array). The
map entry for that sub covers all pubkeys in the batch.

## 10. App-facing API

### Existing `claim_profile` / `release_profile`

These become thin wrappers over the general `claim_replaceable(kind: 0, pubkey, None)` /
`release_replaceable(kind: 0, pubkey, None)` path. No ABI change — existing callers are
unaffected.

### New: force-refresh

```c
void nmp_app_refresh_replaceable(NmpApp *app, uint32_t kind, const char *pubkey, const char *d_tag_or_null);
```

Sets `check_again_after = 0` for the key and immediately enqueues a re-verify REQ if one
is not already pending. Fire-and-forget. Fresh data arrives through the normal snapshot
update path — the app does not need to poll.

### Snapshot / notification

No new snapshot fields for v1. When a re-verify REQ returns a newer event, it flows
through the normal ingestion → projection → snapshot push path. The app sees a standard
data update and does not need to know it was triggered by a TTL expiry.

## 11. Doctrine compliance

| Doctrine | Status | Notes |
|---|---|---|
| D1 (render now, refine in place) | ✅ | Cached value served immediately; re-verify is background |
| D3 (outbox routing) | ✅ | Re-verify REQs use `route_outbox_subscription_relays` |
| D4 (no cache invalidation) | ✅ | `profiles` / equivalent caches never cleared |
| D6 (no blank flash) | ✅ | No render gate on freshness state |
| D8 (bounded reactivity) | ✅ | No polling loop; lazy-only trigger |
| D9 (kernel owns time) | ✅ | All timestamp writes use injected `Clock` |

## 12. Edge cases

| Case | Handling |
|---|---|
| Re-verify REQ returns same event (Duplicate) | `check_again_after` bumped to `now + ttl` — no loop |
| Relay unreachable, no EOSE | INFLIGHT_GUARD (~1h) expires; next access retries |
| Force-refresh while REQ already in flight | `pending_reverify` guard prevents duplicate REQ |
| Force-refresh back-to-back (user spamming) | Same guard; second call is a no-op |
| Kind not in NIP-01 replaceable ranges | `is_replaceable` returns false; hook skips it |
| Cold-start (no relays connected yet) | Key enqueued in `pending_reverify`; drains when relay connects |
| Parameterized replaceable, missing d_tag | Treated as `d_tag = ""` (NIP-01 default) |
| `Kernel::Reset` | Per-kind caches reset; `replaceable_freshness` LMDB rows persist (config survives reset) |
