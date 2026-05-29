---
title: PR Review-and-Land Loop — Automated Merge Workflow
slug: pr-review-land-loop-workflow
summary: "The automated PR review-and-land workflow: sync master, assess all open PRs, spawn rebase agents for conflicts, wait for cargo test to go green, merge clean PRs, prune worktrees, and continue monitoring for new PRs."
tags:
  - loop
  - pr
  - merge
  - ci
  - worktree
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:ae88711c-a987-4b41-939e-32c8ee0ab4d3
---

# PR Review-and-Land Loop — Automated Merge Workflow

> The automated PR review-and-land workflow: sync master, assess all open PRs, spawn rebase agents for conflicts, wait for cargo test to go green, merge clean PRs, prune worktrees, and continue monitoring for new PRs.

## Overview

The PR review-and-land workflow is an automated loop (typically driven by /loop with a fixed interval like 15m) that continuously monitors open PRs, resolves merge conflicts via worktree-isolated rebase agents, waits for CI (specifically cargo test) to go fully green, merges clean PRs, prunes accumulated worktrees, and continues monitoring for new PRs. The workflow never stops when the queue is clear — it keeps checking on the cron cadence in case new PRs appear. [^ae887-19]

## Master Sync

Every iteration begins by syncing local master to origin/master. This ensures the local view of PR mergeability is accurate and that any rebase operations use the latest master tip. When master has advanced (e.g. from concurrent workflows landing commits), the loop re-assesses all PR merge statuses against the new tip. [^ae887-20]

## CI Gate — cargo test is mandatory

Never merge a PR while cargo test shows pending or running. The full cross-crate test suite (nmp-testing, nmp-core, nmp-app-template) must show success before merging. When cargo test is the only remaining check, the loop waits with short-interval ScheduleWakeup checks (3–4 minutes) rather than the full cron cadence. If cargo test takes unusually long (10+ minutes), the loop continues waiting — there is no timeout-based skip.

During sub-interval pacing while waiting for cargo test, the loop uses ScheduleWakeup delays calibrated to the expected remaining time: 3–4 minutes when cargo test is already running and nearing completion (~7–10 min total runtime), 5 minutes when rebase agents are still pushing, and 8 minutes when CI has just been re-triggered on fresh commits and the full suite must run. When cargo test is the last remaining check and has been running 8–9 minutes, a 3-minute check is appropriate — it should complete any moment.

After a rebase push, `gh pr checks` may still show stale (pre-rebase) check results until GitHub refreshes. The loop must re-check CI status after a short wait rather than assuming a rebase introduced failures. This is especially important when a check shows FAILING on a PR where failure is anomalous for the change type (e.g., cargo test failing on a docs-only PR). Check whether the failure is on a fresh commit or a stale result by verifying the commit SHA associated with the check run.

When CI check status is ambiguous (e.g., `mergeStateStatus` changes to UNKNOWN), it may indicate a check just completed and GitHub is recalculating status. Use `gh run view` to check the raw run status on GitHub Actions directly rather than relying solely on `gh pr checks` or `mergeStateStatus`. Multiple check runs may be visible for the same PR (from earlier pushes); identify the run associated with the latest commit SHA.

When a check shows FAILING on a PR where failure is anomalous (e.g., cargo test failing on a docs-only PR that touches no Rust code), investigate before concluding it's a real failure. Check whether the failure is on a fresh post-rebase commit or a stale pre-rebase result. Use `gh run list --commit` to verify the commit SHA the check run executed against. If the failure is stale, re-running the check on the current commit typically resolves it.

<!-- citations: [^ae887-21] [^ae887-39] [^ae887-46] [^ae887-47] [^ae887-50] -->
## Merge Conflict Resolution — Rebase Agents

When a PR has merge conflicts with current master, the loop spawns a worktree-isolated rebase agent. The agent rebases the PR branch onto master, resolves conflicts, and force-pushes. For multiple conflicting PRs that touch different file areas, rebase agents run in parallel. After a rebase push, CI re-triggers on the fresh commit and the loop waits for new check results. Rebase agents are monitored via task-notifications and fallback ScheduleWakeup checks. Once all rebase agents complete and CI is green, the loop proceeds to merge. [^ae887-22]


When spawning parallel rebase agents, first assess which files each PR touches to ensure they target different areas — agents rebasing PRs that touch the same file risk producing conflicting resolutions. PRs touching different subsystems (docs, iOS components, gallery, wallet) can safely rebase in parallel. After all rebase agents push, CI re-triggers on the fresh commits and the loop waits for all checks to complete before merging. [^ae887-38]

While rebase agents are running, the loop arms a Monitor to receive task-notifications when agents complete, and sets a ScheduleWakeup fallback (typically 5 minutes) in case notifications don't fire. When a task-notification arrives (e.g. agent completing a rebase), the loop processes the result immediately and resets the fallback timer. The Monitor catches agent completions; the fallback catches agents that are still working or whose notifications were missed. [^ae887-42]
## Merge Order

Merge PRs in parallel when they touch different file areas and all checks are green. When PRs touch overlapping files, merge sequentially and re-check CI status on remaining PRs after each merge — a prior merge may introduce new conflicts. Merge the PR with the codegen drift fix first when one exists, as it unblocks other PRs that fail the same check. [^ae887-23]


When multiple PRs all fail the same CI check (e.g. "Swift codegen drift"), identify the PR that fixes the root cause and land it first. The codegen drift fix unblocks all other PRs that fail the same check — once merged, the remaining PRs only need to rebase onto the new master to clear the failure. In one case, PR #793 registered a missing `claimed_profiles` projection to fix a 32-field vs 31-entry mismatch in SNAPSHOT_PROJECTIONS; PRs #789–#792 all failed codegen drift until #793 landed. [^ae887-34]

After merging a PR, re-check CI status on remaining PRs before merging them — a prior merge may have introduced new conflicts even if the remaining PR was previously green. In one case, PR #789 was 16/16 green but after #791 and #792 landed, it showed a merge conflict with #792's KernelModel.swift changes and required a rebase agent. [^ae887-35]
## Duplicate PR Detection

Before merging, verify the PR's content is not already present in master (e.g. via a different commit path). A PR whose changes are already byte-identical in master should be closed as a duplicate rather than merged — attempting to merge it produces unnecessary conflicts or empty merges. [^ae887-24]


Detect duplicates by checking whether the PR's commit content already exists in master under a different commit SHA. This can happen when a PR's changes were landed through a different path (e.g. a concurrent workflow or an earlier merge of a related PR). When detected, close the PR with a note citing the existing master commit SHA rather than attempting to merge — merging a duplicate produces unnecessary conflicts or an empty merge commit. [^ae887-43]
## Post-Merge Cleanup

After merging PRs, immediately prune the associated worktrees and delete the remote branches. Each Rust worktree consumes 2–5 GB of disk; accumulated unpruned worktrees cause disk exhaustion. Before force-removing a locked worktree, verify the locking PID is dead — a live agent's worktree must not be removed. Also delete the merged remote branches to keep the remote ref namespace clean.

After a squash-merge, `git branch -d` on the local tracking branch may fail because the branch is checked out in a worktree. This is expected and does not indicate a merge failure — verify the PR shows as merged on GitHub rather than relying on local branch deletion success. The remote branch deletion and worktree removal handle cleanup independently. When the merge is confirmed on GitHub but the local branch is locked in a worktree, proceed with remote branch deletion (`git push origin --delete`) and skip the local branch deletion.

<!-- citations: [^ae887-25] [^ae887-48] [^ae887-51] -->
## Disk Pressure Handling

When disk is critically low (e.g. <200 MB free), merge operations can fail. Check disk usage with df -h before merging. If space is tight, identify safe cleanup candidates: stale worktrees whose locking PIDs are dead, cargo build caches, and system temp files. Worktrees locked by live PIDs (especially long-running orchestrator processes) must not be touched — find space elsewhere. [^ae887-26]


When disk is critically low (e.g. 163 MB free on a full disk), the loop pauses all merge operations to free space before continuing. The recovery sequence: (1) check df -h to confirm the severity, (2) run git worktree list to inventory worktrees and their lock PIDs, (3) for each locked worktree, read the lock file and verify whether the PID is alive via kill -0, (4) identify safe removal candidates — worktrees whose locking PIDs are dead, or whose branches are already merged and the PID is gone, (5) free space from safe caches and temp files, (6) only resume merging once there is sufficient headroom. Worktrees locked by live long-running processes (e.g. a 4.5-hour-old claude orchestrator process holding locks on multiple worktrees simultaneously) must never be touched — find space elsewhere. In one incident, a single PID 43960 held locks on all large worktrees, forcing the loop to recover space from caches rather than worktree pruning. [^ae887-36]
## Continuous Monitoring

The workflow does not stop when the queue is empty. It continues checking on the full cron cadence because concurrent workflows (e.g. long-running orchestrator processes) may open new PRs at any time. Each iteration re-syncs master and re-assesses the full PR list. The final status report when the queue is clear includes a session recap of everything merged.

When the queue clears and all open PRs are landed, the loop produces a session recap listing every PR merged (with number, description, and status) plus any PRs closed as duplicates (with the commit SHA already in master). The recap is followed by a final check-in scheduled on the full cron cadence to detect new PRs that may be opened by concurrent workflows.

Each empty-queue status check includes: syncing local master to origin/master, confirming no open PRs exist, reporting the current master commit SHA, checking disk free space, and noting any active concurrent workflow PIDs that may open new PRs. When disk free space is declining between iterations (e.g. dropping from 1.1 GB to 482 MB), the loop notes the trend and continues monitoring without taking action unless it drops below the critical threshold.

When a new PR appears between iterations (e.g., PR #794 appearing after a series of empty-queue checks), the loop detects it during the standard master-sync-and-assess cycle and proceeds with the normal merge workflow. A PR that is fully green with all checks passing and no conflicts can be merged immediately without spawning rebase agents. After merging and cleaning up the remote branch, the loop returns to empty-queue monitoring on the full cron cadence.

The loop also monitors disk free space across empty-queue iterations, noting when space has recovered (e.g., from 482 MB back to 4.7 GB) and confirming the active workflow PIDs are still alive without touching their worktrees.

<!-- citations: [^ae887-27] [^ae887-37] [^ae887-45] [^ae887-52] -->

CI Gate — cargo test is mandatory

During sub-interval pacing while waiting for cargo test, the loop uses ScheduleWakeup delays calibrated to the expected remaining time: 3–4 minutes when cargo test is already running and nearing completion (~7–10 min total runtime), 5 minutes when rebase agents are still pushing, and 8 minutes when CI has just been re-triggered on fresh commits and the full suite must run. When cargo test is the last remaining check and has been running 8–9 minutes, a 3-minute check is appropriate — it should complete any moment. [^ae887-55]

When cargo test is taking longer than normal (10+ minutes), the loop uses the waiting time productively: checking for orphaned worktree-agent branches, inspecting what the blocking PR's changes actually are, and verifying CI status via multiple methods (gh pr checks, gh run view, GitHub Actions directly). The loop does not sit idle between ScheduleWakeup calls — each iteration re-assesses the full state and looks for parallel work that can be done while waiting. [^ae887-56]

Continuous Monitoring

When the queue clears and all open PRs are landed, the loop produces a session recap listing every PR merged (with number, description, and status marker ✅) plus any PRs closed as duplicates (with the commit SHA already in master). The recap includes the final master commit SHA and confirmation that local master is in sync with origin. The recap is followed by a final check-in scheduled on the full cron cadence to detect new PRs that may be opened by concurrent workflows. Example format: "Session recap — everything merged to master: ✅ #793 — codegen drift fix, ✅ #790 — docs builder-guide, ✅ #791 — nmp-gallery dead pull symbol removal, ✅ #792 — iOS zap amount picker fix, ✅ #789 — iOS nmp-component-adoption, ✅ #784 — closed as duplicate (already in master as a9647dab). Master is at 64ecc91d, local in sync with origin. Queue is clear." [^ae887-57]

Each empty-queue status check includes: syncing local master to origin/master, confirming no open PRs exist, reporting the current master commit SHA, checking disk free space via df -h, and noting any active concurrent workflow PIDs that may open new PRs. When disk free space is declining between iterations (e.g. dropping from 1.1 GB to 482 MB), the loop notes the trend and continues monitoring without taking action unless it drops below the critical threshold. When disk space has recovered (e.g. from 482 MB back to 4.7 GB), the loop confirms the recovery and continues. The standard empty-queue response format is a concise status block: "Queue clear. Master at <sha>, <disk> free, PID <n> still active." [^ae887-58]

When a new PR appears between empty-queue iterations (e.g., PR #794 appearing after a series of empty-queue checks), the loop detects it during the standard master-sync-and-assess cycle and proceeds with the normal merge workflow. A PR that is fully green with all checks passing and no conflicts can be merged immediately without spawning rebase agents. After merging and cleaning up the remote branch via git push origin --delete, the loop returns to empty-queue monitoring on the full cron cadence. [^ae887-60]

When cargo test is taking longer than normal (10+ minutes), the loop uses the waiting time productively: inspecting the blocking PR's actual diff to understand what's changing, checking for orphaned worktree-agent branches that could be pruned, and verifying CI status via multiple methods (gh pr checks, gh run view, GitHub Actions directly). The loop does not sit idle between ScheduleWakeup calls — each iteration re-assesses the full state and looks for parallel work that can be done while waiting. For example, while waiting for PR #793's cargo test, the loop inspected the PR diff and discovered the root cause: a 32-field KernelTypes.generated.swift versus a 31-entry SNAPSHOT_PROJECTIONS mismatch. This understanding informed the merge strategy — once #793 landed, the remaining PRs just needed rebase. [^ae887-62]

Merge Order

When multiple PRs fail the "Swift codegen drift" CI check, diagnose the root cause by inspecting the PR diffs before spawning rebase agents. Codegen drift failures fall into two categories: (1) a projection registry mismatch — the Rust SNAPSHOT_PROJECTIONS array has a different count than KernelTypes.generated.swift fields, typically because a new projection (like claimed_profiles) was registered in Rust but the Swift side wasn't updated. The fix is to merge the PR that registers the missing projection first, then rebase the remaining PRs. (2) A PR that directly commits KernelTypes.generated.swift — the drift check fails because the committed version doesn't match what the current Rust registry generates. The fix is a rebase agent that regenerates the file from current master, not a manual edit. Identifying the category determines whether to merge-first-then-rebase or rebase-first. [^ae887-63]

When CI check status is ambiguous (e.g., mergeStateStatus changes to UNKNOWN), it may indicate a check just completed and GitHub is recalculating status. Use gh run view to check the raw run status on GitHub Actions directly rather than relying solely on gh pr checks or mergeStateStatus. Multiple check runs may be visible for the same PR from earlier pushes — identify the run associated with the latest commit SHA using gh run list --commit. When cargo test is still genuinely running (confirmed on GitHub Actions) after 10+ minutes, continue waiting — there is no timeout-based skip. [^ae887-64]

When a PR directly commits `KernelTypes.generated.swift` (as opposed to having it generated from the Rust registry), the Swift codegen drift check fails because the committed file version doesn't match what the current Rust registry generates. This is a distinct failure mode from the projection-count mismatch. The fix requires a rebase agent that regenerates the file from current master, not a manual edit of the committed file. In PR #795, the agent went further: it discovered that `claimed_events` was a live Rust projection missing from the codegen registry, registered it (moving from 32 to 33 entries), moved `ClaimedEventDto` to `EmbedHost.swift`, and regenerated — performing a substantive registry fix rather than a pure rebase. [^ae887-65]

When CI check status is ambiguous (e.g., mergeStateStatus changes to UNKNOWN), it may indicate a check just completed and GitHub is recalculating status. Use `gh run view` to check the raw run status on GitHub Actions directly rather than relying solely on `gh pr checks` or mergeStateStatus. Multiple check runs may be visible for the same PR from earlier pushes — identify the run associated with the latest commit SHA using `gh run list --commit`. When cargo test is still genuinely running (confirmed on GitHub Actions) after 10+ minutes, continue waiting — there is no timeout-based skip. [^ae887-66]

After a rebase push, `gh pr checks` may still show stale (pre-rebase) check results until GitHub refreshes. The loop must re-check CI status after a short wait rather than assuming a rebase introduced failures. This is especially important when a check shows FAILING on a PR where failure is anomalous for the change type (e.g., cargo test failing on a docs-only PR). Check whether the failure is on a fresh commit or a stale result by verifying the commit SHA associated with the check run via `gh run list --commit`. [^ae887-67]

When a check shows FAILING on a PR where failure is anomalous (e.g., cargo test failing on a docs-only PR that touches no Rust code), investigate before concluding it's a real failure. Check whether the failure is on a fresh post-rebase commit or a stale pre-rebase result. If the failure is stale, re-running the check on the current commit typically resolves it. [^ae887-68]

When multiple PRs fail the "Swift codegen drift" CI check, diagnose the root cause by inspecting the PR diffs before spawning rebase agents. Codegen drift failures fall into two categories: (1) a projection registry mismatch — the Rust SNAPSHOT_PROJECTIONS array has a different count than KernelTypes.generated.swift fields, typically because a new projection was registered in Rust but the Swift side wasn't updated. The fix is to merge the PR that registers the missing projection first, then rebase the remaining PRs. (2) A PR that directly commits KernelTypes.generated.swift — the drift check fails because the committed version doesn't match what the current Rust registry generates. The fix is a rebase agent that regenerates the file from current master, not a manual edit. Identifying the category determines whether to merge-first-then-rebase or rebase-first. [^ae887-69]

Merge Conflict Resolution — Rebase Agents

When a PR directly commits `KernelTypes.generated.swift` (as opposed to having it generated from the Rust registry), the Swift codegen drift check fails because the committed file version doesn't match what the current Rust registry generates. This is a distinct failure mode from the projection-count mismatch. The fix requires a rebase agent that regenerates the file from current master, not a manual edit of the committed file. In PR #795, the agent went further: it discovered that `claimed_events` was a live Rust projection missing from the codegen registry, registered it (moving from 32 to 33 entries), moved `ClaimedEventDto` to `EmbedHost.swift`, and regenerated — performing a substantive registry fix rather than a pure rebase. [^ae887-70]

When cargo test is taking longer than normal (10+ minutes), the loop uses the waiting time productively: inspecting the blocking PR's actual diff to understand what's changing, checking for orphaned worktree-agent branches that could be pruned, and verifying CI status via multiple methods (gh pr checks, gh run view, GitHub Actions directly). The loop does not sit idle between ScheduleWakeup calls — each iteration re-assesses the full state and looks for parallel work that can be done while waiting. For example, while waiting for PR #793's cargo test, the loop inspected the PR diff and discovered the root cause: a 32-field KernelTypes.generated.swift versus a 31-entry SNAPSHOT_PROJECTIONS mismatch. This understanding informed the merge strategy — once #793 landed, the remaining PRs just needed rebase. [^ae887-71]

When the queue clears and all open PRs are landed, the loop produces a session recap listing every PR merged (with number, description, and status marker ✅) plus any PRs closed as duplicates (with the commit SHA already in master). The recap includes the final master commit SHA and confirmation that local master is in sync with origin. The recap is followed by a final check-in scheduled on the full cron cadence to detect new PRs that may be opened by concurrent workflows. Example format: "Session recap — everything merged to master: ✅ #793 — codegen drift fix, ✅ #790 — docs builder-guide, ✅ #791 — nmp-gallery dead pull symbol removal, ✅ #792 — iOS zap amount picker fix, ✅ #789 — iOS nmp-component-adoption, ✅ #784 — closed as duplicate (already in master as a9647dab). Master is at 64ecc91d, local in sync with origin. Queue is clear." [^ae887-72]

Each empty-queue status check includes: syncing local master to origin/master, confirming no open PRs exist, reporting the current master commit SHA, checking disk free space via df -h, and noting any active concurrent workflow PIDs that may open new PRs. When disk free space is declining between iterations (e.g. dropping from 1.1 GB to 482 MB), the loop notes the trend and continues monitoring without taking action unless it drops below the critical threshold. When disk space has recovered (e.g. from 482 MB back to 4.7 GB), the loop confirms the recovery and continues. The standard empty-queue response format is a concise status block: "Queue clear. Master at <sha>, <disk> free, PID <n> still active." [^ae887-73]

When a new PR appears between empty-queue iterations (e.g., PR #794 appearing after a series of empty-queue checks), the loop detects it during the standard master-sync-and-assess cycle and proceeds with the normal merge workflow. A PR that is fully green with all checks passing and no conflicts can be merged immediately without spawning rebase agents. After merging and cleaning up the remote branch via git push origin --delete, the loop returns to empty-queue monitoring on the full cron cadence. [^ae887-74]

Post-Merge Cleanup

After a squash-merge, `git branch -d` on the local tracking branch may fail because the branch is checked out in a worktree. This is expected and does not indicate a merge failure — verify the PR shows as merged on GitHub rather than relying on local branch deletion success. The remote branch deletion and worktree removal handle cleanup independently. When the merge is confirmed on GitHub but the local branch is locked in a worktree, proceed with remote branch deletion (`git push origin --delete`) and skip the local branch deletion. [^ae887-78]

When a rebase agent is spawned to fix a codegen drift failure on a PR that directly commits `KernelTypes.generated.swift`, the agent may discover that the drift check fails because the committed file is missing a live Rust projection that exists in the registry. In this case, the agent must go beyond a pure rebase: it registers the missing projection in `SNAPSHOT_PROJECTIONS`, adds the corresponding DTO types, moves related code to the correct files, and regenerates. This is a "substantive fix" vs. a pure rebase or a manual edit of the committed file. In PR #795, the agent discovered that `claimed_events` was a live Rust projection missing from the codegen registry, registered it (moving from 32 to 33 entries), moved `ClaimedEventDto` to `EmbedHost.swift`, and regenerated — performing a substantive registry fix rather than a pure rebase. [^ae887-79]

When a rebase agent encounters a conflict in `project.pbxproj` (the XcodeGen-generated project file), the correct resolution is to take the union of both sides' file additions. Since `project.pbxproj` is generated from `project.yml` and both sides add disjoint sets of source files, merging both additions produces a valid result. This was exercised in PR #789's rebase, where only `project.pbxproj` conflicted and the agent resolved by unioning both sides' additions. [^ae887-80]

The session recap evolves across iterations, accumulating completed work. When the queue clears mid-session (e.g. after landing a batch of PRs), the loop publishes a "Progress this session" summary listing completed merges and any remaining work, then continues monitoring. When the queue finally clears with no remaining PRs, the loop publishes a "Full session recap" listing every PR merged with ✅ markers and any PRs closed as duplicates with their master commit SHA. The recap also includes the final master commit SHA and confirmation that local master is in sync with origin. The format uses a table when the list is long: | Status | PR | Description |. Completed merges use ✅, closed duplicates use 🗙. [^ae887-81]

When waiting for cargo test, the loop uses the time productively by inspecting the blocking PR's actual diff to understand the root cause of the delay. This diagnostic work often produces insights that inform the merge strategy: in one case, inspecting PR #793's diff while waiting for cargo test revealed a 32-field `KernelTypes.generated.swift` vs. 31-entry `SNAPSHOT_PROJECTIONS` mismatch, which explained why PRs #789–#792 all failed codegen drift — they'd all be unblocked once #793 landed and they rebased. The loop also checks for orphaned worktree-agent branches, verifies CI status via multiple methods (`gh pr checks`, `gh run view`, GitHub Actions directly), and looks for parallel work that can be done while waiting. [^ae887-83]

When multiple cargo test runs are visible for the same PR (from earlier pushes), use `gh run list --commit <SHA>` to identify the run associated with the latest commit. Earlier completed runs may show as "completed early" while the current run is still in progress. Only the run for the latest commit SHA is authoritative for merge decisions. [^ae887-84]

When `mergeStateStatus` transitions to `UNKNOWN`, it may indicate a check just completed and GitHub is recalculating overall status. This is a transient state, not a failure. Use `gh run view` to check the raw run status on GitHub Actions directly, and `gh run list --commit` to identify which runs are associated with the latest commit. Multiple check runs may be visible for the same PR from earlier pushes — identify the run associated with the latest commit SHA. [^ae887-85]

When merging multiple green PRs that touch different file areas but share a dependency chain, a prior merge can introduce new conflicts for a remaining PR even if it was previously 16/16 green. In one case, PR #789 was fully green but after #791 and #792 landed, it showed a merge conflict with #792's `KernelModel.swift` changes and required a rebase agent. Always re-check CI and mergeability status on remaining PRs after each merge in a sequential chain — a green status from before the merge is stale if master advanced with conflicting changes. [^ae887-86]

After a rebase push, `gh pr checks` may continue showing stale (pre-rebase) check results until GitHub re-evaluates. The loop must wait briefly and re-check rather than assuming the rebase introduced failures. This is especially important when a check shows FAILING on a PR where the change type makes failure anomalous (e.g., cargo test failing on a docs-only PR — check whether the failure is on the fresh commit or a stale pre-rebase result by verifying the commit SHA associated with the check run). [^ae887-87]

After spawning rebase agents, check whether branch tips have changed to confirm agents have pushed. If branch tips are unchanged, the agents are still working (or haven't pushed yet). In this case, arm a Monitor to receive task-notifications when agents complete and set a ScheduleWakeup fallback. When a task-notification arrives (e.g., "PR #790 rebase done — zero conflicts, CI re-running"), process the result immediately and reset the fallback timer. The fallback catches agents still working or whose notifications were missed. [^ae887-88]

When the queue is empty for multiple consecutive iterations, each iteration still performs the full check: sync master, scan for open PRs, report disk space, and confirm active workflow PIDs. Disk free space is tracked across iterations — declining trends (e.g., 1.1 GB → 482 MB) are noted but no action is taken unless the critical threshold is crossed. Recovery trends (e.g., 482 MB → 4.7 GB) are also noted. The loop continues indefinitely on the cron cadence, ready to detect and merge new PRs whenever they appear. [^ae887-89]

When the queue finally clears with all PRs landed, the loop publishes a "Full session recap" as a markdown table with columns | Status | PR | Description |. Completed merges use ✅, closed duplicates use 🗙. The table is followed by the final master commit SHA and confirmation that local master is in sync with origin. [^ae887-91]

When a PR has a known fixable CI failure (e.g., codegen drift) and cargo test is still running, spawn the rebase agent immediately rather than waiting for cargo test to complete. The rebase agent works in parallel with the running cargo test, and when both finish the loop can proceed directly to merge assessment. This avoids serializing the agent wait behind the cargo test wait. Once the agent pushes and cargo test completes, the loop re-assesses the full CI status on the fresh commit. [^ae887-92]

When a rebase agent encounters a conflict in `project.pbxproj` (the XcodeGen-generated project file), the correct resolution is to take the union of both sides' file additions. Since `project.pbxproj` is generated from `project.yml` and both sides add disjoint sets of source files, merging both additions produces a valid result without manual intervention. [^ae887-93]

Duplicate PR Detection

A variant of duplicate detection is when a PR appears in the open PR list but was already merged by a concurrent workflow (or on a previous loop iteration). The merge operation succeeds silently on GitHub but the PR still appears in `gh pr list` briefly. When the loop attempts to merge such a PR, GitHub returns that it's already merged. The loop treats this the same as a duplicate — skip it, sync master, and continue. In one case, PR #790 appeared in the list but was already merged; the loop detected this, noted "Already merged", and proceeded to sync and scan for remaining PRs. [^ae887-95]

Master Sync

When a local uncommitted change (dirty working tree) blocks a fast-forward merge to sync master, the loop must inspect the change to determine whether it's already in the incoming commits. If `git diff` shows zero lines of difference against the target commit, the change is already in master and the stash can be safely dropped. After dropping the stash, the fast-forward proceeds normally. This can happen when a PR that touches the same file as the local working tree was merged on a previous iteration. In one case, a local uncommitted change in `apps/chirp/chirp-desktop/src/app.rs` blocked fast-forward; stashing and verifying 0 lines diff confirmed the change was already in master, and the stash was dropped. [^ae887-96]

When a PR has a known fixable CI failure (e.g., codegen drift) while cargo test is still running, spawn the rebase agent immediately rather than waiting for cargo test to complete. The rebase agent works in parallel with the running cargo test: the agent rebases/fixes the code while cargo test continues, and when both finish the loop can proceed directly to merge assessment. This avoids serializing the agent wait behind the cargo test wait. Once the agent pushes and cargo test completes, the loop re-assesses the full CI status on the fresh commit. In one case, PR #795 had codegen drift failing and cargo test still running (13/15 so far); the loop spawned the rebase agent immediately and scheduled an 8-minute check covering both agent completion and fresh CI results. [^ae887-97]

Rebase agents sometimes go beyond pure rebasing and perform substantive fixes. When a PR directly commits a generated file like `KernelTypes.generated.swift`, the rebase agent may discover that a live Rust projection is missing from the codegen registry. The agent must then register the missing projection, add corresponding DTO types, move related code to the correct files, and regenerate — a substantive registry fix rather than a pure rebase or manual edit. The loop distinguishes between rebase (conflict resolution only) and substantive fix (registering missing projections, adding types) when reporting agent results. In PR #795, the agent found that `claimed_events` was a live Rust projection missing from the codegen registry, registered it (32→33 entries), moved `ClaimedEventDto` to `EmbedHost.swift`, and regenerated. The loop reported this as a "proper fix — not just a rebase." [^ae887-98]

The session recap evolves across iterations. When the queue clears mid-session after a batch of merges but work remains (e.g., a rebase agent still running), the loop publishes a "Progress this session" summary listing completed merges and the remaining work item with a ⏳ marker. This mid-session recap keeps the user informed of progress while work continues. When the queue finally clears with all PRs landed, the loop publishes a "Full session recap" as a markdown table with columns | Status | PR | Description |. Completed merges use ✅ and closed duplicates use 🗙. The table is followed by the final master commit SHA and confirmation that local master is in sync with origin. Example mid-session recap format: listing each merged PR with ✅ and the remaining PR with ⏳, followed by a ScheduleWakeup for the next check. Example final recap: including the full table plus "Master is at <sha>, local in sync with origin. Queue is clear." [^ae887-99]

When a local uncommitted change (dirty working tree) blocks a fast-forward merge during master sync, inspect whether the change is already in the incoming commits. Run `git stash` to clear the working tree, then `git diff stash@{0}..<target-commit> -- <file>` to check whether the stashed content is already in the target. If the diff shows zero lines of difference, the change is already in master and the stash can be safely dropped with `git stash drop`. After dropping, the fast-forward proceeds normally. This can occur when a PR that touches the same file was merged on a previous iteration, leaving the local working tree with content identical to the incoming commit. [^ae887-100]

While waiting for cargo test on a blocking PR, the loop inspects the PR's actual diff to understand what it changes. This diagnostic work produces insights that inform the merge strategy and may reveal root causes of cascading CI failures across multiple PRs. In one case, inspecting PR #793's diff while waiting revealed a 32-field `KernelTypes.generated.swift` vs a 31-entry `SNAPSHOT_PROJECTIONS` mismatch — the root cause of codegen drift failures on PRs #789–#792. This informed the strategy: once #793 landed, the remaining PRs only needed to rebase onto the new master to clear the drift check. [^ae887-102]

A variant of duplicate detection occurs when a PR still appears in `gh pr list` but was already merged by the loop on a previous iteration or by a concurrent workflow. When the loop attempts to merge such a PR, GitHub returns that it's already merged (the merge operation is idempotent). The loop treats this identically to a duplicate: skip the PR, sync master to incorporate the merged commit, and continue assessing remaining PRs. After syncing master, re-scan for any additional open PRs before returning to monitoring. [^ae887-103]
## See Also
- [[loop-command|/loop — Recurring and Self-Paced Prompt Scheduling]] — related guide
- [[never-merge-on-pending-cargo-test|Never Merge on Pending cargo test — Cross-Crate Suite Is Mandatory]] — related guide
- [[disk-pressure-kills-agent-fleet|Accumulated Worktrees Cause Disk Exhaustion — Prune After Every Merge]] — related guide
- [[worktree-live-agent-pid-check|Worktree Removal — Check Live Agent PIDs Before Force-Removing Locks]] — related guide
- [[agent-push-to-master-violation|Sub-Agents in Worktrees Must Push to Branch and Open a PR — Never Push to Master]] — related guide
- [[ci-flake-before-retry|Inspect CI Failure Logs Before Assuming a Code Fix Is Needed — Transient Failures Exist]] — related guide
- [[loop-command|/loop — Recurring and Self-Paced Prompt Scheduling]] — related guide
- [[chirp-ios-kernel-types-generated|Chirp iOS KernelTypes.generated.swift — Dev-Time Generation, Lives in Git]] — related guide
- [[loop-command|/loop — Recurring and Self-Paced Prompt Scheduling]] — related guide
- [[loop-command|/loop — Recurring and Self-Paced Prompt Scheduling]] — related guide
- [[chirp-ios-kernel-types-generated|Chirp iOS KernelTypes.generated.swift — Dev-Time Generation, Lives in Git]] — related guide

