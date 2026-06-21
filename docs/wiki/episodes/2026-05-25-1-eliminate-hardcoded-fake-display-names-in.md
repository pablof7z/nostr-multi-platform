---
type: episode-card
date: 2026-05-25
session: 63dfcbb3-3ae0-48bb-9228-a494f85df203
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/63dfcbb3-3ae0-48bb-9228-a494f85df203.jsonl
salience: product
status: superseded
subjects:
  - nmp-gallery-content-pages
  - mention-resolution
  - no-fake-data
supersedes:
  - 2026-05-25-2-profile-data-flows-via-open-author
related_claims: []
source_lines:
  - 928-980
captured_at: 2026-06-18T05:34:33Z
---

# Episode: Eliminate hardcoded fake display names in gallery content views

## Prior State

ContentComponentPages on both platforms used hardcoded fake display names ("jack", "satoshi"), fake pubkeys ("deadbeefcafebabe..."), and synthetic labels. Android ContentComponentPage did not receive GalleryModel, so it could not resolve profiles from the kernel at all — mentions showed raw hex pubkeys like @fa984bd7…8f52.

## Trigger

User observed Android showing raw pubkey instead of resolved name for content mentions, then saw "(synthetic)" labels with fake names, and explicitly said "NO FAKE DATA!"

## Decision

Thread GalleryModel through ContentComponentPage; use profileMap + claimProfile() for live kernel resolution. All display names now come from profileMap[pubkey]?.displayName, falling back to defaultMentionLabel (short pubkey format). iOS SampleContent.richTree uses real npub URI and DEMO_PUBKEY_HEX. Quote cards and mention chips all resolve from the kernel.

## Consequences

- ContentComponentPage now requires a model parameter — call sites updated
- Unresolved profiles show truncated pubkey (e.g. fa984bd7…8f52), never fabricated names
- All sample content wire data uses real npub URIs, not synthetic placeholders
- LaunchedEffect claims profiles on composition so kernel resolves them asynchronously

## Open Tail

*(none)*

## Evidence

- transcript lines 928-980

