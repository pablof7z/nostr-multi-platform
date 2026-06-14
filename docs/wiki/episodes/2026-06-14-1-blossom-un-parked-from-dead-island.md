---
type: episode-card
date: 2026-06-14
session: 2e5449b9-15e0-4d80-98a7-5281bda701d6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/2e5449b9-15e0-4d80-98a7-5281bda701d6.jsonl
salience: reversal
status: active
subjects:
  - nmp-blossom
  - parked-crate-policy
  - workspace-exclude
supersedes: []
related_claims: []
source_lines:
  - 7156-7244
  - 7432-7434
  - 7475-7682
  - 7706-7712
  - 7807-7861
  - 7867-7891
captured_at: 2026-06-14T20:37:50Z
---

# Episode: Blossom un-parked from dead-island to v1 workspace member

## Prior State

nmp-blossom was classified as a 'post-v1 dead island' and parked in [workspace].exclude; CI never built or tested it; external consumers (hl, podcast-player) needed fragile workarounds (/tmp [patch] redirects, vendored copies); the documented standalone-build command (`cargo build --manifest-path crates/nmp-blossom/Cargo.toml`) actually failed because the crate still declared `edition = { workspace = true }` while being excluded from that workspace

## Trigger

User correction at line 7432: 'blossom is not post-v1 — it's part of v1' — plus the discovery that the original parking rationale (crate has Unsupported stubs) was false for blossom, and that #1424's 'standalone-buildable' claim was itself broken

## Decision

Un-park nmp-blossom back into [workspace].members: remove from exclude list, restore workspace field inheritance, delete the standalone [workspace] table (the parking artifact that broke Cargo resolution), correct the exclude-block comment. Only nip60/wallet-poc remain parked (with empty [workspace] table for genuine standalone buildability). Released as nmp-v0.7.2.

## Consequences

- CI now builds/tests blossom — the blind spot where an excluded-but-consumed crate could silently rot (all gates green while the release was broken) is closed for blossom
- podcast-player dropped vendor/nmp-blossom directory and /tmp [patch] workarounds entirely; blossom resolves as a normal git dependency (PR #506 merged)
- hl auto-resolves blossom as a normal path-dep workspace member once its checkout advances past the 0.7.2 commit
- nmp-feedback merged at 0.7.2 (SHA 857dedf45)
- Durable root-cause finding: Cargo workspace auto-discovery walks up from excluded crates and binds them to the parent workspace, breaking field inheritance; the empty [workspace] table per parked crate stops this walk-up — the correct pattern for genuinely parked crates (nip60)
- CI coverage gap remains: other excluded-but-consumed crates could still silently rot (#1426 filed)

## Open Tail

- #1426 policy decision: should 'excluded-but-consumed' category exist at all, or must any crate with live external consumers be a workspace member?
- Proposed CI gate to compile excluded crates with known consumers on every release — awaiting owner prioritization
- hl main checkout must advance past 45ac8c3e4 to pick up the fix (owner-managed, not forced)

## Evidence

- transcript lines 7156-7244
- transcript lines 7432-7434
- transcript lines 7475-7682
- transcript lines 7706-7712
- transcript lines 7807-7861
- transcript lines 7867-7891

