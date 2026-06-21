---
type: episode-card
date: 2026-05-21
session: 30bf8c76-8be2-4e26-b22d-30ca86c37162
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/30bf8c76-8be2-4e26-b22d-30ca86c37162.jsonl
salience: root-cause
status: active
subjects:
  - chirp-ios-build
  - library-search-paths
  - xcode-project-yml
supersedes: []
related_claims: []
source_lines:
  - 297-298
  - 302-310
  - 357-358
  - 399-399
  - 408-408
  - 420-423
captured_at: 2026-06-18T04:43:20Z
---

# Episode: Scope Rust library search paths by SDK to fix iOS device builds

## Prior State

LIBRARY_SEARCH_PATHS in project.yml listed simulator and device Rust archive paths side-by-side without SDK qualification, allowing the linker to resolve the simulator (aarch64-apple-ios-sim) library before the device (aarch64-apple-ios) one when targeting a physical iPhone

## Trigger

Linker error building for device: 'building for iOS, but linking in object file built for iOS-simulator' — the search path order caused the wrong architecture static lib to be selected

## Decision

Split LIBRARY_SEARCH_PATHS into SDK-conditional entries (sdk=iphoneos* and sdk=iphonesimulator*) in project.yml so device builds only resolve aarch64-apple-ios paths and simulator builds only resolve aarch64-apple-ios-sim paths

## Consequences

- Device builds of Chirp now link correctly without requiring manual xcodebuild overrides
- Xcode IDE and xcodebuild both work for device targets out of the box
- Future Rust library additions only need to be placed in the correct SDK-scoped directory to be picked up correctly

## Open Tail

*(none)*

## Evidence

- transcript lines 297-298
- transcript lines 302-310
- transcript lines 357-358
- transcript lines 399-399
- transcript lines 408-408
- transcript lines 420-423

