---
title: Pending Decision Entries in BACKLOG.md May Already Be Resolved — Verify Before Surfacing
slug: pd-decisions-can-be-stale-in-backlog
summary: PD entries in BACKLOG.md can be stale if the decision was made and executed without formally closing the entry.
tags:
  - backlog
  - pending-decision
  - discovery
  - workflow
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
---

# Pending Decision Entries in BACKLOG.md May Already Be Resolved — Verify Before Surfacing

> BACKLOG.md Pending Decision (PD) entries record open architectural or implementation questions. However, decisions are often made and executed in the codebase without the corresponding backlog entry being formally closed. Surfacing a stale PD as an open question wastes stakeholder time and can cause duplicate or conflicting work.

## Details


PD-033-A is the canonical example of a PD closed on paper but spiritually open: marked CLOSED BY DELETION (2026-05-28) because apps/notes/ and apps/longform/ were deleted, but its closure explicitly says 'revisit when V-37 lands.' The podcast-player incident IS that revisit — the framework thesis remains unproven until V-37's affordances land, and the podcast-player as the second app is the live test. A PD entry with a conditional closure (revisit-when) must be treated as open when that condition triggers. [^d0690-57]
### Why PD Entries Go Stale
- A developer makes the decision and implements it but forgets to update BACKLOG.md.
- A PR that resolves the decision does not reference the PD entry in its description.
- Automated backlog tooling only adds entries, never closes them.

### Verification Workflow
Before surfacing a PD entry as an open question:
1. Read the PD description to understand what decision was pending.
2. Search the codebase for evidence of a resolution:
   ```bash
   grep -r "<keyword from PD>" --include="*.rs" --include="*.kt" --include="*.swift"
   git log --oneline --all --grep="<keyword>"
   ```
3. Check merged PRs for the relevant time window:
   ```bash
   gh pr list --state merged --search "<keyword>"
   ```
4. Only if no resolution is found should the PD be treated as genuinely open.

### Closing a Stale PD
When you confirm a PD has been resolved:
- Update the BACKLOG.md entry to mark it closed, citing the commit or PR that resolved it.
- Verify the cited file:line per the `backlog-citations-must-match-head` guide.


### Additional Rule

Concrete example: PD-041 (Marmot/MLS and NWC+zaps scope) was listed as a pending user decision with no owner, but the user immediately clarified it had already been decided and largely executed in the codebase. Before surfacing any PD entry to the user, search for related implementation in the code. A PD entry with substantial related implementation is very likely already resolved and the BACKLOG entry is simply stale.

### Additional Rule

PD-041 (Marmot/MLS and NWC+zaps v1 scope) was listed as a pending user decision with no owner; the user immediately clarified it was already decided and largely executed. Before presenting any PD entry to the user, grep the codebase for the described feature work. If substantial implementation already exists, the decision was made — update or close the entry rather than re-asking the user.
## See Also
- [[backlog-citations-must-match-head|backlog citations must match head]] — related guide
- [[backlog-citations-must-match-head|backlog citations must match head]] — related guide
- [[stale-wip-entries-common|stale wip entries common]] — related guide
