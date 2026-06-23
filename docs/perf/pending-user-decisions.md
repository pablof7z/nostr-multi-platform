# Pending User Decisions

> **SUPERSEDED by GitHub Issues** (2026-06-08).
> Open decisions are tracked under `category:decision` label in GitHub Issues.
> This file is retired — do not add new entries here.
> All pre-2026-06-08 decision history has been pruned (planning scaffolding,
> resolved saga blocks, and stale autonomous-decision logs). The full audit
> trail lives in git history if needed.

## How to file a new decision

```
gh issue create \
  --title "Decision: <short description>" \
  --label "category:decision,priority:p<N>" \
  --body "..."
```

## Open decisions (as of 2026-06-23)

All open decisions are tracked in GitHub Issues with `category:decision` label.
Run `gh issue list --label category:decision --state open` to see current queue.
