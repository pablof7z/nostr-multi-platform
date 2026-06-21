---
type: episode-card
date: 2026-05-25
session: 5a40faff-56c9-442d-ad96-59432b6f2fea
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/5a40faff-56c9-442d-ad96-59432b6f2fea.jsonl
salience: root-cause
status: superseded
subjects:
  - nmp-registry
  - vercel-deploy
  - raw-imports
supersedes: []
related_claims: []
source_lines:
  - 327-341
  - 395-517
  - 558-662
captured_at: 2026-06-18T05:35:45Z
---

# Episode: Registry deploy must be prebuilt locally due to out-of-tree ?raw imports

## Prior State

Registry deployed from web/registry/ directly via `vercel --prod`, which uploads only that subdirectory and lets Vercel build remotely

## Trigger

Production deploy failed — `vercel --prod` from web/registry/ only uploads that subdirectory, but the build resolves ?raw imports from ../../../../crates/nmp-cli/registry/ which are absent on Vercel's build server

## Decision

Switch registry deploys to a two-step local-build-then-deploy pattern: `vercel build --prod` (local, full filesystem access) followed by `vercel deploy --prebuilt --prod` (upload only the built output)

## Consequences

- All future nmp-registry deploys must use the --prebuilt pattern; direct `vercel --prod` from the subdirectory will fail
- The ?raw import architecture from crates/ creates a hard dependency on the full repo tree at build time, meaning Vercel cannot build the project in isolation
- Other Vercel projects in the monorepo that also use ?raw out-of-tree imports will hit the same failure mode

## Open Tail

- Should the registry's vercel.json or a project-level config be updated to encode the prebuilt deploy requirement so future deploys don't repeat this failure?
- Consider whether the ?raw imports should be inlined/bundled before deploy to eliminate the out-of-tree dependency

## Evidence

- transcript lines 327-341
- transcript lines 395-517
- transcript lines 558-662

