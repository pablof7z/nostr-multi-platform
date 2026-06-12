---
type: episode-card
date: 2026-06-12
session: da6b1d73-e1c8-4765-8ac7-056aa90fc154
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/da6b1d73-e1c8-4765-8ac7-056aa90fc154.jsonl
salience: root-cause
status: active
subjects:
  - ci-gap
  - visibility-promotion
  - claimed-profiles
supersedes: []
related_claims: []
source_lines:
  - 3697-3715
  - 3747-3767
captured_at: 2026-06-12T00:59:06Z
---

# Episode: Example-compile gap class — examples not built by workspace CI

## Prior State

`decode_claimed_profiles`/`CLAIMED_PROFILES_SCHEMA_ID` were `pub(crate)` and `#[cfg(test)]`-gated, never publicly exported. The PR's own example migration imported them, but this compiled under `cargo test` (which builds examples) while `cargo build --workspace` does not build examples.

## Trigger

CI `cargo test` (the long ~40min job) caught E0432 in `crates/nmp-app-template/examples/validate_claim_profile.rs`. The reviewer noted that `cargo build --workspace` and scoped `-p` runs never build examples, so this slipped two review rounds.

## Decision

Promote `decode_claimed_profiles`/`ClaimedProfilesModel`/`CLAIMED_PROFILES_SCHEMA_ID`/`FILE_IDENTIFIER`/`SCHEMA_VERSION` to `pub` (matching the `decode_claimed_events` precedent). Add `cargo build --workspace --examples` to the validation playbook.

## Consequences

- The claimed_profiles decode cluster is now on the public typed surface alongside claimed_events
- The validation gap class (examples not built by workspace builds) is now closed in the playbook — `--examples` must be included

## Open Tail

*(none)*

## Evidence

- transcript lines 3697-3715
- transcript lines 3747-3767

