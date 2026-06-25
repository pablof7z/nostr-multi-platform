---
type: episode-card
date: 2026-05-22
session: 64c4fde3-6f5e-456a-b4bb-9f17517e301c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/64c4fde3-6f5e-456a-b4bb-9f17517e301c.jsonl
salience: architecture
status: active
subjects:
  - nmp-nip29
  - nmp-app-chirp
  - crate-boundary
  - thin-shell-rule
supersedes: []
related_claims: []
source_lines:
  - 1656-1680
  - 1793-1820
  - 1956-1992
  - 2129-2197
captured_at: 2026-06-18T05:07:37Z
---

# Episode: NIP-29 wiring relocated from Chirp shell to nmp-nip29 protocol crate

## Prior State

NIP-29 wiring functions (register_group_chat, register_group_discovery, register_nip29_actions) and the group-chat round-trip test lived in the Chirp app crate (nmp-app-chirp). These functions contained zero Chirp nouns — pure NIP-29 protocol logic — violating the D0 doctrine (nmp-core must not name NIP-29 nouns) and the thin-shell rule (app crates contain only C-ABI shims, no domain logic).

## Trigger

User rejection: 'That's a lot of chirp specific code for something that should be an nmp owned provided functionality; you MUST have violated a ton of repo rules!' — explicit architectural correction demanding the wiring move to the protocol crate.

## Decision

Created nmp_nip29::register module with three canonical wiring functions: wire_group_chat (observer + snapshot projection), wire_group_discovery, and register_actions (binds all 5 NIP-29 action modules). Chirp's FFI symbols became one-liner delegates: null-check → GroupId parse → nmp_nip29 call. The round-trip test relocated from apps/chirp/crates/nmp-app-chirp/tests/ to crates/nmp-nip29/tests/ with zero Chirp imports.

## Consequences

- D0 doctrine enforced: nmp-core never names NIP-29 nouns; composition happens at the app layer via protocol-crate-owned wiring
- Thin-shell rule restored: nmp-app-chirp/src/ffi.rs contains only C-ABI argument parsing that delegates to nmp_nip29::register
- Any future app crate (e.g. a web client) can call nmp_nip29::register directly without pulling in Chirp-specific code
- Round-trip test is now a protocol-crate test (66 nmp-nip29 tests pass), not an app-crate test; proves the stack independently of any shell
- All 42 Chirp tests still pass, confirming delegation preserved behavioral parity

## Open Tail

- NIP-17 DM inbox has the same structural violation (wiring in Chirp, not in nmp-nip17) — same refactoring pattern applies
- NIP-29 group_discovery round-trip test not yet written (only group_chat has end-to-end coverage)

## Evidence

- transcript lines 1656-1680
- transcript lines 1793-1820
- transcript lines 1956-1992
- transcript lines 2129-2197

