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
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:019ec57a-fb01-7081-80c8-d7107f302049
  - session:418d555f-8e77-4e56-8166-93d1fef9cfce
  - session:ab8061fc-b277-4ba4-bf55-1532bcb1aa90
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
  - session:78b50727-bccd-4088-8493-a07624a4fa83
---

# CI Gates

## Test Plan

CI must include `cargo test -p nmp-app-template` and `cargo build --workspace --examples` in the test plan to prevent the example-compile gap class that let a `pub(crate)` visibility slip through twice. Local cargo test validation must be scoped to the crates touched and their obvious downstream consumers, not the whole workspace; `cargo test --workspace` is reserved for the merging agent and CI only, to avoid serializing the build queue and starving other worktrees. The doctrine lint smoke test (`cargo test -p nmp-testing --test doctrine_lint_smoke`) and a workspace compile check (`cargo build --workspace`) are always-on local gates that must be run alongside scoped tests. `cargo build --workspace` must be run as a compile-only check whenever a public symbol is renamed, a module is moved, a Cargo.toml dep path is changed, or a workspace member is added. A CI or doctrine-lint rule must forbid app crates from enabling `test-support` in runtime `[dependencies]`.

<!-- citations: [^da6b1-12] [^019ec-3] [^418d5-4] [^019ec-2] -->
## File-Size Gate

The file-size gate must always use `--from-ref origin/master --to-ref HEAD --baseline-ref origin/master` (never `--changed-only`) because `--changed-only` only checks modified files and passes locally while failing CI on files that expanded but weren't in the diff. The file-size CI flake was caused by duplicate path entries in `.file-size-baseline` (`web/registry/src/registry/content.ts` duplicated, and `web/chirp/src/nmp/runtime.test.ts` with divergent caps 512 and 533); removing the duplicates makes the gate deterministic. File-size ratchet violations must be resolved by extracting cohesive modules (not raising baselines). New test files must also be split into separate submodules when they would push a file past the 500-LOC hard cap. A pre-emptive split of test files hovering near the 500-LOC cap eliminates the recurring round-trip where every fix PR tips a file over and needs a split before merge. PR #1295 extracts `settings_view` and `diagnostics_panel` (361 lines) from `app.rs` into a new `settings.rs`, reducing `app.rs` from 2172 to 801 lines (the split was needed for the file-size gate, though the file remains above the 500-line baseline). The push-event file-size CI workflow has a known gap: force-pushed branches produce a red check because the orphaned before SHA is unavailable in the clone; this should be fixed to fall back to the merge-base or skip with a notice.

<!-- citations: [^da6b1-13] [^02745-2] [^02745-28] [^02745-50] [^78c8e-45] [^78c8e-462] [^ab806-262] [^ab806-276] -->
## Pre-existing CI Breakages

The `committed_registry_json_matches_generated_output` CI test currently fails on master (registry.json stale), causing `cargo test` red on multiple PRs that don't touch registry code; this is a pre-existing breakage. The AI architecture signoff CI check (#1267) is RED due to a bad `OPENAI_API_KEY` and is treated as infrastructure noise for merge decisions. (Previously: The 'AI architecture signoff' CI check was a known-broken gate due to a bad `OPENAI_API_KEY` (#1267) and must be ignored when gating PR merges.) The podcast-player TestFlight pipeline has never shipped because CI never built the Rust core for the simulator architecture, causing an undefined symbol `_nmp_free_string` link failure that gated off the deploy step for 40 consecutive runs; PR #429 fixes this by prepending the sim-arch Rust build. (Previously: The podcast-player TestFlight pipeline failed for 40 consecutive runs because CI never built the Rust core for the simulator architecture, causing an undefined-symbol link failure that gated off the deploy job.) The compound `&&` merge command pattern swallowed exit codes and merged PR #1165 with a failing check; merges must gate on explicit pass/nonpass counts, never chain push after piped check. When merging any PR, the full cargo test lane must complete green before merging — 'no failures yet' does not constitute a passed check. (Previously: 'no failures yet' did not constitute a passed check; lesson from #1302 breaking master by merging on incomplete CI.) Ten of eleven needs-decision issues are determined by existing documented direction; only #1281 (whether `since=None` interests should be exempted from the T129 watermark rewrite) genuinely requires owner input. Kernel PR #1436 (M2 profile-claim registry migration) passes all core tests (1541 nmp-core, 113 nmp-ffi, 60 doctrine) but has a web Playwright E2E test (feed.spec.ts:24) failing twice while green on master, preventing merge until diagnosed.

<!-- citations: [^da6b1-14] [^02745-3] [^02745-29] [^02745-51] [^da6b1-44] [^da6b1-65] [^02745-115] [^02745-126] [^019ec-4] [^ab806-73] -->
## Android CI Gate

An Android `--features marmot` build was broken by a missing vendored OpenSSL dependency (libsqlite3-sys bundled-sqlcipher-vendored-openssl), a missing `zeroize` std feature, and a missing import—all uncaught by CI because the featureless lane was green. A CI gate was added to prevent regression. The `NMP_MARMOT_MOCK_KEYRING=1` environment variable remains as an escape hatch for headless/CI/repl contexts, unconditionally installing the in-memory mock store.

Marmot group relays are persisted by MDK (replace_group_relays on create/accept) and recoverable via get_relays after restart, enabling re-subscription without network. <!-- [^78c8e-82] -->

<!-- citations: [^78c8e-19] [^78c8e-46] -->
## Mechanism Census

The mechanism_census test fails CI when a second generation of any tracked capability appears.

<!-- citations: [^2e544-3] [^2e544-342] -->
## CI Parity Matrix

A bunker parity matrix runs every journey acceptance test with backend ∈ {local, bunker} via the nak bunker harness, blocking merge on divergence.

<!-- citations: [^2e544-4] [^2e544-343] [^2e544-426] -->
## Dormant-Surface Inventory

A dormant-surface inventory test fails CI on unregistered public enum variants or hooks with no production construction site. <!-- [^2e544-5] -->

## Cross-Module Contract Comments

Stale cross-module contract comments must cite a test or are review-rejectable. <!-- [^2e544-6] -->

## Multi-Instance Interop

The two-instance interop test runs two NmpApp instances in one process with separate wallets, separate bunker sessions, and no crosstalk.

<!-- citations: [^2e544-7] [^2e544-427] -->
## Kotlin Flatc-Drift CI Gate

The Kotlin flatc-drift CI gate must be extended to cover `nmp/kernel/*.kt` hand-written bindings, not just `nmp/transport/*.kt`, to prevent silent binding divergence. The cross-platform typed-decoder parity gap (projections wired on iOS but silently null on Android) has no CI gate to catch it; issue #1288 was filed for extending the Kotlin drift gate to cover `nmp/kernel/*.kt` bindings.

<!-- citations: [^02745-4] [^02745-30] -->
## Generated Swift Trailing-Whitespace Drift

A Swift flatc-drift CI gate (`check-swift-flatc-drift.sh`) was added to detect stale bindings, mirroring the existing Rust/Kotlin/TS gates. The Swift codegen drift gate requires regenerating both `gen swift` and `gen typed-decoders` after Rust projection types change, via `cargo run -p nmp-core --features codegen-schema --bin dump_projection_schemas | cargo run -p nmp-codegen -- gen swift` and `cargo run -p nmp-codegen -- gen typed-decoders`. The Swift codegen drift gate caught that `RelayDiagnostics.generated.swift` was stale (missing the `RelayDiagnosticsInfo` table and info field from NIP-11 relay metadata after #1195), and a trailing-whitespace mismatch in `TimelineSnapshot.generated.swift`. The gate uses a pure byte-diff with zero fuzzing, consistent with the Rust/Kotlin/TS gates. Committed generated files are excluded from trailing-whitespace trimming: `.editorconfig` disables `trim_trailing_whitespace` and `insert_final_newline` for `ios/Chirp/Chirp/Bridge/Generated/**/*.generated.swift`, and `.gitattributes` marks that path as `-text linguist-generated=true`, so Git applies no EOL/whitespace munging and the gate stays a pure byte-diff. PR #1287 adds these entries, preventing the permanent re-drift caused by flatc's trailing space after the file-identifier accessor.

<!-- citations: [^02745-5] [^02745-31] [^02745-52] [^02745-81] [^02745-127] -->
## Fix-Agent Reversal Gate

When a fix agent reverses documented tested features rather than genuine bugs, those reversals must be split out and escalated as owner decisions instead of merged autonomously. <!-- [^02745-32] -->

## Release Versioning

NMP release version is 0.7.0 with tag nmp-v0.7.0 at SHA ce0097cde. The `nmp-blossom` crate must be listed in `release/nmp-release.toml` as a `[[public_crates]]` entry so the release-manifest CI check passes; nmp-blossom was added to the release manifest, fixing the long-standing release-manifest CI red. The parked crates nmp-blossom and nmp-nip60 were unbuildable when excluded from the workspace because they still inherited edition/version/license/repository via `workspace = true`, fixed in #1427 by adding an empty `[workspace]` table per parked crate, making each its own workspace root so they remain standalone-buildable as external path dependencies.

<!-- citations: [^2e544-344] [^2e544-345] [^2e544-386] [^2e544-428] [^ab806-185] [^ab806-224] -->
## Nightly CI Gates

The nightly CI gate for feed-idle (`ffi-stress feed-idle --fail-on-gate`) is wired into the existing s3-snapshot-pressure-gate workflow so future changes that break feed omission fail nightly CI. A runnable Rust NMP-consumer stress harness (not a permanent test fixture) validates all before/after scenarios with real keys, real Schnorr signatures, real store/GC paths, and a local fixture relay for echo/dedup testing.

<!-- citations: [^78c8e-463] [^78b50-196] [^78b50-213] -->
