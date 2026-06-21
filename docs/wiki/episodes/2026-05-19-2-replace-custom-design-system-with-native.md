---
type: episode-card
date: 2026-05-19
session: 5d893073-9635-450b-b8e9-50648bc1a4e7
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/5d893073-9635-450b-b8e9-50648bc1a4e7.jsonl
salience: reversal
status: active
subjects:
  - chirp-design-system
  - chirp-theme
supersedes: []
related_claims: []
source_lines:
  - 2660-2725
  - 3389-3557
captured_at: 2026-06-18T04:20:28Z
---

# Episode: Replace custom design system with native iOS controls and semantic colors

## Prior State

Chirp used a custom design system with hardcoded colors (Color.purple, Color.blue, Color.black, Color.indigo), custom GlassCard wrapper, ChirpPrimaryButton, capsule badges, gradient backgrounds, decorative orbs, .scrollContentBackground(.hidden), and .listRowBackground(Color.clear) — making the app look non-native.

## Trigger

User said: 'the whole app looks weird instead of looking like a normal iOS app — I don't want any but the typical native controls with semantic names, no hardcoded colors or styles, none of that shit'

## Decision

Replaced the entire custom visual layer with native iOS idioms: Form/List with Section headers, standard Toggle/Button, semantic colors (Color.accentColor, Color(.systemBackground), Color(.secondarySystemBackground)), plain VStack instead of GlassCard. ChirpColor and ChirpFont enums redefined to map to system equivalents. Removed .scrollContentBackground(.hidden), .listRowBackground(Color.clear), gradient backgrounds, and decorative orbs across all views.

## Consequences

- All Swift views now use native Form/List/Section instead of custom card layouts
- ChirpColor enum retained as indirection but maps to semantic system colors only
- No hardcoded Color.purple/.blue/.black/.indigo remaining in view files
- RelaySettingsView (missed in initial rewrite) had to be fully redone in a second pass
- NoteContentView hardcoded colors (Color.black.opacity(0.72), Color.blue, Color.indigo) replaced with semantic equivalents in second pass
- Git history overwrite required cherry-pick recovery of 7 lost commits onto the new native-UI base

## Open Tail

- ChirpColor still contains semantic-color aliases (positive=green, zap=orange, like=red) — may need further review for semantic accuracy

## Evidence

- transcript lines 2660-2725
- transcript lines 3389-3557

