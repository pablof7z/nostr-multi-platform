# Codex review — #1493 P7: NIP-17 DM-inbox relays bypass selection pruning

Commit: `3df70024e`
Reviewer: codex `gpt-5.5` (reasoning effort: high)
Date: 2026-06-18

## Scope reviewed

Diff adding `RoutingSource::Nip17DmRelay` to `relay_bypasses_selection`
in `crates/nmp-planner/src/selection.rs`, plus regressions in
`crates/nmp-planner/src/selection/dm_relay_tests.rs`.

## Verdict

No blocking or important findings.

- **Correctness:** Adding `RoutingSource::Nip17DmRelay` to
  `relay_bypasses_selection` matches the existing pinned-lane model. A
  kind:10050 DM inbox relay is required by routing/privacy semantics, not
  optional coverage capacity — it must bypass the kind:10002 outbox-storm
  optimizer exactly like Hint/Provenance/AppRelay.
- **Tests prove the bug:** The main regression constructs exactly the
  failure shape — NIP-65 relays consume `max_connections`, the DM relay
  carries an empty-author `#p` wildcard, and wildcard backfill cannot run.
  Without the bypass, the DM relay drops. Verified locally: all 3 new tests
  FAIL without the one-line bypass addition and PASS with it.
- **Dual-lane case correct:** `role_tags` are additive; bypass projection
  preserves the relay plan unchanged. Keeping a `Nip65 + Nip17DmRelay`
  relay's NIP-65 author sub-shape unchanged is consistent with the pinned-
  lane contract (AppRelay/Hint/Provenance); narrowing it would create a
  separate policy for required relays.
- **Style/doctrine:** Fine. One mechanical caution — `selection.rs` is now
  499 LOC, just under the 500-LOC hard ceiling. Accepted: the in-repo
  doc-comment linter enforces the fuller bypass rationale, and the
  file-size gate passes (`exit 0`, hard cap 500). Full rationale lives in
  `selection/dm_relay_tests.rs` to keep the production file lean.

## Notes

The two design findings codex flagged in the prior design-first pass
(NIP-29 `PublishPlan` → `RoutingContext::explicit_targets`, and the dead
`explicit_targets` seam) require kernel publish-path changes outside this
lane's editable file set and were reported back to the team lead as a
separate publish-path lane decision.
