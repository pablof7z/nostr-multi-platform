# Pre-merge Audit — `origin/t141-rescue` (T141 substrate-types extract + M7 ingest arms)

**Reviewer:** Codex pre-merge agent (worktree `agent-a8119ec4e5d84f64a`)
**Date:** 2026-05-18
**Branch under review:** `origin/t141-rescue` (locally as 3-commit chain `c60a329` → `9064f3f` → `5474316`)
**Base:** `origin/master` at `afa61aa` (HB67 tip)
**Verdict:** MERGE-OK

> **Status note:** At the time this review was dispatched, `origin/t141-rescue` had been
> rebased by T155 to SHAs `581d415` + `43c0e4a` + `a6c1fbc`, but those commits had NOT yet
> landed on `origin/master` (confirmed via `git merge-base --is-ancestor`). The HB65-agent
> orchestration log entry labelled "T141 / T155 FULLY LANDED" refers to landing in T155's
> worktree branch, not on `origin/master`. This review examines the semantically identical
> pre-rebase chain (`c60a329` + `9064f3f` + `5474316`) which carries the same diff content.
> T155 is responsible for the merge push; this review is independent and advisory.

---

## Commit Map

| Position | SHA | Subject |
|---|---|---|
| 1 | `c60a329` | `refactor(substrate): extract nmp-substrate-types crate with DomainBackend trait seam` |
| 2 | `9064f3f` | `feat(ingest): explicit M7 arms for kinds 7/1111/9735 — reactions/comments/zaps no longer silently dropped (T141)` |
| 3 | `5474316` | `docs(perf): PD-029 RESOLVED at dda5b9b + ad3096e — trait seam landed` |

---

## Per-Commit Findings

### Commit 1 — `c60a329`: substrate-types extract + DomainBackend trait seam (PD-027 + PD-029 Option A)

**What it does:** Moves all substrate-level types (`StoredEvent`, `RawEvent`, `VerifiedEvent`,
`StoreError`, watermarks, GC budget, tombstones, query types, `DomainModule`, `ViewModule`,
`ActionModule`, `CapabilityModule`, `IdentityModule`, `ModuleRegistry`, migration types,
NIP-10 tag codec, `NaddrCoord`) out of `nmp-core` into a new `nmp-substrate-types` leaf crate.
Simultaneously dissolves the `DomainHandleInner` enum (which tied substrate-types to
`nmp-core::store::lmdb::Inner`) via a trait seam: `pub trait DomainBackend: Send + Sync`
with the 4 per-namespace ops (`put / get / delete / scan_prefix`); `DomainHandle` wraps
`Arc<dyn DomainBackend>`. `nmp-core` provides `MemDomainBackend` and `LmdbDomainBackend`.

**PD-027 (substrate-types extract): SOUND.**

- New crate is a genuine leaf: `Cargo.toml` deps are `nostr`, `serde`, `serde_json` only.
  No `heed`, `nostr-lmdb`, or `nostr-database` in the closure. Backend-agnosticism verified
  by the commit message's `cargo tree` result and confirmed by the `Cargo.lock` diff (no LMDB
  transitive deps under `nmp-substrate-types`).
- The `nmp-core → nmp-{reactions,nip22,nip57}` cycle that previously blocked T141 is
  dissolved correctly. The per-NIP crates now depend on `nmp-substrate-types` (not `nmp-core`)
  for substrate types, and `nmp-core` can add the forward edges to those crates in Commit 2.
- Source-compatibility shims at `nmp-core::store`, `nmp-core::tags`, `nmp-core::substrate`,
  and `nmp_core::planner::NaddrCoord` re-export from `nmp-substrate-types` — callers outside
  this diff see no breaking change.
- The 6 per-NIP crates modified (`nmp-nip01`, `nmp-nip22`, `nmp-nip57`, `nmp-reactions`,
  `nmp-threading`, `nmp-nip23`) all correctly swap `use nmp_core::{store, substrate, tags}::`
  for `use nmp_substrate_types::{store, substrate, tags}::`. No orphan cross-imports left
  pointing at `nmp_core` for substrate types in production code.

**PD-029 (trait seam): SOUND.**

- The original `DomainHandleInner { Mem {...}, Lmdb { backend: Arc<nmp_core::store::lmdb::Inner> } }`
  tied the enum to a private LMDB type. The trait seam eliminates this by having backends
  implement `DomainBackend` in `nmp-core` (not in `nmp-substrate-types`) and wrapping
  with `DomainHandle::new(namespace, Arc::new(BackendImpl{...}))`.
- No `#[cfg(feature = "lmdb-backend")]` appears in `nmp-substrate-types`. The T141 ↔ T136b
  LMDB collision is genuinely dissolved, not papered over — the LMDB backend impl lives
  entirely in `nmp-core/src/store/lmdb/domain.rs::LmdbDomainBackend`.
- Each `DomainBackend` method opens its own transaction and commits within the call. No
  transaction lifetime escapes. This is confirmed by reading `lmdb/domain.rs` — every
  `impl DomainBackend for LmdbDomainBackend` method creates a local `txn` and commits
  before returning.
- `scan_prefix` materializes into `Vec<(Vec<u8>, Vec<u8>)>` (not a live cursor). This is
  the right choice for the current backends and avoids transaction lifetime hazards at the
  trait boundary.

**D0 check: CLEAN.**

- One grep hit on `nmp-nip29` in `substrate/domain.rs` is a doc comment noting that 13
  existing `nmp-nip29` module impls inherit the `default fn ingest_kinds()` body — a
  source-compatibility note, not an architectural leak. No nip29/podcast/highlighter
  *nouns* or *types* in `nmp-substrate-types` source.

**D6 check: CLEAN.** No `unwrap` or `expect` in the production paths of `lmdb/domain.rs`,
`mem/domain.rs`, or `store/mod.rs`. Every method returns `Result<_, StoreError>`.

**File sizes:**

- `nmp-substrate-types/src/store/mod.rs`: 141 lines — well within 300 soft cap.
- `nmp-substrate-types/src/tags.rs`: 409 lines — exceeds 300 soft cap; does NOT exceed
  500 hard cap. Note: this is a straight move of the existing `nmp-core/src/tags.rs` which
  was itself 409 lines. No new code; the file was already at this size in master.
- All other new files in `nmp-substrate-types/src/` are either moved-unchanged or trivial
  (0-30 lines).

**OBSERVATION (LOW):** `scan_index` on `DomainHandle` delegates to `scan_prefix` for all
backends because neither backend maintains a secondary index today. This is documented in
code but will need a real implementation when any per-NIP crate adds a composite index.
File a follow-up when that happens; no action needed for merge.

**OBSERVATION (LOW):** The `nmp_core::store::types` re-export shim (a `pub mod types` that
re-exports `nmp_substrate_types::store::types::*`) is an extra indirection layer. Source
callers see no change; IDE navigation may take an extra hop. Accept as-is.

---

### Commit 2 — `9064f3f`: explicit M7 ingest arms for kinds 7/1111/9735

**What it does:** Inserts three explicit match arms in `nmp-core/src/kernel/ingest/mod.rs`
(before the `_ =>` catch-all) for kinds 7, 1111, and 9735. Each arm runs
`verify_and_persist` and then dispatches to the per-NIP `decode_and_route` only when the
outcome is `Inserted | Replaced`. Adds three new ingest files (`reactions.rs`, `comments.rs`,
`zaps.rs`) plus a shared `ingest_into_domain` helper. Adds test-support infrastructure
(`IngestT141TestHarness`) and 4 integration tests in `nmp-testing/tests/ingest_t141_routing.rs`.

**D4 gating: CORRECT.**

The dispatch pattern is `verify_and_persist → matches!(outcome, Inserted | Replaced) →
ingest_into_domain`. This is byte-for-byte identical to the existing kind:0/3/10002 arm
gating. Only events canonical in the store (with a confirmed `Inserted | Replaced` outcome)
reach the per-NIP `decode_and_route`. The D4 invariant is upheld.

**Kind dispatch: COMPLETE.**

- Kind 7 (NIP-25 reactions) → `nmp_reactions::decode_and_route` via `reactions.rs`.
- Kind 1111 (NIP-22 comments) → `nmp_nip22::decode_and_route` via `comments.rs`.
- Kind 9735 (NIP-57 zap receipts) → `nmp_nip57::decode_and_route` via `zaps.rs`.

All three are dispatched BEFORE the `_ =>` catch-all, which retains its existing
`verify_and_persist`-only behaviour. The catch-all is not modified.

**Note on kind:18 reposts:** The `reactions.rs` doc comment mentions kind:6/16 reposts
alongside kind:7. The match arm in `mod.rs` only covers `7 =>`. Kinds 6 and 16 still fall
through to the catch-all. The doc comment is accurate (it says this dispatch happens
"after `verify_and_persist` returns `Inserted | Replaced`" for kind:7 specifically) but
the file-level comment "Kind:7 (NIP-25 reactions) and kind:6 / 16 (NIP-18 reposts) ingest"
may mislead a future reader. This is LOW: no bug, no mis-routing — reposts were always
catch-all and remain so. File as a doc-clarity follow-up.

**`ingest_into_domain` helper: WELL-SHAPED.**

- Lookup by id via `store.get_by_id`: correct (the event was just persisted; should be
  present; lookup miss is logged and treated as a non-fatal skip).
- `store.domain_open(namespace)`: opens the per-NIP namespace handle; failure is logged
  and treated as a skip (consistent with D6 — no FFI exception).
- `decode_and_route` failure: logged, not propagated. This is correct because the
  verify+store side already succeeded; a decode failure here is a data-quality issue with
  the specific NIP's record schema, not a kernel bug. The event is canonical in the store
  regardless.
- The function pointer signature `fn(&StoredEvent, &DomainHandle) -> Result<(), StoreError>`
  is appropriately narrow.

**`IngestT141TestHarness`: APPROPRIATELY SCOPED.**

- Gated on `cfg(any(test, feature = "test-support"))` — never reaches production FFI.
- The `test_deliver_event_for_dispatch` method goes through the full production `handle_event`
  dispatch (including `verify_and_persist` + per-kind arms). Tests that use it exercise the
  real dispatch path, not a stub.
- The `expect()` calls in `test_deliver_event_for_dispatch` (`serde_json::from_str(...).expect(...)`)
  are test-support only. D6 covers errors crossing FFI — not `expect` calls in
  test infrastructure. No violation.

**Tests: ADEQUATE (4 integration tests).**

1. `kind_7_reaction_routes_to_reactions_domain` — asserts at least one row in `nmp.reactions`
   namespace after kind:7 ingest. Uses a real Schnorr-signed event.
2. `kind_1111_comment_routes_to_comments_domain` — asserts at least one row in `nmp.nip22.comments`.
3. `kind_9735_zap_receipt_routes_to_zaps_domain` — asserts at least one row in `nmp.nip57.zaps`.
4. `unknown_kind_falls_through_to_verify_only_catch_all` — kind:9999 writes zero domain rows
   in any of the three namespaces. Regression guard for the catch-all.

Tests sign real events via `nostr::EventBuilder` + `sign_with_keys`, exercising the full
Schnorr verification path inside `verify_and_persist`.

**OBSERVATION (LOW):** Tests assert "at least one row landed" (`!rows.is_empty()`). They do
not assert the exact row count, key shape, or decoded field values. This is acceptable for
routing coverage; per-NIP decode fidelity is tested within the NIP crates themselves. For
a future hardening pass, consider pinning the row key prefix per NIP schema.

**D0 check: CLEAN.** The three new ingest files and the test file are free of nip29/podcast/
highlighter references. `nmp-core`'s new deps on `nmp-nip22`, `nmp-nip57`, and
`nmp-reactions` are protocol-crate edges, not D0 violations (D0 forbids *per-app nouns*
leaking into the kernel, not the kernel dispatching to protocol crates it knows about).

**D6 check: CLEAN.** All error paths in `reactions.rs` / `comments.rs` / `zaps.rs` return
early with a log line, never panicking. No `unwrap` or `expect` in production code.

**File sizes:**

- `reactions.rs`: 85 lines — well within caps.
- `comments.rs`, `zaps.rs`: 21 lines each — trivial.
- `ingest_t141_routing.rs`: 203 lines — within both caps.
- `test_support.rs` (after additions): 264 lines — within both caps. This is the existing
  test_support file with additions; 264 is under the 300 soft cap.

**Cargo.toml change:** `nmp-core` gains runtime deps on `nmp-nip22`, `nmp-nip57`, and
`nmp-reactions`. These are the correct forward edges that the cycle dissolution in Commit 1
enables. No circular dependency re-introduced (the per-NIP crates now depend on
`nmp-substrate-types`, not `nmp-core`).

---

### Commit 3 — `5474316`: PD-029 RESOLVED doc

**What it does:** Updates `docs/perf/orchestration-log.md` and `docs/perf/pending-user-decisions.md`
to record that PD-029 was resolved via Option A (trait seam). Records the two landing SHAs,
verification steps, and the pre-existing `uuid` dep bug in `nmp-testing/store_harness.rs:57`
(unrelated to this work, reproduced on master pre-cherry-pick).

**Assessment: ROUTINE.** Correct bookkeeping. No code change. The PD-029 entry accurately
describes the trait method set (4 ops, `scan_index` delegates to `scan_prefix`), confirms
no `#[cfg(feature = "lmdb-backend")]` in substrate-types, and notes no transaction lifetime
hazard. The pre-existing `uuid` test bug observation is accurate (visible on master today).

---

## Test-Count Gap Explanation

The brief referenced a "1081 not 1238" discrepancy that stalled the original T141 agent.
T155 (HB65-agent) resolved this:

> "regression" was a phantom — cargo default fail-fast was truncating test runs after
> `nmp_core_is_doctrine_clean` (pre-existing T154 fail) inside `nmp-testing` masked
> ~150 subsequent tests. `cargo test --workspace --no-fail-fast` showed:
> master baseline 1286/2/17 vs rescue 1291/1/17 = +5 passed (4 new T141 routing tests
> + 1 `unknown_author_*` test converted from fail→pass via T134 semantics) -1 failed (same).

Breakdown:
- The "1081" figure was a truncated run. The full workspace has ~1286+ tests.
- The `nmp_core_is_doctrine_clean` failure (T154 territory, pre-existing LMDB lint issue)
  caused `cargo test --workspace` to stop early without `--no-fail-fast`.
- Net delta from T141: +4 new tests (`ingest_t141_routing.rs`), +1 converted pass
  (T134-related semantic change). Zero regressions.

Gap is **fully explained**. T155 investigation is complete.

---

## D0 / D6 / File-Size Summary

| Check | Result |
|---|---|
| D0 — no per-app nouns (nip29/podcast/highlighter) in nmp-core | PASS |
| D0 — no per-app nouns in nmp-substrate-types source | PASS (one doc comment reference to nmp-nip29 module count; not a type leak) |
| D6 — no unwrap/expect in production hot paths | PASS |
| D6 — all error paths return Result or log-and-skip | PASS |
| File size — all new files under 300 soft cap | MOSTLY PASS (tags.rs at 409 is a straight move of pre-existing file; no new code above cap) |
| File size — all new files under 500 hard cap | PASS |
| Cargo cycle — no new circular dependency | PASS (verified via Cargo.lock) |

---

## Follow-Up Tasks (post-merge)

### LOW — Doc clarity in `reactions.rs` module comment

File header says "Kind:7 (NIP-25 reactions) and kind:6 / 16 (NIP-18 reposts) ingest" but
the match arm only covers kind:7. Kinds 6/16 still fall through to the catch-all. The
comment should clarify this or be narrowed to kind:7 only. File as a documentation polish
task when T141 workstream is otherwise complete.

### LOW — `scan_index` is a `scan_prefix` alias

`DomainHandle::scan_index` delegates to `scan_prefix` unconditionally. This is correct for
the current backends (no secondary index exists) but the method signature implies richer
semantics. When any per-NIP crate needs a real secondary index, this method needs a real
implementation. File as a T-substrate-secondary-index placeholder task.

### LOW — Test assertions are presence-only

`ingest_t141_routing.rs` tests assert `!rows.is_empty()` without pinning key shapes or
decoded values. Adequate for routing coverage. Per-NIP decode fidelity testing should live
in the NIP crates. No action before merge; document as a hardening follow-up.

### OBSERVATION — Pre-existing `uuid` dep missing in `nmp-testing`

`nmp-testing/store_harness.rs:57` calls `uuid::new_v4()` without `uuid` declared as a
dep. This causes `cargo test --workspace --features lmdb-backend` to fail even on master
before this branch. T155 notes it as pre-existing. File a separate task (T-uuid-dep-fix)
to add `uuid` to `nmp-testing/Cargo.toml` dev-deps; this unblocks the LMDB full-feature
test run.

---

## Overall Verdict

**MERGE-OK**

The three-commit chain is architecturally clean:

1. The substrate-types extract (PD-027 option A) is correct — the new crate is a genuine
   leaf with no LMDB deps, all re-exports are source-compatible, and the Cargo cycle is
   dissolved rather than worked around.

2. The PD-029 trait seam (option A) genuinely resolves the T141 ↔ T136b LMDB collision.
   `DomainHandleInner` is gone. Every backend impl owns its transaction lifetime internally.
   No backend type leaks into substrate-types.

3. The M7 ingest arms for kinds 7/1111/9735 are correctly gated (D4 `Inserted | Replaced`
   check), routed to the right per-NIP `decode_and_route` functions, and covered by 4
   integration tests using real Schnorr-signed events.

4. No D0 violations, no D6 violations, no file-size hard-cap breaches.

5. Test-count discrepancy is explained (cargo fail-fast masking, not a regression).

Follow-ups are all LOW severity and can be filed after merge. No blocking issues.

T140, T142, and T145 are unblocked upon merge.
