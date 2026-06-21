---
type: episode-card
date: 2026-05-25
session: 1231660f-79c1-4b38-9651-9111cc20afb0
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1231660f-79c1-4b38-9651-9111cc20afb0.jsonl
salience: product
status: active
subjects:
  - screenshot-display
  - device-mockup
supersedes: []
related_claims: []
source_lines:
  - 1419-1815
captured_at: 2026-06-18T05:27:43Z
---

# Episode: Screenshot display changed to CSS iPhone device mockup

## Prior State

Screenshots rendered in flat 16/10 aspect-ratio grid tiles with object-fit: cover; no device context; short/wide source images were heavily cropped

## Trigger

User request: "the screenshots should be inlined sizing to the device's aspect ratio properly -- ideally with a proper device bezel so it looks nice"

## Decision

Replaced flat grid tiles with a pure-CSS iPhone device mockup: 9:19.5 fixed aspect-ratio screen, #141414 bezel body, 44px border-radius, Dynamic Island pill, volume/power buttons via ::before/::after pseudo-elements, home indicator bar at bottom. Images use object-fit: cover with object-position: top to fill the phone frame while anchoring content at top

## Consequences

- Portrait phone frame gives all screenshots device context regardless of source image dimensions
- Tall screenshots (content-core, 1206×2622) nearly fill the screen area
- Short/wide screenshots (content-mention-chip, 1206×350) are side-cropped to fill the frame with content visible at top
- Placeholder tiles also render inside the device frame when screenshots fail to load
- Container changed from CSS grid to flexbox with flex-wrap for left-to-right flow

## Open Tail

- Landscape-oriented screenshots may lose significant horizontal content due to object-fit: cover cropping

## Evidence

- transcript lines 1419-1815

