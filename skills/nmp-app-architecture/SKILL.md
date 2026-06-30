---
name: nmp-app-architecture
description: Enforce Rust Multi-Platform (RMP) and Nostr Multi-Platform (NMP) application architecture. Use when creating, modifying, reviewing, or auditing NMP/RMP apps, app templates, platform shells, Rust app kernels, FFI boundaries, capability bridges, Nostr app modules, or performance-sensitive reactive views; use for requests about "proper architecture", "doctrine", "native logic", "Rust core", "multiplatform app", "RMP", "NMP", "app scaffold", "architecture review", "performance gate", or "clean implementation".
---

# NMP App Architecture

This skill turns RMP's Rust-core/thin-shell architecture and NMP's stricter doctrine into a hard review and implementation workflow. Treat these rules as gates, not preferences: a solution that works but violates architecture, privacy, replayability, or performance is not done.

## Load Order

1. If working inside the NMP repo, read the repo instructions first: `AGENTS.md`, then any task-relevant durable docs.
2. Read `references/rules.md` before making or approving architecture decisions.
3. For review or audit work, also read `references/review-rubric.md`.
4. For codebases on disk, run the scanner early:

```bash
python3 <skill-dir>/scripts/nmp_architecture_scan.py <repo-or-app-root>
```

The scanner is a triage tool. Investigate every hit; do not treat a clean scan as architectural proof. It distinguishes real violations from legitimate boundaries: app/operator relay-policy config and one-shot native presentation timers surface as warnings, generated files are skipped (review their canonical source), and D6 fires only at the FFI boundary. The canonical allowances behind these severities are documented in `references/rules.md` under "Scanner Precision And Canonical Allowances".

## Workflow

1. Classify the change.
   - NMP framework substrate: shared reusable Nostr infrastructure belongs under `crates/`.
   - App-specific Rust: product-domain behavior belongs in the app's Rust crate, not in `nmp-core`.
   - Native shell: only rendering and platform capability execution belong in Swift, Kotlin, TypeScript, or desktop UI code.
   - Capability bridge: native executes OS APIs and reports raw results; Rust owns policy.

2. Draw the ownership boundary before editing.
   - Name the single writer for each fact.
   - Name the action/event that introduces each nondeterministic input.
   - Name the snapshot/projection that crosses FFI.
   - Name the tests or gates that prove the boundary.

3. Enforce the hard stops.
   - No native business logic.
   - No polling or sleep-check loops at any layer.
   - No duplicate representation or second source of truth.
   - No relay routing, privacy, retry, cache invalidation, signer, time, or protocol policy in the app shell.
   - No unbounded snapshot, event store, or history crossing FFI.
   - No temporary hack, TODO debt, stub, or "fix later" path unless a canonical backlog/ADR plan already exists.
   - No performance regression: 60fps native feel, bounded reactivity, zero post-warmup hot-path allocation where D8 applies.

4. Implement the clean architecture, not the local patch.
   - If the correct fix requires a new seam, module, crate, projection, action, or ADR, create it.
   - If app code wants a framework fact, add a typed Rust projection/action instead of letting native compute it.
   - If native needs an OS API, add a capability bridge with idempotent start/stop/restart and raw result reporting.
   - If a doctrine appears too strict, stop and document an ADR-quality invariant. Silent waivers are violations.

5. Verify.
   - Run the smallest relevant Rust tests for touched crates.
   - In NMP, run `cargo test -p nmp-testing --test doctrine_lint_smoke` for doctrine gates.
   - Run performance/reactivity gates when hot paths, projections, view updates, snapshots, queues, or FFI cadence change.
   - For frontend/native shell changes, visually or interactively verify the app feels native and has no jank, blank states, or broken accessibility basics.

## Required Response Shape

When reviewing, lead with blocking findings and file/line references. When implementing, state which rules the design discharges and which gates were run. Never describe an architectural violation as "acceptable for now".

## References

- `references/rules.md`: hard RMP/NMP rules and NMP evolution over RMP.
- `references/review-rubric.md`: structured checklist for audits and PR reviews.
- `scripts/nmp_architecture_scan.py`: static triage for common violations.
