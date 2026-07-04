---
type: episode-card
date: 2026-07-04
session: d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform--claude-worktrees-fix-2962-flaky-auto-arm/d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5.jsonl
salience: architecture
status: active
subjects:
  - nip-17-wss-gate
  - issue-2970
  - dm-relay-parser
supersedes: []
related_claims: []
source_lines:
  - 387-401
captured_at: 2026-07-04T12:25:02Z
---

# Episode: NIP-17 wss-only parser gate is a security invariant — rejected test-convenience relaxation

## Prior State

Issue #2970 flagged that NIP-17's wss-only parser gate in parse_dm_relay_list blocks local testing via nak serve, suggesting a cfg(test) or cargo-feature relaxation to allow ws:// relays.

## Trigger

Opus agent evaluated the relaxation on its merits and found it (a) buys nothing for Rust tests since DmRelayCache::upsert doesn't gate scheme — tests can seed directly, and (b) a cargo feature enabled for validation would validate the wrong confidentiality posture, forking the NIP-17 §2 invariant as a footgun.

## Decision

Do NOT relax the wss-only parser gate. The constraint is a NIP-17 §2 security invariant. Correct closures are non-invasive: an in-workspace integration test that injects ws:// via DmRelayCache directly (bypassing the parser), or a cert-trusted wss local-relay recipe (mkcert). Deferred post-v1.

## Consequences

- NIP-17 wss-only gate is established as a non-negotiable invariant not subject to test ergonomics
- Test strategies must bypass the parser layer directly rather than weakening the parser
- Any future relaxation would be a forked confidentiality posture, not a test convenience

## Open Tail

- No cert-trusted wss local-relay recipe documented yet; deferred to next NIP-17 validation pass

## Evidence

- transcript lines 387-401

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-04-2-nip-17-wss-only-parser-gate.json`](transcripts/2026-07-04-2-nip-17-wss-only-parser-gate.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-04-2-nip-17-wss-only-parser-gate.json`](transcripts/raw/2026-07-04-2-nip-17-wss-only-parser-gate.json)
