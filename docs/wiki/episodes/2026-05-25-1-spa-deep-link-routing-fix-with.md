---
type: episode-card
date: 2026-05-25
session: 3de5a430-eb71-466a-a3d0-eb58e2b42276
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/3de5a430-eb71-466a-a3d0-eb58e2b42276.jsonl
salience: product
status: active
subjects:
  - spa-routing
  - vercel-rewrite
supersedes:
  - 2026-05-25-2-vercel-catch-all-rewrite-blocked-all
related_claims: []
source_lines:
  - 1-89
captured_at: 2026-06-18T05:36:58Z
---

# Episode: SPA deep-link routing fix with assets exclusion

## Prior State

Deep links to subpages (e.g. /components/user-npub) returned 404 on Vercel; client-side navigation between pages failed. A prior blanket catch-all rewrite had been removed (commit 910f8d43) because it blocked all static file serving including /screenshots/*.png and /registry.json.

## Trigger

User reported that clicking navigation links did not change the page, and navigating directly to any subpage URL returned 404.

## Decision

Added a catch-all rewrite in vercel.json that excludes the /assets/ path: {"source": "/((?!assets/).*)" , "destination": "/index.html"}. This lets SolidJS's client-side router handle all non-asset routes while preserving Vercel's static file serving for hashed bundles and other assets.

## Consequences

- Direct URL navigation to any SPA route now loads the app correctly.
- Static assets under /assets/ are served directly, avoiding the previous regression where a blanket rewrite broke file access.
- Other static paths (e.g. /screenshots/, /registry.json) fall through to the SPA — may need additional exclusions if they are not meant to be client-side routes.

## Open Tail

- Verify that /screenshots/*.png and /registry.json still serve correctly under the new regex — they are NOT excluded and will hit index.html instead of the filesystem.
- Consider adding exclusions for known static paths beyond /assets/.

## Evidence

- transcript lines 1-89

