# Architecture Decision Records

This directory contains only NMP architecture decisions that still govern the
codebase.

ADRs are not an archive. When a decision stops describing the architecture, any
surviving rule moves to its present owner and the obsolete file is deleted. Git
history, closed issues, and pull request bodies preserve the earlier context.

For the developer-facing overview, see
[`docs/architecture/high-level-app-architecture.md`](../architecture/high-level-app-architecture.md).

## Decision Spine

- [ADR-0069](0069-explicit-feature-composition.md) - Explicit feature
  composition and app-owned product policy.
- [ADR-0070](0070-typed-read-sessions.md) - Typed read sessions own
  app-visible read lifecycles.
- [ADR-0071](0071-write-intents-and-route-provenance.md) - Publish intents,
  composable drafts, and route provenance.
- [ADR-0072](0072-runtime-capability-and-shell-boundary.md) - Runtime,
  capability, and shell boundary.
- [ADR-0073](0073-adr-reset-and-rolling-ratchets.md) - ADR directory governance
  and rolling ratchets.

- [ADR-0067](0067-browser-runtime-ownership-split.md) - Browser runtime
  ownership split (nmp-wasm is ABI glue).

## Extensions

- [ADR-0074](0074-nip09-generic-deletion-ownership.md) - NIP-09 deletion
  artifact ownership.
- [ADR-0075](0075-trellis-private-reconciliation-substrate.md) - Trellis as
  private reconciliation substrate.
- [ADR-0076](0076-app-facing-feed-helpers.md) - App-facing feed APIs as typed
  read-session helpers, including composite multi-lane feeds (#3082/#3086).
