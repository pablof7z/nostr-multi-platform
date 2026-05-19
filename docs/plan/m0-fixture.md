# M0 — Kernel substrate + non-Nostr fixture *(DONE)*

> Part of the [Build & Validation Plan](../plan.md). Arc 1 — Kernel substrate + Nostr social stack.

**Demo product:** `apps/fixture/nmp-app-fixture` — a TODO list app driven by the kernel substrate with no Nostr concepts in it.

**Scope.** Five extension trait families. Composite reverse index. Delta buffer with coalescing. Claim-based GC. Codegen producing a working per-app crate from a fixture module.

**Subsystem deliverables.** `nmp-core::substrate`, `nmp-codegen`, `fixture-todo-core`, `nmp-testing` harness skeletons.

**Exit gate.** ✅ Fixture compiles and runs; codegen determinism test passes; substrate registry test passes (`crates/nmp-core/tests/substrate_registry.rs`).

**Runnable artifact.** `cargo test --workspace`; the fixture module loads in any host.
