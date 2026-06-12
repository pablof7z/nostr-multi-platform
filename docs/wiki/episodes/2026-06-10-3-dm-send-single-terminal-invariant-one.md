---
type: episode-card
date: 2026-06-10
session: 8db7983d-2852-4213-9b8c-43650a958e7a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/8db7983d-2852-4213-9b8c-43650a958e7a.jsonl
salience: product
status: active
subjects:
  - nmp-nip17
  - dm-send
  - publish-engine
  - action-lifecycle
supersedes: []
related_claims: []
source_lines:
  - 1138-1172
captured_at: 2026-06-11T23:11:53Z
---

# Episode: DM-send single-terminal invariant — one action, one verdict

## Prior State

NIP-17 DM send produced two terminal verdicts per action correlation_id: one for the recipient envelope and one for the self-copy gift-wrap. `action_lifecycle` used latest-stage-wins per correlation_id, producing nondeterministic observable outcome for the action spinner.

## Trigger

Audit confirmed double terminals are observable through the full stack: `record_terminal` → `pending_terminals` Vec → `take_action_results_projection` → `action_lifecycle.record()`. No downstream dedup makes this benign.

## Decision

Self-copy envelope now carries `envelope_correlation_id: None`; only the recipient envelope carries `Some(correlation_id)`. Self-copy gift-wrap failure still toasts (D6 visibility) but does NOT fire `RecordActionFailure`.

## Consequences

- One action now produces exactly one terminal verdict — deterministic spinner resolution
- Self-copy failure no longer contradicts a successful recipient delivery
- Uses the existing `ActorCommand::PublishSignedEvent.correlation_id: Option<String>` seam — no new engine machinery

## Open Tail

*(none)*

## Evidence

- transcript lines 1138-1172

