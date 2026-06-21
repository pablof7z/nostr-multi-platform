---
type: episode-card
date: 2026-05-26
session: f2fd46d3-1cbd-4f80-9469-0d8137d75478
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/f2fd46d3-1cbd-4f80-9469-0d8137d75478.jsonl
salience: root-cause
status: active
subjects:
  - nmp-registry
  - vercel-deploy
  - cross-directory-imports
supersedes: []
related_claims: []
source_lines:
  - 256-258
  - 327-328
  - 344-345
  - 500-504
  - 543-543
  - 571-589
  - 641-647
  - 656-661
  - 671-672
  - 698-702
  - 741-758
captured_at: 2026-06-18T06:02:00Z
---

# Episode: Registry requires local prebuilt deploy — server-side Vercel build cannot resolve cross-directory imports

## Prior State

The Vercel project was configured for web/chirp and assumed server-side builds (`npm ci` / `npm run build` on Vercel's infra) would work for any web/ project in this monorepo.

## Trigger

Repeated build failures when deploying web/registry: Vercel's server-side builder only uploads files from the project root, but registry's Vite config imports Swift sources via `../../../../crates/nmp-cli/registry/swiftui/...`, which are outside that subtree and invisible to the build.

## Decision

Registry deploys must use the local prebuilt workflow (`vercel build --prod` then `vercel deploy --prebuilt --prod`), where Vite runs locally with the full repo tree available. Root `vercel.json`, `.vercelignore`, and `.vercel/project.json` were re-pointed from chirp to registry.

## Consequences

- All future registry production deploys require building locally before pushing — server-side CI builds will fail
- Any other web/ project that imports outside its own directory will have the same constraint
- Root-level Vercel config is now registry-specific; deploying chirp to a different Vercel project would need separate project linkage
- The `.vercelignore` pattern was inverted to allow web/registry and its transitive crate dependencies

## Open Tail

- CI/CD pipeline (if added) must replicate the local-build-then-upload pattern rather than relying on Vercel's default build step
- Domain alias nmpui.f7z.io confirmation was not verified in this session

## Evidence

- transcript lines 256-258
- transcript lines 327-328
- transcript lines 344-345
- transcript lines 500-504
- transcript lines 543-543
- transcript lines 571-589
- transcript lines 641-647
- transcript lines 656-661
- transcript lines 671-672
- transcript lines 698-702
- transcript lines 741-758

