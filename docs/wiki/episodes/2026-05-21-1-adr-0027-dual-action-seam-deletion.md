---
type: episode-card
date: 2026-05-21
session: 47203d35-d7c9-4c12-bc47-a40773d7acc2
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/47203d35-d7c9-4c12-bc47-a40773d7acc2.jsonl
salience: architecture
status: active
subjects:
  - adr-0027
  - action-registry
  - dual-seam-closure-path
  - register-action-executor
supersedes:
  - 2026-05-21-1-actionmodule-trait-gains-typed-execute-path
related_claims: []
source_lines:
  - 2636-2660
  - 2719-2731
  - 2743-2750
  - 2849-2870
  - 2957-2984
  - 3098-3135
captured_at: 2026-06-18T04:57:01Z
---

# Episode: ADR-0027 dual-action-seam deletion — single typed registration path

## Prior State

Action registration required two calls: register_action_module + register_executor (the 'dual seam'). This was a documented foot-gun: forgetting half the pair caused silent failures. ADR-0027 proposed migrating to a single typed ActionModule::execute() path, but previous implementation attempts (PR #246) had failed due to merge conflicts across 7 commits.

## Trigger

During this session, the agent discovered that the ActionModule trait migration (type Action + fn execute()) had already been landed on master by another PR — only the backward-compat closure path (ClosureModule, register_executor, nmp_app_register_action_executor) remained structurally orphaned.

## Decision

Delete the entire dual-seam backward-compat path in one atomic PR: remove ClosureModule adapter, register_executor Rust method, nmp_app_register_action_executor FFI symbol, the duplicate register_executor("nmp.publish", ...) block in default_registry(), and migrate ~10 test usages from closures to typed ActionModule structs. Action registration is now a single call: app.register_action::<M>().

## Consequences

- PR #251 merged: -382 net LOC (+408 / -790), 7 typed test ActionModule structs added
- default_registry() collapsed to 5 lines (just registry.register::<PublishModule>())
- Namespace mismatch between validator and executor is structurally impossible — both live under the same M::NAMESPACE
- PR #252 followed: deleted pre-ADR-0027 free-function executors
- The nmp_app_register_action_module C-ABI symbol was also deleted; only doc-historical references remain
- Prior failed PR #246 approach (staged 7-commit rebase) abandoned — the reframing to 'just delete the orphaned path' made it one-session-safe

## Open Tail

- 5 nmp_app_chirp_register_* FFI entry points use two idempotency patterns (handle-returning vs swap-slot) — identified but not dispatched, app-specific, refactor cost > benefit
- 4-mutex NmpApp structural concern (Explorer Finding #5) still stands but judged marginal for 'impossible to fuckup' value
- Observation-channel consolidation flagged as HIGH risk, deferred

## Evidence

- transcript lines 2636-2660
- transcript lines 2719-2731
- transcript lines 2743-2750
- transcript lines 2849-2870
- transcript lines 2957-2984
- transcript lines 3098-3135

