---
type: episode-card
date: 2026-06-13
session: 10fcbaec-12f8-4c59-9c2d-38d1c1f7a9c2
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/10fcbaec-12f8-4c59-9c2d-38d1c1f7a9c2.jsonl
salience: architecture
status: superseded
subjects:
  - ios-build-recipe
  - xcodegen-pbxproj-policy
supersedes: []
related_claims: []
source_lines:
  - 66-83
captured_at: 2026-06-13T19:12:42Z
---

# Episode: Don't restore pbxproj after xcodegen on device builds

## Prior State

The build recipe instructed restoring project.pbxproj via `git checkout HEAD -- project.pbxproj` after running xcodegen, on the assumption the committed file was canonical.

## Trigger

iOS device build failed with Swift compile error 'cannot find BuildInfo in scope' when the restored (committed) pbxproj was used — it omits the file reference for gitignored BuildInfo.generated.swift, while xcodegen's freshly-generated pbxproj correctly includes it.

## Decision

xcodegen's generated pbxproj must be kept (not git-restored) for device builds. The prior recipe step of restoring pbxproj after xcodegen is removed.

## Consequences

- Device builds will include BuildInfo.generated.swift correctly and compile without scope errors
- The committed pbxproj in git is incomplete for device builds (lacks the gitignored file reference)
- Build recipe memory was updated to reflect this correction

## Open Tail

- Whether the committed pbxproj should be updated or a different gitignore strategy adopted for BuildInfo.generated.swift

## Evidence

- transcript lines 66-83

