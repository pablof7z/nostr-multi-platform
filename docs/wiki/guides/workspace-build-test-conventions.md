---
title: Workspace Build & Test Conventions
slug: workspace-build-test-conventions
topic: developer-workflow
summary: Local cargo test runs must be scoped to the crates touched, not the whole workspace; cargo test --workspace is reserved for the merging agent and CI
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:019edc01-fdde-7b20-a348-5a2a9ce1a0f9
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
  - session:019edc0c-2dd1-7b80-b737-7499340e1b49
  - session:019edc16-8e40-7a92-9ea1-7405af0d34f3
  - session:019edc4d-4175-7441-b5af-cb2012068335
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edc92-b628-7ce1-be8a-c3d1013f2969
  - session:019edcc8-fa82-7c13-9736-ecf1337bc58c
  - session:019edcba-b578-71f3-be33-f670962f11a7
---

# Workspace Build & Test Conventions

## Local Testing & Build Conventions

Local cargo test runs must be scoped to the crates touched, not the whole workspace; cargo test --workspace is reserved for the merging agent and CI. The always-on local gates are cargo test -p nmp-testing --test doctrine_lint_smoke (for D-rule gates like D0, D15, D11, file-size, etc.) and cargo build --workspace (compile-only) when renaming a public symbol, moving a module, changing a Cargo.toml dep path, or adding a workspace member. New workspace crates must be added to the release manifest (release/nmp-release.toml) alongside the other public NIP crates. Agents must `cargo clean` their worktrees after each PR merge to manage disk space on the build host. Binary feature-gating must use an inner `#[cfg(feature="lmdb-backend")] fn run()` with a `main()` that conditionally calls it, never `#![cfg]` at the top (which causes a link error when the feature is off). The `CONVERSION_COUNT` `AtomicUsize` detector in `query_streaming.rs` must be widened from `#[cfg(test)]` to `#[cfg(any(test, feature = "test-support"))]` and re-exported as `pub` so integration tests in `nmp-testing` can reach it via `nmp_core::store`. Parallel test execution on the global `CONVERSION_COUNT` static causes race conditions; a `Mutex` serializer in the test file is required to make the materialization gate deterministic. The 500-LOC hard file-size cap is enforced; files exceeding it must be split into sibling modules declared in `mod.rs` (e.g., `query_streaming.rs` split from `query.rs`, `insert_kind5.rs` split from `insert.rs`), never by raising the baseline. Rust module declarations for peer files must go in `mod.rs`, not inside a source file (`mod foo` inside `bar.rs` looks for `parent/bar/foo.rs`, not `parent/foo.rs`). The p3 ops.rs literal-consolidation was reverted because it pushed the 500-LOC god-file over its file-size baseline (713→718), which caused a CI failure. CI must run `cargo test -p nmp-testing --features lmdb-backend` for the acceptance gates to take effect; the current `test.yml` workflow does not include this step and must be updated. P4 Findings 5 and 6 (web ProjectionMergeCache re-implementation and chirpConfig.ts relay defaults) are deferred as post-v1 follow-up issue #1546. CI's cargo test job does NOT compile apps/* crate tests (discovered when #1528 left nmp-app-chirp tests referencing removed fields and CI was green); every wire-shape PR must manually run cargo test -p nmp-app-chirp before pushing. Golden wire fixtures must be regenerated (including triplicated .fb.hex + Kotlin POPULATED_HEX + Swift populatedHex) and both shells compiled as part of every wire-shape PR. Follow-up issues filed: #1538 (P7 — unify dead explicit_targets vs live PublishTarget::Explicit), #1546 (P4 web — ProjectionMergeCache→wasm + config single-source), #1553 (CI blind spot — compile apps/* crate tests).

<!-- citations: [^019ed-32] [^129d2-48] [^019ed-50] [^019ed-64] [^129d2-69] [^019ed-89] [^129d2-96] [^11850-65] [^129d2-112] [^11850-90] [^129d2-120] [^11850-111] [^019ed-122] [^019ed-135] [^019ed-142] [^11850-155] [^11850-203] [^11850-222] [^11850-235] [^019ed-164] -->
