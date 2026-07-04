---
type: episode-card
date: 2026-07-03
session: dcc80382-bcc0-45ea-8b9c-1a2fc741f872
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/dcc80382-bcc0-45ea-8b9c-1a2fc741f872.jsonl
salience: architecture
status: superseded
subjects:
  - read-model-collapse
  - concept-owned-active-reads
  - public-vocabulary
  - feed-scope-boundary
supersedes: []
related_claims: []
source_lines:
  - 1-236
  - 1054-1106
  - 1207-1209
  - 1559-1561
captured_at: 2026-07-03T09:43:37Z
---

# Episode: Read-model collapse: concept-owned active reads as sole public model

## Prior State

Multiple parallel public nouns existed as app-facing vocabulary: 'session' (FeedSession, typed session), 'observed delivery/projection', 'source reducer', 'Trellis', 'interest shape', and 'relation buckets'. A generic open_session(namespace, bytes) API had been explicitly rejected, but the concrete concept-owned seams (open_replies, open_reactions, open_reposts, open_zaps) were still missing for plain notes (#2758/#2758). Feed was overloaded: acting as profile resolver, reply-count engine, thread hydrator, and general content augmentation surface.

## Trigger

User's explicit 'ruthless collapse' directive: one public model (concept-owned active reads), everything else is private machinery. The directive included a vocabulary fate table, feed-specific ownership rules, a deletion/deprecation cut list, and acceptance criteria for a cleanup PR.

## Decision

Adopt concept-owned active reads as the only public read model. Public vocabulary is reduced to open_<concept>(spec) -> Handle, close(handle), load_more(handle), and typed output. 'Session' is deleted from public docs and renamed in code (FeedSessions→Feeds, FeedSessionHandle→FeedHandle, session_id→handle_id across native/uniffi/browser/web surfaces, enforced by feed_vocabulary doctrine-lint ratchet). Internal model collapsed into one read lifecycle engine with three stages (Demand → Admission+model → Output). Feed scope is narrowed: feed owns only primary item acquisition, source/perspective resolution, repost wrapper inclusion, windowing/order, and feed row output; it does NOT own profiles, reply counts, reactions, zaps, thread hydration, referenced-event previews, media loading, or app-specific social bars — those are mounted as separate concept reads. Parallel public lanes are deprecated or deleted (open_observed_feed_source, open_interest, ObservedProjection docs, ReducedSource docs, generic session namespace examples, relation buckets). Trellis is banned from all app-facing types, docs, and generated helpers.

## Consequences

- Vocabulary rename shipped as PR #2800 (merged): FeedSessions→Feeds across all surfaces with feed_vocabulary doctrine-lint ratchet guarding the ban
- open_observed_feed_source deprecated and deleted (#2770 closed)
- Feed rows must expose stable references to other concepts, not hydrate the entire screen — if a feed row needs reply count, mount open_replies; if it needs avatar, mount/open profile output
- Docs-first approach: rename docs before code; public docs, generated helpers, examples, templates, and native/web surfaces must not teach internal nouns
- Acceptance criterion (chirp#15) remains open: collapse is merged but not yet proven in a real client rendering live relay data

## Open Tail

- chirp#15 (Chirp removes fail-closed guard) is still OPEN — the collapse's own baked-in validation gate is unmet
- Chirp repo is on an in-flight feature branch with dirty tree; cross-repo re-pin + wiring not safe to launch without user confirming branch base
- DX question unresolved: should reads get a blessed generic binding in nmp-uniffi-support, or is per-app facade re-wiring the intended thin-shell proof?

## Evidence

- transcript lines 1-236
- transcript lines 1054-1106
- transcript lines 1207-1209
- transcript lines 1559-1561

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-03-1-read-model-collapse-concept-owned-active.json`](transcripts/2026-07-03-1-read-model-collapse-concept-owned-active.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-03-1-read-model-collapse-concept-owned-active.json`](transcripts/raw/2026-07-03-1-read-model-collapse-concept-owned-active.json)
