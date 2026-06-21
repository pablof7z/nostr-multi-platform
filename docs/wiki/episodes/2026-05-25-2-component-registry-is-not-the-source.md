---
type: episode-card
date: 2026-05-25
session: e7a1d168-3c58-4438-a544-aa645850c388
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/e7a1d168-3c58-4438-a544-aa645850c388.jsonl
salience: architecture
status: active
subjects:
  - registry-adoption
  - ios-chirp
  - android-main-app
  - source-of-truth
supersedes: []
related_claims: []
source_lines:
  - 920-965
  - 1070-1098
captured_at: 2026-06-18T05:40:09Z
---

# Episode: Component registry is not the source of truth for consuming apps

## Prior State

The nmp-cli registry was designed as the canonical, installable source of Nostr UI components. Apps were expected to consume components via `nmp add component` and stay aligned with canonicals.

## Trigger

Audit revealed (1) iOS Chirp copied registry components into its tree and they have since diverged — `diff` confirms NostrContentRenderer.swift and NostrContentView.swift differ from registry canonicals; (2) the main Android app has zero registry imports, replacing everything with ~500 LOC of custom monolithic code (NostrRichText.kt, MediaViews.kt, inline Avatar/author headers in TimelineScreen.kt); (3) iOS uses no user-profile registry components at all (custom ChirpAvatar, inline Text(authorDisplayLabel), inline author header in NoteRowView).

## Decision

Adopt a phased reconciliation strategy: P1 re-align or delete-and-reinstall iOS content components; P2 adopt user-profile registry components on iOS (replacing ChirpAvatar, inline name, inline card); P3 adopt content-core + content-view + user-avatar + user-card on Android (eliminating ~500 LOC of unmaintained inline code).

## Consequences

- iOS content components must be diff-audited and reconciled before any new feature work on them
- Android main app must transition from monolithic custom renderers to registry imports
- The gallery app is the only Android codebase proving registry works — its adoption pattern becomes the template
- NoteRowView and NoteActionsRow on iOS are candidates for new registry components (content-actions, note-row)

## Open Tail

- Whether to enforce registry canonicals via CI integration tests (nmp-cli already has them for install-ids and file-mappings)
- Whether copy-and-drift pattern should be formally prohibited by policy or tooling

## Evidence

- transcript lines 920-965
- transcript lines 1070-1098

