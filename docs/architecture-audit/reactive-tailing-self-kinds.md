# Plan: Reactive Tailing Self-Kind Subscriptions at Login

## Context

NMP currently bootstraps the active account's own profile data (kinds 0, 3, 10002, 10050) as `OneShot + limit:1` interests — they fire once and close. This means:

1. If the user publishes a new kind:3 from another client while logged into chirp-tui, the follow list is never updated.
2. Kind:10006 (blocked relay list) is fetched but never enforced — `BlockedRelaySet` infrastructure exists in nmp-router but `build_routing_context()` always passes an empty set.
3. Kinds 10000 (mute list) and 10006 are absent entirely.

The fix: switch these to `Tailing` subscriptions (no limit) so apps get live reactivity for the logged-in user's self-config kinds completely automatically, with no app code required. Apps can override the default set.

---

## Step 1 — Decouple indexer-bootstrap routing from OneShot lifecycle

**Files:** `crates/nmp-planner/src/interest.rs`, `crates/nmp-planner/src/compiler/partition/case_a_authors.rs`

The `is_discovery_oneshot` gate in `case_a_authors.rs` currently routes interests to `bootstrap_indexer_relays` when `lifecycle == OneShot && scope == Global`. Switching self-kind interests to Tailing would break this.

Add a sentinel field to `LogicalInterest`:

```rust
// interest.rs
pub struct LogicalInterest {
    // existing fields …
    #[serde(default)]
    pub is_indexer_discovery: bool,
}
```

Update `case_a_authors.rs` to check `is_indexer_discovery` instead of inferring from `OneShot`:

```rust
// before: lifecycle == OneShot && scope == Global
// after:
interest.is_indexer_discovery
```

Set `is_indexer_discovery: true` on the existing indexer-bootstrap path in `startup.rs` (see step 2).

---

## Step 2 — Rewrite `active_account_bootstrap_requests()` in `startup.rs`

**File:** `crates/nmp-core/src/kernel/requests/startup.rs`

Replace `register_bootstrap_interest()` (which hardcodes `OneShot + limit:1`) with a new helper that uses `Tailing` and no limit:

```rust
fn register_self_kind_tailing(
    kernel: &mut Kernel,
    owner_key: &str,
    author_pubkey: &str,
    kinds: &[u64],
) {
    let key = SubOwnerKey::new(owner_key);
    kernel.planner.drop_owner(&key);          // clears stale account interests
    let interest = LogicalInterest {
        authors: vec![author_pubkey.to_string()],
        kinds: kinds.to_vec(),
        lifecycle: InterestLifecycle::Tailing,
        scope: InterestScope::Global,
        limit: None,
        is_indexer_discovery: false,
        ..Default::default()
    };
    kernel.planner.set_sub(&key, interest);
}
```

Default self-kinds: `[0, 3, 10002, 10000, 10006]`  
Remove 10050 from this set — NIP-17 runtime owns DM relay list separately.

The function must read the override set from `kernel.config.bootstrap_self_kinds` if set; otherwise use the default.

Account-switch safety: `drop_owner` before `set_sub` ensures switching accounts tears down old interests immediately.

The existing OneShot indexer-discovery interests (currently also in `active_account_bootstrap_requests`) keep their `InterestLifecycle::OneShot` but gain `is_indexer_discovery: true`.

Split the existing inline tests to `startup_tests.rs` as the file grows.

---

## Step 3 — `BlockedRelayLookup` trait + parser

**New file:** `crates/nmp-router/src/blocked_relay_lookup.rs`  
**Existing file:** `crates/nmp-core/src/substrate/routing.rs` (has `BlockedRelaySet`)

Add a substrate trait:

```rust
pub trait BlockedRelayLookup: Send + Sync {
    fn blocked_relays_for(&self, pubkey: &str) -> BlockedRelaySet;
}

pub struct InMemoryBlockedRelaysCache {
    inner: RwLock<HashMap<String, HashSet<String>>>,
}

impl InMemoryBlockedRelaysCache {
    pub fn update(&self, pubkey: &str, relay_urls: impl Iterator<Item = String>) { … }
}

impl BlockedRelayLookup for InMemoryBlockedRelaysCache { … }
```

Add `Kind10006Parser`: parses the relay URL list out of a kind:10006 event's tags and calls `cache.update()`. This lives in `crates/nmp-router` (not nmp-core) to avoid D0 NIP-naming in kernel types.

---

## Step 4 — Wire `BlockedRelayLookup` into kernel routing

**Files:** `crates/nmp-core/src/kernel/mailboxes.rs`, `crates/nmp-core/src/kernel/ingest/` (kind:10006 handler)

`mailboxes.rs` has 4 call sites of `build_routing_context()` that always do `let blocked = BlockedRelaySet::new()`. Change each to:

```rust
let blocked = kernel
    .blocked_relay_lookup
    .as_ref()
    .map(|lk| lk.blocked_relays_for(&active_pubkey))
    .unwrap_or_default();
```

Add `blocked_relay_lookup: Option<Arc<dyn BlockedRelayLookup>>` to `Kernel` struct.

Add a kind:10006 ingest handler (parallel to `contacts.rs`) that calls `Kind10006Parser`, updates the cache, and enqueues a `UserConfiguredRelaysChanged` trigger so the router re-evaluates outbox lanes immediately.

---

## Step 5 — NmpApp pre-start configuration slots

**File:** `crates/nmp-ffi/src/lib.rs`

Add two `Arc<Mutex<Option<…>>>` slots to `NmpApp`:

```rust
bootstrap_self_kinds: Arc<Mutex<Option<Vec<u64>>>>,
blocked_relay_lookup: Arc<Mutex<Option<Arc<dyn BlockedRelayLookup>>>>,
```

Expose C-ABI setter for `bootstrap_self_kinds` so embedders can override before calling `nmp_app_start`. Transfer both slots into `Kernel::config` / `Kernel` at start time.

---

## Step 6 — Register `Kind10006Parser` with `EventIngestDispatcher`

**File:** wherever `EventIngestDispatcher` is built (likely `crates/nmp-core/src/kernel/ingest/dispatcher.rs` or similar)

Register the new observer so that kind:10006 events arriving on any tailing subscription (including the self-kind one from step 2) flow through the parser automatically.

---

## Critical files

| File | Change |
|------|--------|
| `crates/nmp-planner/src/interest.rs` | Add `is_indexer_discovery: bool` field |
| `crates/nmp-planner/src/compiler/partition/case_a_authors.rs` | Update gate to use sentinel |
| `crates/nmp-core/src/kernel/requests/startup.rs` | Switch to Tailing, use drop_owner+set_sub, add 10000/10006, remove 10050 |
| `crates/nmp-router/src/blocked_relay_lookup.rs` | New — trait + cache + Kind10006Parser |
| `crates/nmp-core/src/substrate/routing.rs` | Extend `BlockedRelaySet` if needed |
| `crates/nmp-core/src/kernel/mailboxes.rs` | 4 call sites — populate `BlockedRelaySet` |
| `crates/nmp-core/src/kernel/ingest/` | New kind:10006 handler |
| `crates/nmp-ffi/src/lib.rs` | Two new pre-start slots |

## Reuse

- `BlockedRelaySet` in `crates/nmp-core/src/substrate/routing.rs` — already correct shape, just needs population
- `nmp-router` lane checks `ctx.blocked_relays.contains(url)` everywhere — infrastructure is complete
- `ingest_contacts()` in `contacts.rs` pattern — follow for kind:10006 handler
- `drop_owner` + `set_sub` on `Planner` — already in planner API, used elsewhere

## Verification

```bash
# Planner / interest changes
cargo test -p nmp-planner

# Startup / bootstrap changes
cargo test -p nmp-core

# Router / BlockedRelaySet changes
cargo test -p nmp-router

# Doctrine lint (always)
cargo test -p nmp-testing --test doctrine_lint_smoke
```

End-to-end: log in to chirp-tui, publish a new kind:3 from a second client, confirm chirp-tui's follow feed expands within one relay round-trip (no restart). Verify that relay URLs in kind:10006 are not connected to by the outbox router.
