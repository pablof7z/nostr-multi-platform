---
type: episode-card
date: 2026-05-25
session: 3de5a430-eb71-466a-a3d0-eb58e2b42276
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/3de5a430-eb71-466a-a3d0-eb58e2b42276.jsonl
salience: root-cause
status: active
subjects:
  - vercel-deployment
  - monorepo-build
supersedes:
  - 2026-05-25-1-registry-deploy-must-be-prebuilt-locally
related_claims: []
source_lines:
  - 400-746
captured_at: 2026-06-18T05:36:58Z
---

# Episode: Monorepo ?raw imports require prebuilt Vercel deploys

## Prior State

vercel.json used simple build commands (npm install / npm run build) assuming Vercel could build in-place from the project directory. No special deployment procedure was documented.

## Trigger

Multiple deploy attempts failed because Vite ?raw imports reference source files 4 levels up (into crates/). When Vercel CLI deploys from web/registry, those files don't exist on the build server. Setting rootDirectory caused path-doubling errors, and cd-prefixed commands in vercel.json failed because the directory didn't exist in the uploaded context.

## Decision

Deploy using local build + --prebuilt: run `vercel build --prod` then `vercel deploy --prebuilt --prod` from within web/registry. This ensures ?raw imports resolve against the local monorepo before the built output is uploaded.

## Consequences

- All future CLI deploys of nmp-registry must use the --prebuilt workflow; a plain `vercel --prod` will fail.
- rootDirectory was explored and reverted — it causes path doubling when used with CLI deploys.
- Vercel git integration (which clones the full repo) is recommended as a long-term alternative.
- vercel.json was restored to simple commands (no cd prefixes) since the build step now runs locally.

## Open Tail

- Set up Vercel git integration to automate builds from the full repo and eliminate the need for manual --prebuilt deploys.

## Evidence

- transcript lines 400-746

