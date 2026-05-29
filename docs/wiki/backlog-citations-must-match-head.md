---
title: "BACKLOG.md Violation Entries Must Cite File:Line Verified Against Current HEAD"
slug: backlog-citations-must-match-head
summary: "Every new backlog violation entry must have its file:line citations ground-truthed against the live HEAD tree before being written."
tags:
  - backlog
  - citations
  - audit
  - workflow
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# BACKLOG.md Violation Entries Must Cite File:Line Verified Against Current HEAD

> BACKLOG.md Section 1 maintains an invariant: every violation entry cites a specific `file:line` that exists in the current HEAD tree. Transcribing audit findings verbatim without re-verifying against HEAD produces stale or wrong citations that mislead future agents and waste triage time.

## Details

### The Invariant
Every violation entry in BACKLOG.md Section 1 must include a `file:line` reference that:
1. Exists in the current HEAD tree at the time of writing.
2. Points to the actual offending code, not a nearby line or a line that has since moved.

### Verification Workflow
Before adding a new entry:
```bash
# Confirm the file exists at HEAD
git show HEAD:<path/to/file> | head -n <line+5> | tail -n 10
# Or use grep to find the current line number
grep -n "<pattern>" <path/to/file>
```
Only after confirming the exact line should you write the citation.

### Why Verbatim Transcription Fails
- Audit tooling may have run against a stale checkout or a different branch.
- Refactors, file moves, and line insertions shift line numbers between audit time and backlog-write time.
- A wrong citation causes the next agent to waste time hunting for a violation that is no longer at the cited location.

### Stale Citation Consequences
- Future agents may mark violations as resolved when they cannot find the cited line, even if the violation still exists elsewhere.
- CI tooling that cross-references backlog citations will produce false negatives.


### Additional Rule

Never transcribe audit findings or GitHub issue descriptions verbatim into BACKLOG.md. In a 31-issue audit, 5 entries cited code that had already been fixed or referenced non-existent paths. The verification step is mandatory, not optional — confirm the symbol, function, or block still exists at the cited location in HEAD before writing the entry.

### Additional Rule

When bulk-importing GH issues as backlog entries (e.g., V-87–V-105), 5 of 31 issues cited code that no longer existed at HEAD — two were already fixed, three cited deleted files. Never transcribe issue text verbatim. Always `grep` the live tree first; correct citations that have moved and drop citations for code that no longer exists.
## See Also
- [[pd-decisions-can-be-stale-in-backlog|pd decisions can be stale in backlog]] — related guide
- [[pd-decisions-can-be-stale-in-backlog|pd decisions can be stale in backlog]] — related guide
- [[stale-wip-entries-common|stale wip entries common]] — related guide
- [[gallery-vs-production-app-distinction|Gallery App Implementations Do Not Satisfy Production Backlog Items]] — related guide
