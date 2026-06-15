---
type: episode-card
date: 2026-06-15
session: fabf8ca3-e1b9-4a7c-bcd5-bf5731fb571d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/fabf8ca3-e1b9-4a7c-bcd5-bf5731fb571d.jsonl
salience: root-cause
status: active
subjects:
  - squash-merge-landing-signal
  - branch-triage-policy
supersedes: []
related_claims: []
source_lines:
  - 1257-1283
captured_at: 2026-06-15T08:30:10Z
---

# Episode: Squash-merge makes landed branches appear unlanded — abort auto-landing of KEEP branches

## Prior State

Uncommitted work in an unmerged branch that differs from master was assumed to be abandoned valuable work that should be recovered, committed, and landed as a new PR.

## Trigger

PR #1433 (blocked-relay-publish fix) was opened from recovered 'abandoned' work, but rebase revealed master already contained the identical feature via PR #1274 (squash-merged with a different commit history). The original branch showed 'differs from master' despite the feature being fully shipped.

## Decision

Close #1433 as redundant. Switch from auto-landing KEEP-verdict branches to preserving them for owner judgment. The heuristic 'branch differs from master ≠ unlanded work' now governs branch recovery.

## Consequences

- 6 KEEP branches preserved for manual owner review instead of auto-landed (fix/adr0055-publish-ver-bump, test/profile-claim-lifecycle-invariants, main, two iOS worktree-agent branches, ios/fix-pr789-build-breaks)
- File-size doctrine split of builder.rs → mod.rs + app_host.rs was wasted work (part of the closed redundant PR)
- Future branch-triage must check feature-level landing (PR state, master file content) not just commit ancestry
- Finding recorded in project memory for future sessions

## Open Tail

- fix/adr0055-publish-ver-bump looks genuinely unlanded (self-contained publish_engine.rs fix) — owner may want it landed
- 258 auto-generated knowledge-capture wiki .md files archived but not committed — owner must decide whether to review or discard
- The 'no-net-change' classifier caught cleanly-landed branches but the '2f-differ' classifier was over-conservative for squash-merged branches where master evolved those files post-merge

## Evidence

- transcript lines 1257-1283
