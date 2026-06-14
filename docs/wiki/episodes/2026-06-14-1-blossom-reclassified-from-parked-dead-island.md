---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: reversal
status: superseded
subjects:
  - nmp-blossom
  - workspace-membership
  - parked-crates
supersedes: []
related_claims: []
source_lines:
  - 7156-7244
  - 7432-7517
  - 7638-7666
  - 7676-7710
captured_at: 2026-06-14T18:22:12Z
---

# Episode: Blossom reclassified from parked dead-island to v1 workspace member

## Prior State

nmp-blossom was classified as 'post-v1' and excluded from the workspace; #1324 parked it; #1424 claimed parked crates were 'standalone-buildable' but the claim was false — Cargo's workspace auto-discovery still walked up and bound the excluded crate to the workspace root that excludes it, causing 'failed to find a workspace root' for both path-dep consumers (hl) and the documented standalone build command

## Trigger

hl build failed against nmp-blossom path-dep, revealing the #1424 fix was insufficient; user explicitly corrected the classification: 'blossom is not post-v1 — it's part of v1', noting the parking was itself the bug since blossom has zero Unsupported stubs and is actively consumed by two external apps

## Decision

Un-park nmp-blossom from the workspace exclude list back into members; remove its standalone [workspace] table (a parked-crate artifact); restore workspace field inheritance (edition, version, license, repository); update description; release as nmp-v0.7.2. nmp-nip60 (the Cashu PoC, which does have Unsupported stubs) stays parked with the empty [workspace] table fix from #1427

## Consequences

- CI now builds and tests blossom as a first-class workspace member — the no-dead-code doctrine is satisfied because blossom is complete, not stubbed
- External consumers (hl, podcast-player) can depend on blossom as a normal git dependency, eliminating the fragile /tmp blossom-copy [patch] workaround
- nmp-v0.7.2 released (0.7.0 → keystone breaking; 0.7.1 → de-inherited parked crates; 0.7.2 → blossom un-parked)
- nmp-nip60 remains parked but genuinely standalone-buildable via its empty [workspace] table
- #1426 filed for the systemic CI gap: excluded-but-consumed crates are invisible to CI release gates

## Open Tail

- #1426 CI-gate question unresolved: should a crate with live external consumers ever be classified as a 'dead island'?
- nmp-nip60 still parked — will need similar un-parking when Cashu enters scope
- External PRs (nmp-feedback#1, podcast-player#501) need merge in dependency order, re-pinned to 0.7.2, with podcast's /tmp patch dropped

## Evidence

- transcript lines 7156-7244
- transcript lines 7432-7517
- transcript lines 7638-7666
- transcript lines 7676-7710

