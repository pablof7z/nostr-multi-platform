---
type: episode-card
date: 2026-05-26
session: 0048057e-cb95-4da0-9f74-039a07dfc89f
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/0048057e-cb95-4da0-9f74-039a07dfc89f.jsonl
salience: product
status: active
subjects:
  - chirp-onboarding
  - build-info-footer
supersedes: []
related_claims: []
source_lines:
  - 225-410
  - 421-428
  - 541-661
  - 738-740
  - 754-845
  - 856-867
captured_at: 2026-06-18T05:58:33Z
---

# Episode: Build info footer on welcome screen

## Prior State

Welcome screen had no build metadata; developers had no way to visually verify which branch/commit/build was running on the simulator.

## Trigger

User directive: 'add to the bottom of the welcome screen the current branch, commit hash and build time -- this should be picked up automatically and rendered automatically without us having to update it manually'

## Decision

Added an Xcode pre-build Run Script phase in project.yml that shells out to git (branch, short hash) and date (UTC) to write BuildInfo.generated.swift, then displays it on the welcome screen via .safeAreaInset(edge: .bottom). The generated file is gitignored and regenerated on every build.

## Consequences

- Welcome screen now shows 'master · b7bbe007 · 2026-05-26 13:21 UTC' at the bottom automatically
- Initial VStack insertion of the build label caused a SIGTRAP crash in SwiftUI's FrameLayoutCommon on iOS 26 beta; resolved by switching to .safeAreaInset which avoids modifying the existing VStack layout
- Chirp7z.debug.dylib must be present in the app bundle — a clean DerivedData build without Rust artifacts produces a dyld abort; full rebuild chain (Rust → Xcode) is required
- Bundle ID is io.f7z.chirp and app bundle is Chirp7z.app (not Chirp.app) — the justfile still references com.example.Chirp

## Open Tail

- Justfile run-ios still hardcodes wrong bundle ID (com.example.Chirp) and targets 'iPhone 17' instead of 'iPhone 17 Pro'

## Evidence

- transcript lines 225-410
- transcript lines 421-428
- transcript lines 541-661
- transcript lines 738-740
- transcript lines 754-845
- transcript lines 856-867

