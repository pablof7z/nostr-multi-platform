---
title: Marmot/MLS Silent Failure Surfacing — Snapshot Diagnostics
slug: marmot-silent-failure-surfacing
summary: MarmotSnapshot now exposes orphaned_commit_count and keyring_unavailable so shells can detect silent MLS commit drops and keyring init failures instead of receiving silently degraded behavior.
tags:
  - marmot
  - mls
  - v61
  - v62
  - snapshot
  - silent-failure
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# Marmot/MLS Silent Failure Surfacing — Snapshot Diagnostics

> MarmotSnapshot now exposes orphaned_commit_count and keyring_unavailable so shells can detect silent MLS commit drops and keyring init failures instead of receiving silently degraded behavior.

## V-61: Orphaned MLS Commit Surfacing

`crates/nmp-marmot/src/service.rs` previously had `Drop` impls on `PendingGroupChange` and `CreateGroupPending` that silently cleared pending MLS commits with no signal to the host.

Fix:
- `MarmotError::OrphanedCommit { group_id_hex }` variant added with a clear Display message
- `orphaned_commit_count: Arc<AtomicU32>` field added to `MarmotService`, cloned into both pending-change structs
- Both `Drop` impls increment the counter and emit `eprintln!` of the typed error when dropped unresolved (SelfRemove drops are explicitly exempt)
- `orphaned_commit_count()` accessor added to `MarmotService` [^42908-34]

## V-62: Keyring Unavailable No Longer Silent

`crates/nmp-marmot/src/ffi.rs` previously fell back to an in-memory mock store on `MarmotService::new` failure with no return-code change or error signal.

Fix:
- The `!use_mock` fallback branch that installed the mock store is deleted
- Any `MarmotService::new` failure returns null immediately with a diagnostic `eprintln!` naming the `KeyringUnavailable` error class
- `MarmotProjection::new` gains a `keyring_unavailable: bool` parameter [^42908-35]

## Snapshot Surface

`MarmotSnapshot` gains two additive fields (both `#[serde(default)]` for backward compatibility):
- `orphaned_commit_count: u32` — incremented whenever a pending commit is dropped unresolved
- `keyring_unavailable: bool` — set when the keyring could not be initialized

Shells and tests can assert `orphaned_commit_count == 0` and `keyring_unavailable == false` to detect silent failure modes. [^42908-36]

## See Also

