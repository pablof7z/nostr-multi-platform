---
type: episode-card
date: 2026-05-27
session: cd2b6122-2b7c-43fc-941b-c51e79ffc691
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/cd2b6122-2b7c-43fc-941b-c51e79ffc691.jsonl
salience: root-cause
status: active
subjects:
  - nmp-backlog
  - d6-doctrine
  - silent-degradation
  - nmp-nip47
  - nmp-marmot
  - nmp-nostr-lmdb
supersedes: []
related_claims: []
source_lines:
  - 3473-3567
  - 3581-3603
  - 3614-3640
  - 3965-3999
captured_at: 2026-06-18T06:10:48Z
---

# Episode: Fallback/escape-hatch audit produces 19 new violations (V-61..V-79) with D6 doctrine boundary clarified

## Prior State

Numerous fallback/escape-hatch patterns and scaffolding existed across the codebase without systematic classification — many silent degradations were untracked, and the boundary between D6-doctrined fire-and-forget patterns and genuine violations was informal.

## Trigger

User directed two parallel audit waves: (1) 20 Haiku agents searching for fallbacks that mask symptoms or create technical debt, then Opus triage; (2) 10 Haiku agents searching for unjustifiable scaffolding, then Opus validation.

## Decision

31 fallback findings triaged into 16 new violations (V-61..V-76), 3 folded into existing entries (J→V-43, L→V-14, X→V-57), 1 found incorrect (media_cache misread), 11 classified as D6-doctrined or dev-tool acceptable and dismissed. Scaffolding audit then produced 3 more (V-77 MakeInvoice dead surface, V-78 bunker zap signing gap, V-79 wallet connection no heartbeat), with 2 findings folded (B→V-08, C intentional) and 2 dead-code items ruled not backlog-worthy (deleted instead). Critical violations now tracked: NIP-47 payment response silently swallowed, Marmot keyring failure silently installs mock store, Marmot PendingGroupChange::drop silently clears MLS commit, LMDB .ok()?? swallows corruption, NIP-47 unwrap_or_default sends empty-string payment frames.

## Consequences

- V-61..V-76 permanently catalog 16 silent-degradation patterns as violations with priority levels, replacing informal awareness with tracked debt
- D6 doctrine boundary now explicitly documented: fire-and-forget channel sends across dying-kernel boundary, D6 null-on-error FFI returns, and D15 panic-safety wrapping are acceptable; payment-frame serialization masking, LMDB error swallowing, and mock-store silent installation are not
- Three HIGH-priority violations (V-61 MLS state divergence, V-62 MLS secret loss, V-63 silent payment failure) flagged for immediate attention
- V-78 establishes bunker zap signing as its own violation distinct from V-06 (NIP-42 AUTH) and V-08 (DM inbox), closing an ownership gap

## Open Tail

- V-63/V-64 NIP-47 payment serialization and timeout sweep need implementation timelines
- V-67 kernel-init LMDB degradation to in-memory store needs a user notification path
- V-69 LMDB .ok()?? corruption opacity needs a tracing or error-propagation decision

## Evidence

- transcript lines 3473-3567
- transcript lines 3581-3603
- transcript lines 3614-3640
- transcript lines 3965-3999

