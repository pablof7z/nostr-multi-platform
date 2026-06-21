# Architecture Decision Records — status index

This directory holds NMP's Architecture Decision Records (ADRs). **Each ADR is a
point-in-time decision**, not a living document: it records the call that was made
and the reasoning at that moment. When a later decision changes the picture, the
old ADR is *superseded* or *amended* by a newer one rather than rewritten —
git history and `docs/wiki/` hold the full lineage. Read an ADR for *why a thing
was decided*, then check the "Superseded / amended by" column here (and the
authoritative current doctrine in [`docs/aim.md`](../aim.md)) for whether it still
stands as written.

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

| Number | Title | Status | Superseded / amended by |
|---|---|---|---|
| 0001 | Composite dependency keys as primary reverse-index entries | Accepted | — |
| 0002 | Delta-volume budget is per-view, not absolute | Accepted | — |
| 0003 | Memory budget is for working set, not total cached events | Accepted | — |
| 0004 | Allocation measurement plumbed via counting allocator | Accepted | — |
| 0005 | Domain-keyed platform shadow with refcounted component wrappers | Accepted | — |
| 0006 | Vertical-slice-first delivery for Phase 1 | Accepted (positioning modified by 0009) | Positioning amended by 0009; demo target partly superseded by 0008 |
| 0007 | Diagnostics and non-Nostr domain data over the app bridge | Accepted | — |
| 0008 | Initial Chirp social baseline on iOS as the Phase 1a demo target | Accepted (positioning modified by 0009) | Supersedes 0006 in part (demo-target choice only); positioning amended by 0009 |
| 0009 | App-extension kernel boundary | Accepted | — |
| 0010 | Generated per-app concrete enums at the FFI boundary | Accepted | — |
| 0011 | NMP owns the LMDB environment and injects it into nostr-lmdb | Accepted | — |
| 0012 | `relay_pin` and the third routing lane | Accepted | Extended by 0020 (intent-classed routing) |
| 0013 | NIP-29 metadata-signer trust model | Accepted | — |
| 0014 | LMDB write-path policy — MemEventStore canonical, fork compensates | Accepted | (renumbered from 0012) |
| 0015 | M6 signer trait, IdentityModule, and crate boundary | Accepted | — |
| 0016 | F-TTL FFI surface — `force` argument on the claim functions | Accepted | (renumbered from 0041) |
| 0017 | D1 placeholder contract: `Placeholder<T>` newtype | Accepted | — |
| 0018 | ContentTree FFI wire projection (`ContentTreeWire`) | Accepted | — |
| 0019 | Failed NIP-42 AUTH is fail-closed | Accepted | — |
| 0020 | Intent-classed routing + NIP-50 search | Accepted | Extends 0012 |
| 0021 | Relay roles: Indexer + AppRelay | Accepted | — |
| 0022 | NMP owns its relay transport (not `nostr-sdk` relay pool) | Accepted | — |
| 0023 | `HttpCapability` over the synchronous capability socket | Accepted | Async variant added by 0024 |
| 0024 | Async capability protocol for non-blocking HTTP executors | Superseded by 0040 (capability-worker seam) | 0040 |
| 0025 | Marmot bespoke FFI cluster: named exception | Superseded (write path retired; read path retained) | Write path → `dispatch_action` (0027 direction) |
| 0026 | Signer NIP-44 seal seam (`RemoteSignerHandle` nip44 verbs) | Implemented (trait verbs survive; seal-execution model retired) | Seal-exec model superseded by 0050 §D5 |
| 0027 | Unify the `ActionModule` trait | Accepted (implemented) | — |
| 0028 | Actor-liveness probe FFI (`nmp_app_is_alive`) | Accepted | — |
| 0029 | Bounded actor command channel + shed-load policy | Accepted (decision; bound never built in production) | Never-implemented as specified |
| 0030 | UniFFI vs C-ABI: the two-surface binding decision | Accepted | — |
| 0031 | `nmp-signer-broker` owns the NIP-46 relay transport | Accepted | — |
| 0032 | Backend sends raw data; presentation layers format | Accepted | Superseded by aim.md §2 (raw-data doctrine); partial completion by 0041 |
| 0033 | NMP feed viewport FFI | Accepted | — |
| 0034 | Kind-dispatched content rendering with open widget registry | Accepted | — |
| 0035 | Generic root-indexed feed engine in `nmp-feed` | Accepted | — |
| 0036 | Composition-root expansion of the follow-set timeline | Accepted (Revision 2 supersedes its own interest-expansion design) | Self (Rev 2) |
| 0037 | Typed FlatBuffers sidecar for high-volume runtime projections | Accepted/implemented (early compat text superseded) | 0044 + PR-B/F-05/F-10 work |
| 0038 | Typed FlatBuffers sidecar for the OP-centric home feed | Accepted/implemented | — |
| 0039 | The push projection seam is canonical | Accepted | Amended / partially superseded by 0053; reconciled by 0058 |
| 0040 | Capability-worker seam (signer/IO off the actor thread) | Accepted (V-90 closed) | Supersedes 0024 |
| 0041 | Relay-settings cluster: strip presentation strings | Decided | Partial completion of 0032 |
| 0042 | M2: generic `open_interest` replacing per-verb feed primitives | Accepted (mechanism); read-path finalized by 0057 | Read path finalized/amended by 0057 |
| 0043 | `nmp-blossom` protocol crate | Proposed | — |
| 0044 | Typing the Tier-3 top-level snapshot envelope fields | Accepted/implemented | — |
| 0045 | Store→projection replay (offline / second-launch render) | Accepted pending implementation (Rev 2/3: single always-on cache-serve) | Self (Rev 2/3 single-mechanism) |
| 0046 | Composition is a library, not a generator | Accepted | — |
| 0047 | NMP browser worker runtime contract | Accepted | Write/signing contract → 0064 |
| 0048 | NIP-55 Android signer (Amber) via `ExternalSignerCapability` | Accepted (shipped) | — |
| 0049 | Defaults yield; composition is observable | Accepted | — |
| 0050 | Signer-session capability port | Accepted pending implementation | §D5 supersedes 0026 seal-exec model |
| 0051 | First-class NIP-11 relay-information documents in NMP | Proposed | — |
| 0052 | Instance-scoped extension seams — register values, not types | Proposed | — |
| 0053 | Host-declared projection subscriptions | Accepted | Amends / partially supersedes 0039 |
| 0054 | Web persistence (OPFS-SQLite sync VFS) + offline-queue durability | Accepted (Stage 5; Stages 6–9 queued) | — |
| 0055 | Incremental projection emission (per-projection revision transport) | Accepted (implemented; Rungs 0-3 + capstone) | Amends aim.md §10 + Doctrine #12 |
| 0056 | K3 coverage ledger | Accepted (Stage A landed; B–E queued) | — |
| 0057 | Unified kind-agnostic accepted-event ingest chokepoint | Proposed | Amends 0042 (finalizes its read path) |
| 0058 | Cursor-based event-log consumption (the "pull" model) | Accepted pending implementation | Reconciles 0039 (does not reverse it) |
| 0059 | Account lifecycle is separate from bootstrap publish | Accepted pending implementation | — |
| 0060 | NIP-29 admin actions and joined-groups projection | Accepted pending implementation | — |
| 0061 | NIP-22 comments | Accepted pending implementation | — |
| 0062 | Observer-scoped read-model catch-up | Proposed | — |
| 0063 | Reference resolution: unified keyed `RefResolver` primitive | Accepted | Supersedes/amends 0042; extends 0053, 0055 |
| 0064 | Unified write/command boundary: one byte transport, open FlatBuffers payloads, signing as a capability round-trip | Accepted pending implementation | Extends 0027, 0050, 0040; folds in the worker write/signing contract from 0047 |
