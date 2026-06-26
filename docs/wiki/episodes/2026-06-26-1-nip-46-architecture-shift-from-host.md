---
type: episode-card
date: 2026-06-26
session: 1077a92b-e2b0-457d-870e-5e12e4f524cf
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1077a92b-e2b0-457d-870e-5e12e4f524cf.jsonl
salience: architecture
status: active
subjects:
  - nip-46
  - nmp-signer-broker
  - nmp-nip46
  - browser-nip46
  - transport-seam
supersedes: []
related_claims: []
source_lines:
  - 3509-3532
  - 3574-3612
  - 3612-3638
  - 3661-3894
  - 3909-4068
captured_at: 2026-06-26T08:04:10Z
---

# Episode: NIP-46: Architecture shift from host-brokered to transport-agnostic protocol core

## Prior State

NIP-46 support implemented via host-brokering for wasm (PR #2117: browser relies on JS app to handle relay; native spawns per-sign OS threads in BunkerBroker). Protocol logic entangled with execution (threads, Pool, intake queue) in nmp-signer-broker. Crypto/message-format half reusable; transport/RPC-session half requires platform-specific reimplementation.

## Trigger

User architectural critique (lines 3509-3532): 'Shouldn't NIP-46 own NIP-46 concerns and leave transport/process spawning to something else? Why would NIP-46 even spawn another process?' Codex design review (3599-3612) confirmed the entanglement is real and wrongly-modeled — NIP-46 conflates pure protocol (handshake state, id correlation, NIP-44 envelope) with transport/execution (sockets, threads, connection pools).

## Decision

Close PR #2117 (host-brokered approach; wrong architecture). Adopt multi-step decoupling: extract transport-agnostic `nmp-nip46` protocol core owning only handshake sequencing, RPC envelope build/parse, and id correlation (no sockets, threads, execution). Protocol emits effects (`SendFrame`/`Subscribe`/`Progress`); both native and browser drive it via minimal `FrameSink` trait over their existing transports (actor relay lane for native, BrowserRelayDriver for browser). Track as issue #2119 with explicit 6-step sequence: (1) extract core, (2) rewrite blocking handshake to event-reducer, (3) define Effect enum, (4-5) remove broker's thread/socket/intake machinery, (6) make SignerReady carry transport-effect-emitting signer.

## Consequences

- PR #2117 closed (host-brokered wasm approach abandoned)
- Issue #2119 created to durably track 6-step decoupling (extracted design doc captures full architecture)
- Step 1 implemented as PR #2123: new nmp-nip46 crate with FrameSink trait, broker becomes thin adapter, wire-builder deduped
- Browser NIP-46 support deferred pending completion of Steps 2-6
- Cross-cutting refactor impacts nmp-signer-broker (shared by nmp-ffi and native apps); ABI preserved via re-export facades to minimize churn
- Codex review enforced behavior-preservation: error-string drift caught and corrected; signed-wire bytes verified identical to prevent silent NIP-46 failures
- Step 1 plan explicitly stages blocking handshake rewrite to non-blocking reducer in Step 2, removing crossbeam-channel dep from core

## Open Tail

- Steps 2-6 implementation pending (event-reducer rewrite, effect-enum design, thread/socket/intake removal, per-RPC transport-effect emission)
- Decision on response-correlation parse location (currently split across nmp-signers and nmp-signer-broker; STEP 6 candidate for consolidation)
- Timeline decision: user chose to decouple NIP-46 now rather than defer relative to CI-gates (#2053/#2081/#2082) and Chirp rebuild (#2038)

## Evidence

- transcript lines 3509-3532
- transcript lines 3574-3612
- transcript lines 3612-3638
- transcript lines 3661-3894
- transcript lines 3909-4068

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-1-nip-46-architecture-shift-from-host.json`](transcripts/2026-06-26-1-nip-46-architecture-shift-from-host.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-1-nip-46-architecture-shift-from-host.json`](transcripts/raw/2026-06-26-1-nip-46-architecture-shift-from-host.json)
