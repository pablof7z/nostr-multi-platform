---
type: episode-card
date: 2026-05-21
session: 1c093fa5-0f0e-4dee-bf38-99781e763f13
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1c093fa5-0f0e-4dee-bf38-99781e763f13.jsonl
salience: product
status: active
subjects:
  - action-stages
  - terminal-verdict-lifecycle
  - record-action-failure
supersedes: []
related_claims: []
source_lines:
  - 3714-3756
  - 3897-3903
captured_at: 2026-06-18T04:41:52Z
---

# Episode: Terminal verdicts must survive cap eviction; action_stages contract hardened

## Prior State

Existing test asserted that terminals COULD be evicted when the history cap was reached. Executor send-then-panic was an orphan — no failure was recorded. iOS SwiftUI body-rendering ordering caused ACK races.

## Trigger

Codex review of PR #230 found: H2 (existing test proved wrong eviction behavior), M4 (executor panic left no audit trail), H1 (iOS render race), M3 (D12 single-line heuristic was trivially bypassable).

## Decision

Contract inverted: terminal verdicts must survive at cap, only non-terminals get evicted. New ActorCommand::RecordActionFailure variant lets the actor record a failure even when the executor panics (catch_unwind → RecordActionFailure). D12 lint extended to walk multi-line function bodies with brace-depth tracking. DispatchQueue.main.async acknowledged as partial H1 fix — proper view-driven ACK protocol deferred.

## Consequences

- Old eviction test inverted; new tests for terminal survival and degenerate all-terminal histories
- RecordActionFailure adds envelope shape {correlation_id, error} so host can ACK the resulting Failed terminal
- D12 lint now handles multi-line function bodies — single-line bypass closed
- H1 (iOS render race) deferred: DispatchQueue.main.async is not a SwiftUI render barrier, needs design conversation
- M4 codex follow-up: iOS KernelBridge.swift:570 parses correlation_id first and ignores error field — sync-Err vs recorded-Failed are indistinguishable to host; envelope contract disambiguation still needed (PR-G3 queued but not shipped)

## Open Tail

- H1 iOS render race needs view-driven ACK protocol design
- M4 envelope contract disambiguation — sync-Err and panic-recorded-Failed look identical to iOS host
- M3 D12 string-literal robustness deferred until AST-level scanning

## Evidence

- transcript lines 3714-3756
- transcript lines 3897-3903

