---
type: episode-card
date: 2026-05-19
session: fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/fe79b2c4-3f04-4fc9-8dde-08f19a3190b4.jsonl
salience: architecture
status: active
subjects:
  - marmot-ownership
  - chirp-thin-shell
  - nmp-marmot
supersedes: []
related_claims: []
source_lines:
  - 1-68
  - 70-70
  - 105-153
  - 157-159
captured_at: 2026-06-18T04:30:47Z
---

# Episode: Chirp thin-shell doctrine enforced — Marmot logic migrates to nmp-marmot

## Prior State

Marmot business logic (ops.rs, state.rs, publish.rs, tap.rs, payload.rs — 2180 lines) was embedded directly in nmp-app-chirp/src/marmot/, with the stated plan to 'extract into a standalone nmp-marmot crate post-v1'. Chirp contained MLS domain logic instead of being a thin FFI shell.

## Trigger

User forcefully corrected: 'chirp is supposed to have no fucking logic, the whole fucking point is to prove that this shit is reusable from any fucking app, if you put a bunch of fucking logic this makes zero fucking sense!' — the nmp-marmot crate already existed but was underused.

## Decision

All non-FFI Marmot code (ops, state, publish, tap, payload) migrates from nmp-app-chirp into nmp-marmot. Chirp retains only C-ABI FFI exports (ffi.rs, ffi/) that delegate to nmp-marmot. The prior 'deferred post-v1' plan is abandoned — the extraction is now.

## Consequences

- Chirp proves the reusability architecture: any future NMP app can use marmot without duplicating logic
- nmp-marmot is the sole owner of MLS business logic; Cargo.toml FFI exception comment is the only app-specific concession
- The nmp-marmot crate's existing projection/ directory structure (publish, state, tap, ops, payload) receives the migrated logic

## Open Tail

- The FFI translation-layer exception noted in Cargo.toml rustdoc remains — JSON/hex crossing the C-ABI boundary is still app-specific glue

## Evidence

- transcript lines 1-68
- transcript lines 70-70
- transcript lines 105-153
- transcript lines 157-159

