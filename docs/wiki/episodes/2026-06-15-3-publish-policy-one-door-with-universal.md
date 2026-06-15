---
type: episode-card
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
salience: architecture
status: superseded
subjects:
  - publish-policy-onedoor
  - private-fail-closed
  - gift-wrap-leak
supersedes:
  - 2026-06-15-2-privatefailclosed-privacy-gate-enforced-at-universal
related_claims: []
source_lines:
  - 2978-2984
  - 3103-3115
  - 3130-3157
  - 3173-3184
captured_at: 2026-06-15T15:03:56Z
---

# Episode: Publish policy one-door with universal dispatch-site enforcement

## Prior State

Scattered literal guards for private/gift-wrap events — raw.kind == KIND_GIFT_WRAP && Auto in actor/commands/publish.rs (missed kind:14 sealed DMs). Resume-from-store and manual-retry paths bypassed any gate. Reintroduction gate scanned only action.rs for exact kind==N substrings — near-vacuous.

## Trigger

Three codex review rounds: (1) PrivateFailClosed declared but not enforced — gift-wrap/sealed DM with Auto routing could leak to public relays (D10 violation); (2) resume_from_store and manual retry bypassed run_publish_engine_at gate, building EVENT frames directly; (3) refused private rows lingered pending in durable store, re-refusing on restart.

## Decision

policy.rs is the single door for kind→policy decisions. PrivateFailClosed enforced at the universal dispatch_due emit site (where initial + resume + retry + availability-redispatch all converge) by consulting persisted per-relay relay_reasons — only Explicit-rationled relays may emit private kinds. Refused rows terminal-finalized (no linger). Doctrine lint gate broadened to catch match/matches!/.contains evasion shapes across full routing surface.

## Consequences

- kind:14 sealed DMs now covered (previously missed)
- Resume and retry paths cannot leak private envelopes to public relays
- Old scattered literal guard deleted — no kind-literal guards outside policy.rs
- Entry-point guards serve as fail-fast defense-in-depth; dispatch_due is load-bearing
- Non-vacuity proven: disabling dispatch_due guard makes resume-leak test emit an EVENT frame

## Open Tail

- Separate raw_event_forwarder emits EVENT frames but default policy only forwards replaceable kinds, not 1059/14 — out of scope for publish gate

## Evidence

- transcript lines 2978-2984
- transcript lines 3103-3115
- transcript lines 3130-3157
- transcript lines 3173-3184
