---
type: episode-card
date: 2026-06-26
session: ccf39f42-1717-41d2-aa85-48f6d27e6298
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/ccf39f42-1717-41d2-aa85-48f6d27e6298.jsonl
salience: architecture
status: active
subjects:
  - nip29-publish-action
  - action-module-design
  - kind-routing
  - cache-consolidation
supersedes: []
related_claims: []
source_lines:
  - 103-110
  - 151-207
  - 282-294
  - 355-395
  - 415-435
  - 509-525
captured_at: 2026-06-26T08:50:42Z
---

# Episode: Replace kind-specific NIP-29 actions with generic kind-agnostic publish

## Prior State

nmp-nip29 exposed separate ActionModule implementations for each event kind (PostChatMessageAction for kind 9, ReactInGroupAction for kind 7, ShareEventInGroupAction for kind 11, RepostInGroupAction for kind 16). Each action manually built ['h', ...] envelope and owned kind-specific logic. Previous tags came from a separate RecentGroupEvents cache (dead code—only constructed in tests, never used in production). Actions had no capability to read from the kernel store.

## Trigger

User critique: 'nip29 shouldn't have anything chat specific -- chat is just one more event kind -- this is poor design.' Investigation confirmed the codebase already violates its own stated principle: group_event.rs and composed.rs explicitly document 'NIP-29 doesn't own these kinds, only the h-tag routing concern,' but PostChatMessageAction bakes in kind 9 + chat semantics. The previous-tag mechanism (RecentGroupEvents) is wired but dead.

## Decision

Replace multiple kind-specific actions with one generic 'nmp.nip29.publish' action namespace accepting {group, kind, content, tags}. nmp-nip29 injects ['h', local_id] and populates previous tags by querying the kernel store via StoreQuery::Tags{#h, limit:5} at publish time (not a separate cache). Requires adding a small, generic, D0-clean store-read capability to the ActionModule layer (a reusable trait, not NIP-29 specific). Delete PostChatMessageAction and migrate all consumers (chirp dispatch, chirp-tui, FFI tests, fixtures) to the generic namespace without compat aliases.

## Consequences

- Eliminates the false architectural claim that NIP-29 owns specific event kinds
- Collapses two caches into one: deletes RecentGroupEvents cache and its recorder observer wiring
- Makes the previous-tag anti-spam mechanism live (currently dead code)
- Requires threading a synchronous store-read handle through ActionModule::execute—but this seam is generic and reusable for any action needing to read events (reply actions fetching targets, etc.)
- Single source of truth for cached events: the kernel LMDB store
- Corrects a latent bug: previous tags now reflect actual cached history, not just events arriving after observer registration (ADR-0062 read-cache is respected)
- Net code removal: deletes cache + recorder infrastructure instead of adding it
- PostChatMessageAction namespace is removed; all dispatch sites migrate to generic namespace

## Open Tail

- Codex producing detailed seam design for ActionModule store-read capability trait and edit plan
- Implementation in worktree
- Scoped test runs + doctrine-lint verification
- PR landing to master

## Evidence

- transcript lines 103-110
- transcript lines 151-207
- transcript lines 282-294
- transcript lines 355-395
- transcript lines 415-435
- transcript lines 509-525

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-1-replace-kind-specific-nip-29-actions.json`](transcripts/2026-06-26-1-replace-kind-specific-nip-29-actions.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-1-replace-kind-specific-nip-29-actions.json`](transcripts/raw/2026-06-26-1-replace-kind-specific-nip-29-actions.json)
