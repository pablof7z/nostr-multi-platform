# Decision log

> Part of the [Build & Validation Plan](../plan.md).

ADRs live in `docs/decisions/`. Format per the template in older revisions of this plan. Currently:

- **ADR-0001**: Composite dependency keys (composite-first reverse index; broad axes guardrailed).
- **ADR-0002**: Per-view delta budget (60/view/sec, not absolute).
- **ADR-0003**: Working-set memory budget (hot/cold split, not total events).
- **ADR-0004**: Allocation measurement via counting allocator.
- **ADR-0005**: Domain-keyed platform shadow + refcounted component wrappers.
- **ADR-0006**: Vertical-slice-first delivery (modified by ADR-0009; the slice now layers on the kernel substrate).
- **ADR-0007**: Diagnostics and non-Nostr data over the actor-owned bridge with explicit records, not raw callbacks or fake Nostr events.
- **ADR-0008**: Initial Chirp social baseline on iOS as the Phase 1a demo target (modified by ADR-0009 — repositioned as first canonical extension-module set).
- **ADR-0009**: App-extension kernel boundary. Five trait families, four layers, no app nouns in nmp-core.
- **ADR-0010**: Per-app concrete enums generated at the FFI boundary. Codegen is critical-path v1 infrastructure.

New ADRs land alongside any milestone whose execution revises a design.

## The harness-first pattern

Every design doc has measurable gates. Gates run on the reactivity-bench harness (or `firehose-bench` for end-to-end behavior). Failures revise the design **before** implementation. Pre-implementation measurement is cheaper than post-implementation rework. Run 001 of reactivity-bench established the pattern: the reverse-index direction was validated (100×–1000× headroom), one design refinement landed (composite keys), and two budget bugs surfaced (per-view delta, working-set memory) — all before any view-kind code shipped.

## Modeled budget contract vs runtime evidence

Two distinct claims about the same harness:

- **Modeled budget contract.** Replay mode runs deterministic synthetic workloads through a model of the runtime. Passing here proves budgets are internally consistent and the harness scaffolding is sound. Does **not** prove the real runtime hits those budgets.
- **Runtime evidence.** Live mode (or replay mode with real adapters substituted for modeled segments) runs against actual LMDB, actual WebSockets, actual UniFFI marshaling. Passing here is real evidence.

Each milestone moves the boundary rightward — replaces another modeled segment with a real adapter and graduates the corresponding firehose-bench scenarios from `modeled` to `measured` in `docs/perf/`.
