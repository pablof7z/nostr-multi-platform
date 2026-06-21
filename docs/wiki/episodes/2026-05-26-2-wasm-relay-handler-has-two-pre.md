---
type: episode-card
date: 2026-05-26
session: e4861768-9a00-4d83-b7a3-a39d07749d1c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/e4861768-9a00-4d83-b7a3-a39d07749d1c.jsonl
salience: root-cause
status: active
subjects:
  - nmp-wasm-relay-pool
  - wasm32-instant
  - refcell-double-borrow
supersedes: []
related_claims: []
source_lines:
  - 1805-2107
  - 2318-2321
  - 2666-2677
captured_at: 2026-06-18T05:55:20Z
---

# Episode: Wasm relay handler has two pre-existing crash bugs on real WebSocket connections

## Prior State

Browser wasm runtime was assumed to work for live relay connections; no prior diagnosis of relay-handler panics had been made visible (panics surfaced as opaque 'unreachable' with no message)

## Trigger

Live browser test of the rebuilt wasm bundle produced two distinct runtime panics when connecting to wss://relay.damus.io: (1) `std::time::Instant::now` panics with 'time not implemented on this platform' in `mark_lane_connected` (wasm32 has no monotonic clock), and (2) `RefCell already borrowed` in `relay_pool.rs:100` in the same `build_on_open` closure

## Decision

Both panics are in code paths untouched by the FlatBuffers PR (nmp-network browser_driver / nmp-wasm relay_pool); classified as pre-existing bugs, not regressions from this PR. A debug panic hook was temporarily added to surface panic messages, then discarded (not committed). PR merged without fixing these; browser crash tracked as separate issue

## Consequences

- Web platform is known-broken for real WebSocket relay connections until both bugs are fixed
- Instant::now on wasm32 requires a polyfill or architectural change (e.g., web_sys::performance::now or js-sys Date) in nmp-core's relay lifecycle code
- RefCell double-borrow in relay_pool build_on_open suggests the same pattern as the nip57 KernelClockAdapter bypass (commit 757fcaa5 on master) — recursive borrow during callback emission
- Panic hook installation pattern (wasm_bindgen extern for console.error) is validated as a useful debug tool for future wasm diagnosis

## Open Tail

- Need to confirm panic reproduces on master baseline (bisection not performed)
- Both the Instant polyfill and the RefCell re-entrancy fix need to be addressed before the web app is shippable

## Evidence

- transcript lines 1805-2107
- transcript lines 2318-2321
- transcript lines 2666-2677

