---
type: episode-card
date: 2026-05-25
session: 53838558-81bd-433d-a46d-d117ecebb361
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/53838558-81bd-433d-a46d-d117ecebb361.jsonl
salience: reversal
status: superseded
subjects:
  - vercel-routing
  - registry-static-assets
supersedes: []
related_claims: []
source_lines:
  - 7444-7453
  - 7696-7698
  - 7704-7708
captured_at: 2026-06-18T05:29:54Z
---

# Episode: Vercel catch-all rewrite blocked all static asset serving on nmpui.f7z.io

## Prior State

web/registry/vercel.json contained {"rewrites": [{"source": "/(.*)", "destination": "/index.html"}]} which matched every request path, including /screenshots/*.png and /registry.json, routing them all to the SPA shell.

## Trigger

Deployed registry site returned HTML (the SPA shell) for every URL including screenshot images and registry.json; curl confirmed content-type: text/html with ~14KB response for PNG paths.

## Decision

Remove the catch-all rewrite from vercel.json entirely. Vercel's Vite framework preset already provides correct SPA-fallback routing (with check: true on the filesystem handler) while continuing to serve real static files from the build output. The explicit rewrite was both redundant and actively harmful.

## Consequences

- Static assets (screenshots, registry.json) now served correctly with proper content types
- Future deployments from the nmp-registry project will use Vercel's default Vite routing without manual intervention
- PR #572 merged; the clean vercel.json contains only build/install commands and framework declaration

## Open Tail

*(none)*

## Evidence

- transcript lines 7444-7453
- transcript lines 7696-7698
- transcript lines 7704-7708

