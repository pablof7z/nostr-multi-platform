---
title: "nostr-lmdb env-injection seam — Gate-1 audit status (T136)"
status: blocked-on-upstream-decision
date: 2026-05-18
relates-to:
  - docs/decisions/0011-lmdb-env-sharing.md
  - docs/design/nostrdb-rs-evaluation.md
  - docs/design/lmdb/trait.md
resolves: T136 Gate 1
opens: T136a (option selection), T136b (Gate 2+ implementation)
---

# `nostr-lmdb` env-injection seam — Gate-1 audit

T136 Gate 1 was a **STOP-and-report** if upstream `nostr-lmdb` lacks the
env-injection seam ADR-0011 depends on. **It does.** This document records the
evidence, the four options, and what each costs — so T136a can pick a path
without re-doing the audit.

## 1. Verdict

**Upstream `nostr-lmdb` v0.44.1 (current crates.io) and master** expose **no
env-injection seam**. ADR-0011's primary design — "NMP owns the LMDB env and
injects it into `nostr-lmdb`" — cannot be implemented against the published
crate as-is. Gate 2 (implement the 33 `EventStore` methods routing to
`nostr-lmdb`) is **blocked** until one of the four options below is selected.

Status: T136 Gate-1 complete with a STOP outcome. T136a is the
option-selection task; T136b is the implementation task once the option is
chosen.

## 2. Evidence (audited 2026-05-18 against `nostr-lmdb-0.44.1` + master)

Read against `~/.cargo/registry/src/.../nostr-lmdb-0.44.1/src/` and the
`rust-nostr/nostr` master branch on GitHub.

### 2.1 The env is created internally and never exposed

`src/store/lmdb/mod.rs:111-118` (and identically on master):

```rust
let env: Env = unsafe {
    EnvOpenOptions::new()
        .flags(EnvFlags::NO_TLS)
        .max_dbs(11 + additional_dbs)
        .max_readers(max_readers)
        .map_size(map_size)
        .open(path)?
};
```

The `Env` is held in a `pub(crate) struct Lmdb { env: Env, ... }` (line 73) and
the `Store` that owns it has a `pub(crate)` field — **neither is reachable
from outside the crate.** No `with_env` constructor, no `From<heed::Env>`, no
getter. The public surface (`src/lib.rs`) is `NostrLmdb::open(path)` and
`NostrLmdb::builder(path)` — both path-only.

### 2.2 All writes go through a dedicated ingester thread

`src/store/mod.rs:45`: `let ingester = Ingester::run(db.clone());`
`src/store/mod.rs:61-65`: `save_event` posts an `IngesterItem` to a
`flume::Sender` and awaits a oneshot — the actual `RwTxn` is opened *inside
the ingester thread loop*, not by the caller.

This is the strfry/share-nothing design also flagged in
`nostrdb-rs-evaluation.md` §1: the caller cannot interpose its own writes in
the same txn as `save_event`.

### 2.3 The txn-scoped write primitive exists but is `pub(crate)`

`src/store/lmdb/mod.rs:295`:

```rust
pub(crate) fn store(
    &self,
    txn: &mut RwTxn,
    fbb: &mut FlatBufferBuilder,
    event: &Event,
) -> Result<(), Error>
```

This is exactly the shape ADR-0011 needs as `save_event_in_txn` — but it's
private to the crate **and it doesn't implement NIP-09 deletion or
replaceable/addressable supersession.** Those policies live in the ingester
loop (`src/store/ingester.rs`, 411 LOC), not in `Lmdb::store`. Exposing
`store` as-public would give a write that *skips the entire policy layer the
upstream crate exists to provide* — net-negative on the "battle-tested code
for free" argument that motivated using the crate at all.

A real `save_event_in_txn` needs the policy layer **factored out of the
ingester thread loop into a callable function over `&mut RwTxn`**. That is
not a one-liner.

### 2.4 Upstream PR tracker

GitHub search `is:pr nostr-lmdb env` on `rust-nostr/nostr`: **0 open, 0
closed.** There is no precedent for env-injection upstream — no signal on
maintainer appetite, no prior art to align against.

Issue #969 ("Initializing Client with LMDB database crashes iOS", closed
2025-07-09) is the only env-adjacent issue and is about iOS map-size, not
env ownership.

### 2.5 Sub-db slot reservation does exist

`NostrLmdbBuilder::additional_dbs(u32)` (`src/lib.rs:82-88`) reserves
additional sub-db slots beyond the 9 internal ones. This is the *one* hook
the crate already exposes that would help an env-sharing design — NMP's
provenance / watermarks / claims / domain sub-dbs would fit here cleanly,
**but** they still cannot share a `RwTxn` with `save_event` because of 2.2.

### 2.6 Heed version

`nostr-lmdb` depends on `heed = "0.20"` (resolved to `0.20.5`). **Note for
ADR-0011 readers:** ADR-0011 uses the names `lmdb::Environment` /
`lmdb::RwTxn` throughout, but the actual crate is `heed` (a pure-Rust LMDB
wrapper, distinct from the `lmdb` crate). The ADR's architectural claim is
unchanged but the type names need updating in a follow-up commit. Filing
this as a doc-fix follow-up, not blocking T136a.

## 3. Options (pick one in T136a)

### Option A — Upstream PR

**Scope:** two additions to `rust-nostr/nostr/database/nostr-lmdb`:

1. `NostrLmdbBuilder::with_env(env: heed::Env) -> Self` (or equivalent
   constructor accepting a pre-opened env). **Mechanical** — the existing
   `build()` already calls `EnvOpenOptions::new()...open(path)`; replace
   that path with the injected env.

2. `NostrLMDB::save_event_in_txn(&self, txn: &mut RwTxn, event: &Event)
   -> Result<SaveEventStatus, Error>`. **Non-trivial.** Today the
   NIP-09 / replaceable / addressable policy lives in the ingester
   thread loop (`store/ingester.rs`, 411 LOC). The PR has to refactor:
   *extract the per-event policy decision (`SaveEventStatus` + the
   `store`/`remove`/`mark_deleted` side-effects) into a callable
   `Lmdb::save_event_policy(txn, event) -> Result<SaveEventStatus,
   Error>` that runs synchronously on the caller's `RwTxn`.* The
   ingester then becomes a thin batching wrapper over this primitive.

**Cost:** moderate Rust work (~200-400 LOC diff on the upstream side) +
unknown acceptance latency (no precedent in the PR tracker; no prior
contact with maintainers about env-injection).

**Pros:** clean long-term; no fork maintenance.

**Cons:** blocks T136b for an indeterminate window; if rejected, falls
back to B or C anyway with time wasted.

### Option B — Pinned local fork

**Scope:** same two additions as A, carried in a fork referenced via
`[patch.crates-io]` in workspace `Cargo.toml`. Replaced with upstream once
the PR lands.

**Cost:** same code work as A (the refactor is required either way) **plus**
the maintenance surface of carrying a fork across `rust-nostr/nostr`
version bumps. The fork has to track `nostr 0.44 → 0.45 → ...` upstream
releases — NMP currently pins `nostr = "0.44"`, so the maintenance is
deferred but not avoided.

**Pros:** unblocks T136b immediately; gives a working PR diff to file
upstream in parallel.

**Cons:** non-trivial maintenance; the ingester-refactor lives in a
private fork rather than upstream review.

### Option C — Drop `nostr-lmdb`, hand-roll directly on `heed`

**Scope:** keep `crates/nmp-core/src/store/lmdb.rs` as the implementation
home; depend on `heed = "0.20"` directly; re-implement the indexes
`nostr-lmdb` provides (ci, akc, ac, kc, atc, ktc, tc + deleted_ids +
deleted_coordinates — see `store/lmdb/mod.rs:73-98`) ourselves.

**Cost:** ADR-0011 §"Alternatives B" estimates 2 000+ LOC of NIP-09 /
replaceable / addressable logic plus the index machinery. The
`store/lmdb/mod.rs` file in upstream is 1 429 LOC plus 411 LOC in
`ingester.rs` plus 278 LOC in `lmdb/index.rs` — call it ~2 100 LOC of
mechanism we would be re-deriving.

**Pros:** zero upstream dependency for the env-injection seam — NMP owns
the entire `heed::Env` lifecycle by construction. No PR latency.

**Cons:** the largest one-shot LOC commitment of the four options; the
NIP-09 policy bugs `nostr-lmdb` has already shaken out (see e.g.
`test_kind5_deletion_query_bug_fix` in upstream tests) become NMP's bugs
to find. ADR-0011 alternatives §B previously rejected this on "battle-
tested logic for free" grounds; that argument **weakens** once options
A+B both require touching the policy layer anyway.

### Option D — Two-env fallback (rejected, listed for completeness)

ADR-0011 §"Two-phase-write fallback" already rejected this on
write-amplification + recovery-window-ambiguity grounds. Not a viable
choice; mentioned only so anyone reading this doc later does not
re-propose it.

## 4. Recommendation surface (decision deferred to user / T136a)

A clean reading of the four options:

- **If upstream maintainer relationship is cheap and time is plentiful:** A,
  then B as bridge if PR latency stretches.
- **If T136 needs to unblock M3 within this milestone window:** B (carry
  the fork; file the PR; replace when merged). The ingester refactor is the
  same code either way; B just lets us land it without waiting.
- **If we want maximum independence from upstream churn:** C. The "battle-
  tested" advantage of `nostr-lmdb` shrinks once we are committed to
  refactoring its ingester regardless.

**This doc does not pick.** Picking is T136a's job and requires user input
(or autonomous-mode decision per memory `autonomous-mode.md` if the user
is unavailable). Logging to `docs/perf/pending-user-decisions.md` is
appropriate here.

## 5. What was NOT done in T136 Gate 1 (and why)

- **`cargo add nostr-lmdb`** — task spec lists it as Gate-1 step 1, but
  adding an unused dep on a STOP path leaves a stale entry. Deferred to
  whichever option is chosen.
- **Gate 2 (33 method implementations)** — explicitly STOP'd per task
  spec ("Gate 1 alone is enough to make progress").
- **Gate 3 (kernel wiring)** — depends on Gate 2.
- **Gate 4 (crash-restart test)** — depends on Gates 2-3.

## 6. Follow-ups (not blocking T136a)

1. **ADR-0011 type names:** ADR-0011 says `lmdb::Environment` /
   `lmdb::RwTxn`; the real crate is `heed::Env` / `heed::RwTxn`.
   Single-line corrections, but worth doing once the option is chosen
   so the ADR matches the implementation.

2. **`crates/nmp-core/src/store/lmdb.rs:23-27` doc comment** still
   describes the rejected two-env model ("two separate heed environments
   ... Atomicity across the two environments is best-effort with startup
   repair"). This contradicts ADR-0011 and was already obsolete when
   the skeleton was committed. Fix as part of T136b's first commit.

3. **`docs/design/lmdb/trait.md` §5** references
   `nostr_lmdb::NostrLMDB + NMP sub-dbs` for the LMDB backend — that
   stays correct under A/B but becomes "wraps `heed::Env` directly"
   under C. Update once the option is chosen.

## 7. Sources

- Upstream crate audited: `nostr-lmdb-0.44.1` at
  `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/nostr-lmdb-0.44.1/`
- Master branch confirmed via
  `https://raw.githubusercontent.com/rust-nostr/nostr/master/database/nostr-lmdb/src/lib.rs`
- PR tracker search:
  `https://github.com/rust-nostr/nostr/pulls?q=is%3Apr+nostr-lmdb+env`
  (0 open, 0 closed as of 2026-05-18)
- Issue tracker search:
  `https://github.com/rust-nostr/nostr/issues?q=is%3Aissue+nostr-lmdb+env`
  (1 unrelated closed issue #969)
