---
type: episode-card
date: 2026-05-19
session: cb671af9-5784-4174-9c3d-d10151d9fb01
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/cb671af9-5784-4174-9c3d-d10151d9fb01.jsonl
salience: architecture
status: active
subjects:
  - no-polling-doctrine
  - d8-reactivity
  - event-driven-architecture
supersedes: []
related_claims: []
source_lines:
  - 1-1
  - 44-119
  - 272-295
  - 396-413
  - 415-432
  - 517-587
captured_at: 2026-06-18T04:34:35Z
---

# Episode: No-polling doctrine: eliminate all polling, codify full-stack prohibition

## Prior State

Polling anti-patterns existed in 6 places across the codebase (3 Rust: recv_timeout(0ms) spin-wait, poll()+sleep(10ms) test helper, try_recv()+sleep(50ms) worker loop; 3 iOS: 2s Task.sleep refreshDiagnostics loop, 0.25s Timer polling detectedBoxes, 5s sleep loop for position persistence). The D8 reactivity anti-pattern only forbade UI→kernel polling, not internal Rust channels or iOS background timers.

## Trigger

User directive: 'I hate polling and I think its a total disaster and should not be used ever anywhere'; comprehensive codebase search found 6 instances across Rust and iOS layers.

## Decision

Eliminate all 6 polling patterns and replace with proper blocking or event-driven primitives (blocking recv(), SignerOp::wait(), metadata delegate callbacks + debounced clear, wall-clock-gated periodicTimeObserver). Codify the prohibition as a full-stack invariant: expanded D8 doctrine row, broadened 06-reactivity-contract anti-pattern from UI-only to all layers, added top-level section in AGENTS.md, and wrote a memory note for future sessions.

## Consequences

- nmp-signer-broker relay_client.rs: recv_timeout(0ms) drain loop → try_recv() (semantic fix, same drain behavior, no spin)
- nmp-signers nip46 handle.rs: poll()+sleep(10ms) → SignerOp::wait() (uses existing API that already does the right thing)
- nmp-repl fanout.rs: try_recv()+sleep(50ms) worker spin → blocking recv() (threads sleep in kernel, not userspace)
- NetworkSettingsStore.swift: 2s polling refreshDiagnostics → applyStatus() event hook only (event bridge already existed)
- BookScannerModel.swift: 0.25s Timer polling detectedBoxes → accumulate in metadata delegate + 500ms debounced Task.sleep clear
- PodcastPlayerStore.swift: 5s sleep loop → wall-clock gate inside existing 0.25s periodicTimeObserver
- D8 doctrine row now explicitly lists sleep+poll loops as forbidden alongside allocations and false wakes
- AGENTS.md and memory note ensure all future agent sessions see the prohibition on dispatch

## Open Tail

- PodcastPlayerStore position persistence via periodicTimeObserver is borderline acceptable but not purely event-driven (still timer-based, just piggybacking an existing observer)

## Evidence

- transcript lines 1-1
- transcript lines 44-119
- transcript lines 272-295
- transcript lines 396-413
- transcript lines 415-432
- transcript lines 517-587

