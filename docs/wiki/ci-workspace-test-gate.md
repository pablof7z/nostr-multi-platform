---
title: CI Workspace Test Gate
slug: ci-workspace-test-gate
summary: CI must run `cargo test --workspace` on every PR via GitHub Actions to catch regressions.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:12b3f443-3c2d-4e47-976a-7f4ceab75343
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:9fc44c34-8e49-4959-91b3-714d4722ac3d
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:d98be997-81df-4738-8846-8323d40ab9ff
  - session:3a906f87-ee2b-4d3a-9d5f-e82ccab29349
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# CI Workspace Test Gate

## CI Workspace Test Gate

CI must run `cargo test --workspace` on every PR via GitHub Actions to catch regressions; the cargo test CI check must be green before merging to prevent breaking master. GitHub CI `pull_request` events merge the PR branch with current master to create a test merge commit; if a PR has merge conflicts, GitHub never creates this ref and `pull_request` CI events do not fire. Due to GitHub CI throttling, only approximately 3 of 10 workflows may run on rapid PR merges; when full CI cannot be triggered, PRs are merged based on local test verification. Failing PRs must be fixed immediately — always check CI on open PRs and launch a fix agent right away for any failing checks. Never run `cargo test --workspace` for agents — always scope to specific crates with `cargo test -p <crate>` to prevent stalls and machine crashes. New crates must be added to the release manifest (release/nmp-release.toml) or CI will hard-fail; this has caught two PRs (V-42 and V-57). A snapshot serialization CI regression gate with a threshold for make_update_us/serialize_us instrumentation is an untracked v1 exit item. Zero-test crates `nmp-threading` (710 LoC), `nmp-nip59` (146 LoC), and `nmp-signer-iface` (239 LoC) must have focused unit tests added. The `box_dyn_transport_round_trips_rpc` test is misnamed because it uses `Arc::new()` + `&dyn`, not `Box<dyn>` at `transport.rs:57-58`. The CI script ci/check-ffi-header-drift.sh references only headers that still exist in the repository tree. All workspace member crates listed in Cargo.toml have their source directories committed to the repository.

<!-- citations: [^57528-4] [^12b3f-3] [^1c093-4] [^1670f-2] [^9fc44-4] [^45258-4] [^d98be-3] [^3a906-2] [^4edd4-5] -->
## See Also

