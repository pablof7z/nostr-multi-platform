# Architecture Decision Records — status index

This directory holds NMP's Architecture Decision Records (ADRs). ADRs preserve
durable decision context, but the current rule must stay readable in the owning
ADR. When later work removes or reverses a mechanism, edit the ADR in place so it
describes the current design rather than preserving incorrect guidance.

This index is a navigation aid. The ADR file itself is authoritative for its own
status; if they disagree, fix the index.

## Numbering notes

- ADR **0014** (LMDB write-path policy) was originally filed as 0012, which
  collided with 0012 (`relay_pin` / third routing lane). It was renumbered to the
  free number 0014; the `relay_pin` ADR keeps 0012.
- ADR **0016** (F-TTL FFI surface) was originally filed as 0041, which collided
  with 0041 (relay-edit-row raw projection). It was renumbered to the free number
  0016; the relay-edit ADR keeps 0041.
- The ADR-0055 "Rung 3" family (pr-ladder, rung3, implementation-gates,
  s1b-cleared-signal) was collapsed into the single ADR-0055; the durable
  host-apply contract now lives in its "Host apply contract" section. The
  planning-time rung ladder is preserved in git history.

## Index

| Number | Title | Status | Related updates |
|---|---|---|---|
| 0001 | Composite dependency keys as primary reverse-index entries | Accepted | — |
| 0002 | Delta-volume budget is per-view, not absolute | Accepted | — |
| 0003 | Memory budget is for working set, not total cached events | Accepted | — |
| 0004 | Allocation measurement plumbed via counting allocator | Accepted | — |
| 0005 | Domain-keyed platform shadow with refcounted component wrappers | Accepted | — |
| 0006 | Vertical-slice-first delivery for Phase 1 | Accepted (positioning modified by 0009) | Positioning amended by 0009; demo target updated by 0008 |
| 0007 | Diagnostics and non-Nostr domain data over the app bridge | Accepted | — |
| 0008 | Initial Chirp social baseline on iOS as the Phase 1a demo target | Accepted (positioning modified by 0009) | Updates 0006 demo target; positioning amended by 0009 |
| 0009 | App-extension kernel boundary | Accepted | — |
| 0010 | Runtime registration at the FFI boundary | Accepted | — |
| 0011 | NMP owns the LMDB environment and injects it into nostr-lmdb | Accepted | — |
| 0012 | `relay_pin` and the third routing lane | Accepted | Extended by 0020 (intent-classed routing) |
| 0013 | NIP-29 metadata-signer trust model | Accepted | — |
| 0014 | LMDB write-path policy — MemEventStore canonical, fork compensates | Accepted | (renumbered from 0012) |
| 0015 | Signer crate boundary and session ownership | Accepted | — |
| 0016 | F-TTL FFI surface — `force` argument on the claim functions | Accepted | (renumbered from 0041) |
| 0017 | Missing display facts remain raw absent facts | Accepted / reconciled | 0032 |
| 0018 | ContentTree FFI wire projection (`ContentTreeWire`) | Accepted | — |
| 0019 | Failed NIP-42 AUTH is fail-closed | Accepted | — |
| 0020 | Intent-classed routing + NIP-50 search | Accepted | Extends 0012 |
| 0021 | Relay roles: Indexer + AppRelay | Accepted | — |
| 0022 | NMP owns its relay transport (not `nostr-sdk` relay pool) | Accepted | — |
| 0023 | HTTP work runs off the actor | Accepted | Refined by 0040 |
| 0024 | Async capability protocol for non-blocking executors | Accepted | Refined by 0040 |
| 0025 | Marmot read/lifecycle FFI exception | Accepted / narrowed | 0039 |
| 0026 | Signer NIP-44 encryption seam | Accepted / implemented | 0050 |
| 0027 | Unify the `ActionModule` trait | Accepted (implemented) | — |
| 0028 | Actor-liveness probe FFI (`nmp_app_is_alive`) | Accepted | — |
| 0029 | Actor queue observability and backpressure policy | Accepted | — |
| 0030 | UniFFI vs C-ABI: the two-surface binding decision | Accepted | — |
| 0031 | `nmp-signer-broker` owns the NIP-46 relay transport | Superseded | Superseded by the actor-lane design (#2119); `nmp-signer-broker` deleted, NIP-46 rides the actor `Pool` lane via `nmp-nip46-runtime` |
| 0032 | Backend sends raw data; presentation layers format | Accepted | Aligned with aim.md §2 raw-data doctrine; partial completion by 0041 |
| 0033 | NMP feed viewport FFI | Accepted | — |
| 0034 | Kind-dispatched content rendering with open widget registry | Accepted | — |
| 0035 | Generic root-indexed feed engine in `nmp-feed` | Accepted | — |
| 0036 | Composition-root expansion of the follow-set timeline | Accepted (Revision 2 is current) | Self (Rev 2) |
| 0037 | Typed FlatBuffers sidecars for runtime projections | Accepted / implemented | 0044 |
| 0038 | Typed FlatBuffers sidecar for the OP-centric home feed | Accepted/implemented | — |
| 0039 | Push projection seam is canonical | Accepted / amended | 0053, 0058 |
| 0040 | Capability worker seam | Accepted / implemented | 0024 |
| 0041 | Relay-settings cluster: strip presentation strings | Decided | Partial completion of 0032 |
| 0042 | M2: generic `open_interest` replacing per-verb feed primitives | Accepted (mechanism); read-path finalized by 0057 | Read path finalized/amended by 0057 |
| 0043 | `nmp-blossom` protocol crate | Accepted / implemented | — |
| 0044 | Typed snapshot envelope fields | Accepted / implemented | — |
| 0045 | Store→projection replay (offline / second-launch render) | Accepted / implemented (single always-on cache-serve) | Self (Rev 2/3 single-mechanism) |
| 0046 | Composition is a library, not a generator | Accepted | — |
| 0047 | NMP browser worker runtime contract | Accepted | Write/signing contract → 0064 |
| 0048 | NIP-55 Android signer (Amber) via `ExternalSignerCapability` | Accepted (shipped) | — |
| 0049 | Defaults yield; composition is observable | Accepted | — |
| 0050 | Signer-session capability port | Implemented | Updates 0026 seal-exec model; §D7 extended by 0066 |
| 0051 | First-class NIP-11 relay-information documents in NMP | Accepted / implemented | — |
| 0052 | Instance-scoped extension seams — register values, not types | Accepted / implemented | — |
| 0053 | Host-declared projection subscriptions | Accepted | Amends 0039 |
| 0054 | Web persistence (OPFS-SQLite sync VFS) + offline-queue durability | Accepted (implemented; Stages 5–9 shipped via #2147–#2165) | — |
| 0055 | Incremental projection emission (per-projection revision transport) | Accepted (implemented; Rungs 0-3 + capstone) | Amends aim.md §10 + Doctrine #12 |
| 0056 | K3 coverage ledger | Accepted / implemented | — |
| 0057 | Unified kind-agnostic accepted-event ingest chokepoint | Accepted / implemented | Amends 0042 (finalizes its read path) |
| 0058 | Cursor-based event-log consumption (the "pull" model) | Accepted / implemented | Reconciles 0039 (does not reverse it) |
| 0059 | Account lifecycle is separate from bootstrap publish | Accepted / implemented | — |
| 0060 | NIP-29 admin actions and joined-groups projection | Accepted / implemented | — |
| 0061 | NIP-22 comments | Accepted / implemented | — |
| 0062 | Observer-scoped read-model catch-up | Accepted (implemented) | — |
| 0063 | Reference resolution: unified keyed `RefResolver` primitive | Accepted | Amends 0042; extends 0053, 0055 |
| 0064 | Unified write/command boundary: one byte transport, open FlatBuffers payloads, signing as a capability round-trip | Accepted/implemented | Extends 0027, 0050, 0040; folds in the worker write/signing contract from 0047 |
| 0065 | `ActorCommand` sub-enum collapse | Accepted | Aligns the in-process command vocabulary with 0064 |
| 0066 | Delegated NIP-44 decrypt sessions for bunker DM backfill | Accepted for staged implementation | Extends 0050 §D7; NIP-46 transport principle from 0031 (superseded; now via nmp-nip46-runtime) |
| 0067 | Browser runtime ownership split (nmp-wasm is ABI glue) | Accepted | Amends 0047 and 0054 |
