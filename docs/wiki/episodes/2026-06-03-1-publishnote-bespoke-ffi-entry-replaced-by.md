---
type: episode-card
date: 2026-06-03
session: 7f143c67-6e46-424a-90a8-5bf844947fee
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/7f143c67-6e46-424a-90a8-5bf844947fee.jsonl
salience: architecture
status: active
subjects:
  - publish-action
  - ffi-surface
  - one-door-per-capability
supersedes: []
related_claims: []
source_lines:
  - 30-45
  - 775-784
captured_at: 2026-06-11T22:58:15Z
---

# Episode: PublishNote bespoke FFI entry replaced by PublishRaw namespace

## Prior State

Three bespoke FFI symbols (`nmp_app_publish_unsigned_event`, `nmp_app_publish_signed_event`, `nmp_app_publish_signed_event_to`) and a kind:1-specific `PublishNote` action variant existed as separate publish doors.

## Trigger

PR #916 deleted `PublishNote` and the bespoke FFI surface, consolidating all publish paths under `nmp_app_dispatch_action` → `PublishRaw{kind, tags, content, target}` per the one-door-per-capability rule.

## Decision

All callers must use `PublishRaw` with an explicit `kind` field. Kind:0 and kind:3 are gated to their dedicated variants; `PublishRaw` blocks them. The publish lifecycle (retry/cancel) remains a separate narrow FFI seam.

## Consequences

- Android, iOS, and Rust callers all migrated to emit `{"PublishRaw": {kind:1, tags, content, target:"Auto"}}`
- The `PublishNote` JSON envelope is wire-incompatible post-#916; any client still emitting it will break at dispatch
- Parity tests now deserialize into `PublishAction::PublishRaw` to prove wire compatibility
- chirp-repl was deleted rather than migrated (unused app)

## Open Tail

- iOS and Android still emit minimal NIP-10 reply markers — full root forwarding blocked on projection change (#920)

## Evidence

- transcript lines 30-45
- transcript lines 775-784

