---
title: Never Merge on Pending cargo test — Cross-Crate Suite Is Mandatory
slug: never-merge-on-pending-cargo-test
summary: Never merge a PR while cargo test shows pending; the full nmp-testing/nmp-core/nmp-app-template cross-crate suite must be green before merging any PR that touches nmp-core or its dependents.
tags:
  - ci
  - testing
  - merging
  - cargo-test
  - release
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# Never Merge on Pending cargo test — Cross-Crate Suite Is Mandatory

> Never merge a PR while cargo test shows pending; the full nmp-testing/nmp-core/nmp-app-template cross-crate suite must be green before merging any PR that touches nmp-core or its dependents.

## The Rule

Never merge a PR while its `cargo test` CI check shows **pending** (running). The check must show **success** (green) before merging. This is a hard rule with no exceptions for "clean" or "trivial" changes. [^42908-39]

## Why: Per-Crate Test Scope is Insufficient

Agent fix PRs frequently run only `cargo test -p <affected-crate>`. This misses cross-crate contract tests in `nmp-testing`, `nmp-core`, and `nmp-app-template`. Changes that appear correct in isolation can break these integration tests in ways that only the full CI `cargo test` job detects.

Concrete incident: merging PRs #768–#773 on "14 pass, 1 pending" caused `c13_view_payload_uses_placeholders_then_refines_in_place` in `nmp-testing` to fail on the master tip. The fix (PR #779) required opening a timeline view before ingesting events to match the D5 snapshot-key gating introduced by V-46. [^42908-40]

## Verification Sequence Before Tagging a Release

After merging the last release-blocking PR:
1. `git fetch origin master`
2. `SHA=$(git rev-parse origin/master)`
3. `gh api repos/pablof7z/nostr-multi-platform/commits/$SHA/check-runs -q '.check_runs[]|select(.name=="cargo test")|(.conclusion//.status)'`
4. Only proceed to tag if the conclusion is `success` (not `in_progress`, not `pending`). [^42908-41]

## Cross-Crate Test Mandate for nmp-core Changes

Any PR touching `nmp-core`, `nmp-store`, or `nmp-planner` must run the full cross-crate suite before being considered mergeable:
```bash
cargo test -p nmp-testing   # framework contract tests (370+ tests)
cargo test -p nmp-core      # 192+ tests
cargo test -p nmp-app-template  # composition-root integration tests
cargo test -p nmp-testing --test doctrine_lint_smoke  # 42 doctrine rules
``` [^42908-42]

## See Also
- [[loop-command|/loop — Recurring and Self-Paced Prompt Scheduling]] — related guide
- [[pr-review-land-loop-workflow|PR Review-and-Land Loop — Automated Merge Workflow]] — related guide
- [[red-ci-merges-to-master|Red CI Merges to Master — Pattern and Prevention]] — related guide

