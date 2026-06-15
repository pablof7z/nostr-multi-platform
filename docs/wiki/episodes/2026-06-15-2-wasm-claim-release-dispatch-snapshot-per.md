---
type: episode-card
date: 2026-06-15
session: ab8061fc-b277-4ba4-bf55-1532bcb1aa90
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ab8061fc-b277-4ba4-bf55-1532bcb1aa90.jsonl
salience: root-cause
status: superseded
subjects:
  - wasm-dispatch
  - nmp-wasm-runtime
  - solidjs-render-loop
supersedes:
  - 2026-06-15-2-wasm-claim-release-dispatch-must-not
related_claims: []
source_lines:
  - 2196-2198
  - 2342-2357
  - 2388-2412
  - 2414-2419
  - 2479-2489
captured_at: 2026-06-15T03:41:03Z
---

# Episode: Wasm claim/release dispatch: snapshot-per-claim → ACK-only (no snapshot)

## Prior State

The wasm dispatch arm for claim_profile/release_profile pushed a fresh snapshot frame on every invocation, mirroring how other dispatch paths (like relay-pool ingest) worked.

## Trigger

After the registry migration PR #1436, the web Playwright feed test regressed (3/3 failures on branch, 8/8 green on master). CI logs showed fixture data arriving but posts/avatars never rendering, preceded by a storm of redundant overlapping REQs and a ~2.5-min stall. Local reproduction proved 0/8 passed on the branch vs 2/5 on master (tail-flake only). Investigation revealed an unbounded UI loop: 170k+ snapshot frames, 16k+ alternating claim/release calls, OOM-crashing the renderer.

## Decision

Claim/release dispatch now ACKs with ActionAccepted only and pushes no snapshot. Resolved kind:0 data arrives via the relay-pool ingest sink, which pushes its own snapshot independently. This mirrors the native actor behavior. A native regression guard (claim_no_snapshot_tests) was added asserting claim/release emit ActionAccepted with no UpdateBytes.

## Consequences

- Web feed test passes 10/10; full Playwright suite 3/3; vitest 42/42
- Eliminates the infinite SolidJS <For> re-render loop (claim → snapshot → remount → onMount claim → snapshot → loop)
- The reconnect-gating fix (524909a0) that preceded this was complementary but insufficient alone — the snapshot-per-claim was the actual engine of the loop
- runtime.rs exceeded the 500-LOC file-size hard cap after the fix, requiring extraction of dispatch.rs as a sibling module (518 LOC under baseline)

## Open Tail

*(none)*

## Evidence

- transcript lines 2196-2198
- transcript lines 2342-2357
- transcript lines 2388-2412
- transcript lines 2414-2419
- transcript lines 2479-2489
