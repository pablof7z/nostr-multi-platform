---
type: episode-card
date: 2026-05-26
session: 37e351ee-aa2b-43eb-9793-482de338f883
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/37e351ee-aa2b-43eb-9793-482de338f883.jsonl
salience: architecture
status: active
subjects:
  - ffi-abi-guard
  - ci-ffi-drift
supersedes:
  - 2026-05-26-2-flatbuffers-transport-contract-invariants-version-pins
related_claims: []
source_lines:
  - 39-81
captured_at: 2026-06-18T05:53:12Z
---

# Episode: FFI UpdateCallback ABI explicitly guarded in CI

## Prior State

The FFI header drift check caught symbol additions/removals but deliberately ignored C function signatures, so a future regression changing UpdateCallback's parameter types (e.g., const char*/length) would pass CI as long as the symbol name stayed the same.

## Trigger

Review feedback noted that the existing symbol-level drift check was insufficient to catch ABI regressions on the hot update callback path.

## Decision

Added check_update_callback_abi() that explicitly pins both the Rust FFI UpdateCallback type definition and the C header typedef/signature for UpdateCallback and nmp_app_set_update_callback across all three iOS app headers.

## Consequences

- Any change to the UpdateCallback signature or nmp_app_set_update_callback declaration will fail CI even if the symbol name is unchanged
- The Rust-side and C-side signatures must now stay in lockstep

## Open Tail

*(none)*

## Evidence

- transcript lines 39-81

