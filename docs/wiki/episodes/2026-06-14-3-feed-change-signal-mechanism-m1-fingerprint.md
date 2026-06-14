---
type: episode-card
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: architecture
status: superseded
subjects:
  - feed-omit-mechanism
  - adr-0055-r6-s1
  - feed-emission-state
supersedes:
  - 2026-06-14-2-feed-idle-waste-measurement-58-8
related_claims: []
source_lines:
  - 9993-10092
captured_at: 2026-06-14T13:12:38Z
---

# Episode: Feed change-signal mechanism — M1 fingerprint-of-encoded-bytes, trap-proof by construction

## Prior State

The op_feed engine had no change-signal — it unconditionally snapshot+encodes every tick. Kernel manifest gating (Seam B, used for built-in projections) was the existing omission path, but the feed is host-registered and its state lives in its own Mutex outside the kernel's SourceVersions, making kernel manifest gating architecturally wrong for this projection.

## Trigger

Design analysis (Opus + codex) of three mechanisms: M1 (fingerprint encoded payload bytes), M2 (O(1) dirty counter), M3 (skip-gate). M1 is trap-proof by construction — the change-signal is a pure function of the exact bytes the host receives, so a missed bump cannot exist. M2 makes correctness depend on exhaustively hooking every current and future window-affecting mutation (including subtle profile-refresh-inside-a-visible-card), which is unacceptable on the riskiest surface. M3 reintroduces the same enumeration risk. Codex concurred on M1.

## Decision

M1 — fingerprint the encoded FlatBuffers payload bytes via exact byte-equality (memcmp, not hash — any collision probability = permanently frozen timeline). Omit in the producer closure (Seam A, `Ok(None)`), not kernel manifest. FeedEmissionState lives in the closure layer, engine `snapshot()` untouched. Monotonic per-epoch rev (not the hash) for host reorder guard. Capability-gated on `incremental_apply_handle: Arc<AtomicBool>` (lock-free to avoid SnapshotRegistry re-lock deadlock). Epoch change (account-switch) resets emission state — first post-epoch tick always emits full baseline.

## Consequences

- Feed omission is trap-proof by construction — a missed change-signal cannot exist because the signal IS the bytes the host would receive
- Engine remains pure materialization; emission decision is entirely in the closure layer (separation of concerns)
- Capability-OFF is byte-identical to today (should_emit always returns Some), so non-retaining hosts cannot be blanked
- Exact byte-equality on ~58.8 KB is trivial CPU cost; a hash would save ~58 KB memory but introduce nonzero collision risk = freeze
- 25 cardinal-trap tests (Group A must-emit, Group B false-resend, Group C host-coherence sim) backstop as regression guards
- Feed ships typed-sidecar-only; generic Value path is off-wire and not a correctness concern for omit

## Open Tail

- Opus adversarial review in flight: checking encode determinism (any relative-time field that defeats omission?), two-flags-for-one-fact smell (incremental_apply_handle vs registry flag), epoch-reset coverage beyond account-switch (load_older/pagination)
- R6-S2 (register feed + 2 cheap whole-value keys for gating), S3 (host ProjectionCache confirm), S4 (capstone: idle feed bytes → ~0), S5 (release/device measurement) still pending

## Evidence

- transcript lines 9993-10092

