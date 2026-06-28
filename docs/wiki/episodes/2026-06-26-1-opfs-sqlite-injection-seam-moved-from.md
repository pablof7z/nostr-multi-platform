---
type: episode-card
date: 2026-06-26
session: 2b86015b-6b6b-44e9-a870-3b16c0763d7f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2b86015b-6b6b-44e9-a870-3b16c0763d7f.jsonl
salience: architecture
status: active
subjects:
  - wasm-opfs-storage-injection
  - nmp-browser-runtime-lifecycle
  - async-before-start-seam
supersedes: []
related_claims: []
source_lines:
  - 73-82
  - 115-127
  - 106-275
  - 134-142
  - 172-199
  - 277-279
captured_at: 2026-06-26T12:00:32Z
---

# Episode: OPFS-SQLite injection seam moved from nmp-wasm to nmp-browser-runtime (ADR-0067 supersedes ADR-0054)

## Prior State

ADR-0054 (2026-06-13) placed OPFS-SQLite injection in nmp-wasm's RawWasmAbiAdapter::set_injected_store. The in-flight feat/1007-opfs-sqlite branch was built on this architecture with a crate skeleton waiting to be completed.

## Trigger

Code audit of current master reveals ADR-0067 (2026-06-25, 'Supersedes-in-part ADR-0054') published the day before has already superseded the architecture: the live browser runtime is nmp-browser-runtime's NmpRuntimeCore → BrowserAppBuilder → BrowserRuntimeHandle, not nmp-wasm's RawWasmAbiAdapter (which is legacy/untested). The real injection seam is BrowserAppBuilder::inject_store. Additionally, there is currently no async hook before Start where OPFS can be opened, a gap ADR-0054 did not resolve.

## Decision

Adopt ADR-0067 as the authoritative architecture. nmp-browser-runtime owns 'browser storage initialization and lifecycle' (crate-boundaries.md §10a). Create a new async-before-Start seam: NmpWasmRuntime opens OPFS pool, stores Arc<dyn EventStore> on NmpRuntimeCore, handle_start consults it instead of hardcoding .in_memory(). Crate ownership: nmp-sqlite-wasm (new storage-engine crate Layer 0/1) owned by nmp-store contract layer, not the retained nmp-wasm protocol crate. Abandon stale branch; rebuild from current master with 9-PR breakdown grounded in ADR-0067.

## Consequences

- nmp-wasm exits the storage-injection path entirely; nmp-browser-runtime becomes the definitive storage lifecycle owner
- New async-before-Start seam fills an architectural gap—allows async OPFS pool open to complete before synchronous Start dispatch
- Crate dependency graph locked: nmp-sqlite-wasm → nmp-store; nmp-browser-runtime → {nmp-store, nmp-sqlite-wasm (wasm32-only)}; nmp-wasm uninvolved
- Feature placement: opfs-sqlite-backend feature on nmp-store, declared under [target.'cfg(target_arch="wasm32")'.dependencies], CI-gated to prevent native builds from enabling it
- Issue #1007 unblocked (injection seam ADR-0054 waited for has landed in ADR-0067) and reframed as storage lane of #2045 epic; 9-PR breakdown issued with explicit sequencing gates (PR-6 Worker-conformance vehicle must precede engine PRs 3/4/5)
- Prior art (stale branch) salvaged as design reference only for schema/codec/shim shapes; all wiring reimplemented against ADR-0067

## Open Tail

- Worker-only OPFS SAH constraint: createSyncAccessHandle exists only in dedicated Workers, not main thread. Conformance test vehicle cannot use wasm_bindgen_test's run_in_browser. Dedicated-Worker test runner (Playwright-driven) is a HIGH risk mitigation gate and must precede engine PRs.
- Multi-tab SAH pool-lock contention decision deferred: Web-Locks-single-durable-tab vs. explicit ephemeral-tier strategy for tab 2+ on same database_name must be decided before engine implementation to prevent silent degradation.
- Tag-index parity with LMDB (scan_by_tags tci/atci/ktci access paths) is the hardest schema risk; conformance must verify byte-for-byte equality of query results.

## Evidence

- transcript lines 73-82
- transcript lines 115-127
- transcript lines 106-275
- transcript lines 134-142
- transcript lines 172-199
- transcript lines 277-279

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-1-opfs-sqlite-injection-seam-moved-from.json`](transcripts/2026-06-26-1-opfs-sqlite-injection-seam-moved-from.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-1-opfs-sqlite-injection-seam-moved-from.json`](transcripts/raw/2026-06-26-1-opfs-sqlite-injection-seam-moved-from.json)
