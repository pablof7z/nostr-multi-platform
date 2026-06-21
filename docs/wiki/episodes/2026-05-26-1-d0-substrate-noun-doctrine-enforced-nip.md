---
type: episode-card
date: 2026-05-26
session: f26050da-6d8a-4128-9179-4088a9df94b9
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/f26050da-6d8a-4128-9179-4088a9df94b9.jsonl
salience: architecture
status: active
subjects:
  - d0-doctrine
  - app-host-trait
  - swap-dm-inbox-observer
supersedes: []
related_claims: []
source_lines:
  - 4023-4048
captured_at: 2026-06-18T06:04:59Z
---

# Episode: D0 substrate-noun doctrine enforced — NIP-specific name removed from AppHost trait

## Prior State

The `AppHost` substrate trait exposed `swap_nip17_dm_inbox_observer`, a NIP-17-specific noun, violating D0 doctrine which requires zero NIP knowledge in the nmp-core substrate layer.

## Trigger

Codex architectural assessment (run 1) identified a P1 D0 violation: `swap_nip17_dm_inbox_observer` appeared in app_host.rs, slots.rs, nmp-ffi/src/lib.rs (7 locations), nmp-app-template runtimes.rs, and Chirp nip17.rs tests.

## Decision

Renamed `swap_nip17_dm_inbox_observer` to `swap_dm_inbox_observer` across all call sites — the substrate API now uses a NIP-agnostic noun, and the protocol-specific context is documented only in doc comments, not in symbol names.

## Consequences

- D0 doctrine enforced: no NIP-specific identifiers in substrate traits
- All downstream callers (FFI layer, app-template, Chirp tests) updated to the generic name
- Doctrine grep gate for D0 now passes (zero hits for `swap_nip17_dm_inbox_observer`)
- PR #654 merged to master

## Open Tail

*(none)*

## Evidence

- transcript lines 4023-4048

