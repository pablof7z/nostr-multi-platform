---
name: nmp-app-architecture
description: Enforce Rust Multi-Platform (RMP) and Nostr Multi-Platform (NMP) application architecture. Use when creating, modifying, reviewing, or auditing NMP/RMP apps, app templates, platform shells, Rust app kernels, FFI boundaries, capability bridges, Nostr app modules, or performance-sensitive reactive views; use for requests about "proper architecture", "doctrine", "native logic", "Rust core", "multiplatform app", "RMP", "NMP", "app scaffold", "architecture review", "performance gate", or "clean implementation".
---

# NMP App Architecture

This skill turns RMP's Rust-core/thin-shell architecture and NMP's stricter doctrine into a hard review and implementation workflow. Treat these rules as gates, not preferences: a solution that works but violates architecture, privacy, replayability, or performance is not done.

## Load Order

1. If working inside the NMP repo, read the repo instructions first: `AGENTS.md`, then any task-relevant durable docs. The authoritative crate-graph spec is `docs/architecture/crate-boundaries.md`; the ADR ledger is `docs/decisions/README.md`.
2. Read `references/rules.md` before making or approving architecture decisions. It carries the RMP/NMP baseline, the D-rules, and — under "The 2026 Redesign Spine" — an index into the deep-dive references below.
3. Read the deep-dive reference(s) for the area you are touching (FFI/native surface, composition, read sessions, projections/emission, write intents, runtime/capability/shell boundary, crate layers, protocol crates, governance). Do not design or review in an area without its reference.
4. For review or audit work, also read `references/review-rubric.md`.
5. For codebases on disk, run the scanner early:

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
   - Name the single writer for each fact, and the layer (L0–L6) each new type lives at.
   - Name the action/event that introduces each nondeterministic input.
   - Name the typed read session that owns the read, and the typed projection it emits across the UniFFI/wasm boundary.
   - For writes, name the typed intent, the actor stage that finalizes/signs it, and its route provenance.
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
   - In NMP, run `cargo test -p nmp-testing --test doctrine_lint_smoke` for doctrine gates (this enforces D0–D27 + product_raw_read; the scanner only complements it across languages and external repos).
   - For UniFFI interface or facade changes, run the bindings-drift gate and `uniffi-bindgen generate --library` for Swift and Kotlin.
   - Run performance/reactivity gates when hot paths, projections, view updates, snapshots, queues, or FFI cadence change.
   - For frontend/native shell changes, visually or interactively verify the app feels native and has no jank, blank states, or broken accessibility basics.

## Required Response Shape

When reviewing, lead with blocking findings and file/line references. When implementing, state which rules the design discharges and which gates were run. Never describe an architectural violation as "acceptable for now".

## References

Core:
- `references/rules.md`: hard RMP/NMP rules, the D-rules, and the index to the 2026 redesign spine.
- `references/review-rubric.md`: structured checklist for audits and PR reviews.
- `scripts/nmp_architecture_scan.py`: cross-language static triage that complements doctrine-lint.

Deep dives (read the one for the area you touch):
- `references/ffi-and-native-surface.md`: UniFFI as the sole native ABI, wasm-bindgen browser, FlatBuffers-through-UniFFI, app-owned facades.
- `references/composition-and-product-policy.md`: explicit composition, `register_defaults()` killed, `nmp-defaults` as installer library.
- `references/read-sessions.md`: typed read sessions; `open_interest`/`ObservedProjection`/`ReducedSource` as private substrate.
- `references/projections-and-emission.md`: typed projections, incremental diff-frame emission, the registration seam.
- `references/write-intents-and-publishing.md`: the one write door, dispatch≠success, composable drafts, typed route provenance.
- `references/runtime-capability-shell-boundary.md`: three-tier runtime, capability port contract, shell/headless boundary.
- `references/crate-layers-and-inversion.md`: the L0–L6 layer model and the layer-inversion rule.
- `references/protocol-crates-and-kind-blind-transport.md`: protocol-crate purity, D0 scope, NIP-29 kind-blind transport.
- `references/doctrine-governance-and-enforcement.md`: ADR ledger, rolling ratchets, doctrine-lint vs scanner, escape hatches.
