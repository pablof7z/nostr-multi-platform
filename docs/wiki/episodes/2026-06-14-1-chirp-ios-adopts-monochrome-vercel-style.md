---
type: episode-card
date: 2026-06-14
session: 286c6f24-af4b-4e59-b72f-ed72e8b9d781
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/286c6f24-af4b-4e59-b72f-ed72e8b9d781.jsonl
salience: reversal
status: active
subjects:
  - chirp-ios-monochrome-doctrine
  - chirp-theme
  - chirp-ui-components
supersedes: []
related_claims: []
source_lines:
  - 1-7
  - 2117-2160
  - 2291-2310
  - 2450-2464
  - 2525-2544
captured_at: 2026-06-14T22:17:11Z
---

# Episode: Chirp iOS adopts monochrome Vercel-style design doctrine

## Prior State

Chirp iOS used scattered blue/cyan/yellow/red chrome — colored pubkey-gradient avatar fallbacks, teal accent, candy-colored wallet tiles, capsule CTAs, blue-tinted thread cards, decorative gradient profile banner, raw nostr: URI loading text, always-visible compose ring, hardcoded color badges. Codex initial review scored 7/10 and called it 'amateur, childish, half-baked'.

## Trigger

User directive: app 'feels like a very amateur, childish, prototype where things barely work — very half baked'; must be minimalistic and monochrome anchored on a single black accent (Vercel style). Four iterative codex image-based reviews (7→8→8.5→9/10) drove specific refinements: colored avatar was 'the loudest wrong note', solid-black CTAs were 'too heavy', capsule shapes were 'childish', vertical proportions were 'inflated'.

## Decision

Adopt a single adaptive monochrome accent (black in light mode, white in dark) as the only chromatic element. Concrete changes folded into this doctrine: (1) new AccentColor asset + global-accent build setting in project.yml as source of truth; (2) grayscale avatar fallbacks replacing pubkey-color gradients; (3) CTA shapes from full capsule to rounded-rect (radius 12-14); (4) Edit Profile demoted from solid-black primary to secondary gray bordered chip; (5) Settings rebuilt as native inset-grouped Form with compact spacing; (6) profile banner flattened from decorative gradient to neutral surface (118→84pt); (7) Wallet candy tiles removed, hero icon resized, CTA width-constrained; (8) embedded-event loading state from raw URI text to skeleton placeholder; (9) compose progress ring hidden at zero characters; (10) thread focused-note card de-tinted; (11) OutboxRow raw 'kind N' label hidden; (12) Notifications empty-state green icon neutralized.

## Consequences

- Entire app is now visually monochrome in both light and dark modes; no scattered color chrome remains except actual user profile pictures
- AccentColor asset + global-accent xcodegen setting must be maintained as the single source of truth for the accent
- All NMP UI component consumers must respect the monochrome palette or be overridden at the Chirp theme layer
- Codex final verdict 9/10 'ship it — the monochrome direction is coherent'
- Feed/thread/DM styling mechanism-verified but not pixel-verified (sim account had no relay data)

## Open Tail

- Remaining codex nits (native iOS 26 tab-bar/Form vertical density) explicitly marked as 'not worth fighting' by reviewer
- project.yml must be committed alongside any pbxproj changes or xcodegen regen will silently drop the monochrome accent setting

## Evidence

- transcript lines 1-7
- transcript lines 2117-2160
- transcript lines 2291-2310
- transcript lines 2450-2464
- transcript lines 2525-2544
