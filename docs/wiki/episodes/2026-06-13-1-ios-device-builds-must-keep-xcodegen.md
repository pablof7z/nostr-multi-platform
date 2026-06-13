---
type: episode-card
date: 2026-06-13
session: 10fcbaec-12f8-4c59-9c2d-38d1c1f7a9c2
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/10fcbaec-12f8-4c59-9c2d-38d1c1f7a9c2.jsonl
salience: root-cause
status: active
subjects:
  - ios-build-recipe
  - pbxproj-restoration-policy
  - buildinfo-generated-swift
supersedes:
  - 2026-06-13-1-don-t-restore-pbxproj-after-xcodegen
related_claims: []
source_lines:
  - 68-83
captured_at: 2026-06-13T21:12:25Z
---

# Episode: iOS device builds must keep xcodegen-generated pbxproj, not restore from git

## Prior State

The build recipe instructed restoring project.pbxproj from git (`git checkout HEAD -- project.pbxproj`) after running xcodegen, under the assumption the committed file was correct for all builds.

## Trigger

Restoring the committed pbxproj after xcodegen caused a Swift compile error — 'cannot find BuildInfo in scope' — because the committed pbxproj lacks the file reference for BuildInfo.generated.swift (the file is gitignored, so git doesn't track its project reference).

## Decision

For device builds, the xcodegen-generated pbxproj must be kept as-is; do NOT restore it from git after running xcodegen.

## Consequences

- The git-tracked pbxproj is now recognized as incomplete for device builds (missing BuildInfo.generated.swift reference)
- Build recipe memory updated to reflect this correction
- Future device builds must skip the git-checkout-restoration step that was previously standard procedure

## Open Tail

- Whether CI/release builds also need this adjustment, or only local device builds where xcodegen is run fresh
- Whether BuildInfo.generated.swift should be un-gitignored so its project reference can be committed

## Evidence

- transcript lines 68-83

