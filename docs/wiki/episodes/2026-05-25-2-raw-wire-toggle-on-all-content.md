---
type: episode-card
date: 2026-05-25
session: 63dfcbb3-3ae0-48bb-9228-a494f85df203
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/63dfcbb3-3ae0-48bb-9228-a494f85df203.jsonl
salience: product
status: active
subjects:
  - nmp-gallery-raw-toggle
  - content-rendering
  - mention-display
supersedes:
  - 2026-05-25-1-eliminate-hardcoded-fake-display-names-in
related_claims: []
source_lines:
  - 1100-1897
captured_at: 2026-06-18T05:34:33Z
---

# Episode: Raw wire toggle on all content-parsing gallery pages

## Prior State

Content views only showed resolved display names (or short pubkey fallback). No mechanism existed for users or developers to see the raw wire-level URIs that the kernel received, making it hard to understand what data actually came off the wire versus what was resolved.

## Trigger

User request: "I want each one of the pages to have a raw toggle, which toggles whether the mention is rendered raw or not — WITHOUT LYING! — it should show the same 'hello nostr:npub1....' as raw when enabled and the actual kind:0 when enabled as 'hello @kind0name'"

## Decision

Added per-page rawMode toggle (Switch on Android, Toggle on iOS) to content-view, content-mention-chip, and content-minimal pages on both platforms. When ON, mentionLabel returns the full wire URI (uri.uri / uri.uri). When OFF, mentionLabel returns kernel-resolved @displayName. NostrMinimalContentView gained an optional mentionLabel closure threaded through walkMinimal so the minimal renderer also participates in the toggle.

## Consequences

- NostrMinimalContentView.swift API expanded with optional mentionLabel parameter
- iOS RawToggle struct added as reusable component across content pages
- Android ContentComponentPages.kt uses per-page remember { mutableStateOf(false) } for rawMode
- Demo mention tree added to content-mention-chip page so NostrContentView section responds to the toggle
- Raw mode shows exact wire data — no synthesis, no truncation

## Open Tail

*(none)*

## Evidence

- transcript lines 1100-1897

