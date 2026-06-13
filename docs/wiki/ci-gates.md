---
title: CI Gates
slug: ci-gates
topic: ci-gates
summary: CI must include `cargo test -p nmp-app-template` and `cargo build --workspace --examples` in the test plan to prevent the example-compile gap class that let a `
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# CI Gates

## Test Plan

CI must include `cargo test -p nmp-app-template` and `cargo build --workspace --examples` in the test plan to prevent the example-compile gap class that let a `pub(crate)` visibility slip through twice. <!-- [^da6b1-12] -->

## File-Size Gate

The file-size gate requires splitting files approaching 500 LOC rather than bumping baselines, enforced by `check-file-size.sh` with a no-debt doctrine. File-size ratchet violations must be resolved by extracting cohesive modules (not raising baselines). New test files must also be split into separate submodules when they would push a file past the 500-LOC hard cap. A pre-emptive split of test files hovering near the 500-LOC cap eliminates the recurring round-trip where every fix PR tips a file over and needs a split before merge. PR #1295 extracts `settings_view` and `diagnostics_panel` (361 lines) from `app.rs` into a new `settings.rs`, reducing `app.rs` from 2172 to 801 lines (the split was needed for the file-size gate, though the file remains above the 500-line baseline). The push-event file-size CI workflow has a known gap: force-pushed branches produce a red check because the orphaned before SHA is unavailable in the clone; this should be fixed to fall back to the merge-base or skip with a notice.

<!-- citations: [^da6b1-13] [^02745-2] [^02745-28] [^02745-50] [^78c8e-45] -->
## Pre-existing CI Breakages

The `committed_registry_json_matches_generated_output` CI test currently fails on master (registry.json stale), causing `cargo test` red on multiple PRs that don't touch registry code; this is a pre-existing breakage. The AI architecture signoff CI check (#1267) is RED due to a bad `OPENAI_API_KEY` and is explicitly ignored per standing instructions. (Previously: The 'AI architecture signoff' CI check was a known-broken gate due to a bad `OPENAI_API_KEY` (#1267) and must be ignored when gating PR merges.) The podcast-player TestFlight pipeline has never shipped because CI never built the Rust core for the simulator architecture, causing an undefined symbol `_nmp_free_string` link failure that gated off the deploy step for 40 consecutive runs; PR #429 fixes this by prepending the sim-arch Rust build. (Previously: The podcast-player TestFlight pipeline failed for 40 consecutive runs because CI never built the Rust core for the simulator architecture, causing an undefined-symbol link failure that gated off the deploy job.) The compound `&&` merge command pattern swallowed exit codes and merged PR #1165 with a failing check; merges must gate on explicit pass/nonpass counts, never chain push after piped check. When merging any PR, the full cargo test lane must complete green before merging — 'no failures yet' does not constitute a passed check. (Previously: 'no failures yet' did not constitute a passed check; lesson from #1302 breaking master by merging on incomplete CI.) Ten of eleven needs-decision issues are determined by existing documented direction; only #1281 (whether `since=None` interests should be exempted from the T129 watermark rewrite) genuinely requires owner input.

<!-- citations: [^da6b1-14] [^02745-3] [^02745-29] [^02745-51] [^da6b1-44] [^da6b1-65] [^02745-115] [^02745-126] -->
## Android CI Gate

An Android `--features marmot` build was broken by a missing vendored OpenSSL dependency (libsqlite3-sys bundled-sqlcipher-vendored-openssl), a missing `zeroize` std feature, and a missing import—all uncaught by CI because the featureless lane was green. A CI gate was added to prevent regression. The `NMP_MARMOT_MOCK_KEYRING=1` environment variable remains as an escape hatch for headless/CI/repl contexts, unconditionally installing the in-memory mock store.

Marmot group relays are persisted by MDK (replace_group_relays on create/accept) and recoverable via get_relays after restart, enabling re-subscription without network. <!-- [^78c8e-82] -->

<!-- citations: [^78c8e-19] [^78c8e-46] -->
## Mechanism Census

Superseded mechanism generations must be deleted in the same milestone, enforced by a mechanism_census test that fails CI when a second generation appears. <!-- [^2e544-3] -->

## CI Parity Matrix

A CI parity matrix runs every journey acceptance test with backend ∈ {local, bunker} to prevent silent second-classness regression. <!-- [^2e544-4] -->

## Dormant-Surface Inventory

A dormant-surface inventory test fails CI on unregistered public enum variants or hooks with no production construction site. <!-- [^2e544-5] -->

## Cross-Module Contract Comments

Stale cross-module contract comments must cite a test or are review-rejectable. <!-- [^2e544-6] -->

## Multi-Instance Interop

Two NmpApp instances in one process pass an interop test with separate wallets, bunker sessions, and no crosstalk. <!-- [^2e544-7] -->

## Kotlin Flatc-Drift CI Gate

The Kotlin flatc-drift CI gate must be extended to cover `nmp/kernel/*.kt` hand-written bindings, not just `nmp/transport/*.kt`, to prevent silent binding divergence. The cross-platform typed-decoder parity gap (projections wired on iOS but silently null on Android) has no CI gate to catch it; issue #1288 was filed for extending the Kotlin drift gate to cover `nmp/kernel/*.kt` bindings.

<!-- citations: [^02745-4] [^02745-30] -->
## Generated Swift Trailing-Whitespace Drift

A Swift flatc-drift CI gate (`check-swift-flatc-drift.sh`) was added to detect stale bindings, mirroring the existing Rust/Kotlin/TS gates. The Swift codegen drift gate requires regenerating both `gen swift` and `gen typed-decoders` after Rust projection types change, via `cargo run -p nmp-core --features codegen-schema --bin dump_projection_schemas | cargo run -p nmp-codegen -- gen swift` and `cargo run -p nmp-codegen -- gen typed-decoders`. The Swift codegen drift gate caught that `RelayDiagnostics.generated.swift` was stale (missing the `RelayDiagnosticsInfo` table and info field from NIP-11 relay metadata after #1195), and a trailing-whitespace mismatch in `TimelineSnapshot.generated.swift`. The gate uses a pure byte-diff with zero fuzzing, consistent with the Rust/Kotlin/TS gates. Committed generated files are excluded from trailing-whitespace trimming: `.editorconfig` disables `trim_trailing_whitespace` and `insert_final_newline` for `ios/Chirp/Chirp/Bridge/Generated/**/*.generated.swift`, and `.gitattributes` marks that path as `-text linguist-generated=true`, so Git applies no EOL/whitespace munging and the gate stays a pure byte-diff. PR #1287 adds these entries, preventing the permanent re-drift caused by flatc's trailing space after the file-identifier accessor.

<!-- citations: [^02745-5] [^02745-31] [^02745-52] [^02745-81] [^02745-127] -->
## Fix-Agent Reversal Gate

When a fix agent reverses documented tested features rather than genuine bugs, those reversals must be split out and escalated as owner decisions instead of merged autonomously. <!-- [^02745-32] -->
