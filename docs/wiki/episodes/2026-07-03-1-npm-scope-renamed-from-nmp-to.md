---
type: episode-card
date: 2026-07-03
session: 04745411-a0c1-4523-ac83-71dc983f410b
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/04745411-a0c1-4523-ac83-71dc983f410b.jsonl
salience: reversal
status: active
subjects:
  - npm-scope
  - nmpis
  - public-namespace
  - release-manifest
supersedes: []
related_claims: []
source_lines:
  - 67-68
  - 79-82
  - 1034-1049
  - 1071-1072
captured_at: 2026-07-03T09:41:42Z
---

# Episode: npm scope renamed from @nmp to @nmpis

## Prior State

The release manifest and all web packages hardcoded the @nmp npm scope; the assistant's initial recommendation was to keep NMP as-is to avoid a 67-crate rename on the critical path.

## Trigger

npm availability checks showed @nmp scope was not claimable by the user (403/404 on org and user endpoints), while @nmpis was genuinely unclaimed; the user confirmed ownership of @nmpis org.

## Decision

Renamed the public npm scope from @nmp to @nmpis across all 4 web packages (runtime-web, components-web, gallery-web, registry-web), the release manifest, lockfile, doctrine-lint boundary-token gate, nmp-codegen doc-comments, CI typecheck workflow, vercel.json, and docs. Rust crate names remain nmp-*.

## Consequences

- PR #2819 merged to master; release manifest gate passes with 61 public crates and 2 public npm packages under @nmpis
- Doctrine-lint smoke 183/183 including new gates: nmp_uniffi_directory_does_not_exist and nmp_uniffi_is_not_reintroduced_as_live_crate_in_source
- All four web packages typecheck clean after rebuild; scoped diff was 26 files, 74 insertions, 74 deletions
- @nmpis org confirmed owned by pablof7z on npmjs.com

## Open Tail

- Gallery-web and registry-web remain at 0.1.0 version, not yet bumped to match the workspace release version

## Evidence

- transcript lines 67-68
- transcript lines 79-82
- transcript lines 1034-1049
- transcript lines 1071-1072

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-07-03-1-npm-scope-renamed-from-nmp-to.json`](transcripts/2026-07-03-1-npm-scope-renamed-from-nmp-to.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-07-03-1-npm-scope-renamed-from-nmp-to.json`](transcripts/raw/2026-07-03-1-npm-scope-renamed-from-nmp-to.json)
