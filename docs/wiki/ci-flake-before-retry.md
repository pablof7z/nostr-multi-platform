---
title: Inspect CI Failure Logs Before Assuming a Code Fix Is Needed — Transient Failures Exist
slug: ci-flake-before-retry
summary: CI checks can fail transiently (e.g., crates.io network errors); always read the logs before writing a code fix.
tags:
  - ci
  - flake
  - network
  - debugging
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:cd331450-f93f-48d0-960e-3c73e927775e
---

# Inspect CI Failure Logs Before Assuming a Code Fix Is Needed — Transient Failures Exist

> CI pipelines interact with external services (crates.io, npm registry, GitHub artifact storage) that can fail transiently. A red check does not always mean the code is wrong — it may mean a download timed out or a registry returned a 503. Always read the failure log before spending time on a code change.

## Details

### Common Transient Failure Signatures
- `error: failed to download from https://static.crates.io/...` — crates.io CDN blip.
- `Could not resolve host: registry.npmjs.org` — DNS/network issue in the runner.
- `Connection reset by peer` during artifact upload/download.
- Gradle daemon OOM killed mid-download.

### Diagnosis Workflow
1. Open the failing CI job log.
2. Search for `error`, `failed`, `timeout`, `connection`, `503`, `429`.
3. If the failure is in a network/download step and the error message references an external host, it is likely transient.
4. Re-run the job (GitHub Actions: "Re-run failed jobs") before writing any code.

### When to Write a Code Fix
Only after a re-run also fails with the same error, or after the error clearly points to a source file (compile error, test assertion failure, lint violation).

### Cost of Premature Code Fixes
- Introduces unnecessary churn in the diff.
- May mask the real transient failure, making it harder to diagnose recurring flakiness.
- Wastes review bandwidth on a no-op change.


### Additional Rule

## SSL / crates.io Timeout Pattern

A common transient failure pattern: SSL drops or crates.io fetch timeouts cause a CI check to fail on a PR that is otherwise clean. If the failure log shows network-level errors (curl SSL errors, registry timeouts) rather than compilation or test failures, re-run the check before writing any code fix. A passing second run with identical code confirms flaky infrastructure, not a code problem. Document the flake in the PR description so reviewers are not misled.

### Additional Rule

Confirmed pattern on PR #762: a doctrine lint CI check failed, but raw log inspection revealed a network transient error downloading a crate from crates.io — not a real lint violation. A retry run passed all 42 tests. Always read the raw CI logs for network/infrastructure errors before treating a CI failure as a code problem requiring investigation or a fix.

### Additional Rule

PR #762 doctrine-lint failure: a fix agent was dispatched before logs were read; the failure was a transient crates.io network error, not a real violation. Concrete checklist before dispatching a fix agent: (1) open the raw CI log, (2) look for network/disk/registry error strings, (3) only dispatch a code-fix agent if the failure is reproducible and traceable to a source change.
## See Also
- [[pr-review-land-loop-workflow|PR Review-and-Land Loop — Automated Merge Workflow]] — related guide
