# Codex post-merge review - iOS T103 envelope unwrap + kind:6 repost (8f7bbad)

## Verdict

PARTIAL.

The snapshot unwrap fixes the reported onboarding failure: all three bridges now extract envelope `v` only for `t == "snapshot"`, so `apply(update:)` can receive `activeAccount` again (`KernelModel.apply` mirrors it into the RootView/RootShell gate). Unknown and non-snapshot frames return `nil` without crashing.

## Findings

- `ios/NmpPulse/NmpPulse/Bridge/KernelBridge.swift:134` and `ios/NmpStress/NmpStress/KernelBridge.swift:80` - Medium - REPORT: envelope parse and snapshot decode failures still return `nil` via `try?` with no log/toast/status. Chirp current master logs parse/decode failures at `ios/Chirp/Chirp/Bridge/KernelBridge.swift:158` and `:177`, but Pulse/Stress can still regress silently.
- `ios/Chirp/Chirp/Components/NoteRowView.swift:136` - Medium - REPORT: repost content parsing is protocol/event-shape logic in the Swift app. For D0 inverse, the durable fix is a kernel/shared content projection (`displayContent`, `isRepost`, or nodes), not per-app Nostr JSON parsing.
- `crates/nmp-core/src/update_envelope.rs:48` and `crates/nmp-core/src/actor/dispatch.rs:207` - Low - REPORT: T103 has a canonical two-shape envelope and the actor emits discrete `update` frames, but the Swift bridges hand-parse only `snapshot` and silently drop `update`. That is graceful for current snapshot renderers, but not a typed shared consumer contract.
- `ios/Chirp/Chirp/Bridge/KernelBridge.swift:155`, `ios/NmpPulse/NmpPulse/Bridge/KernelBridge.swift:129`, and `ios/NmpStress/NmpStress/KernelBridge.swift:74` - Low - REPORT: the envelope unwrap is copy-pasted across all three bridges. Extract a shared Swift helper when bridge code is consolidated.
- `ios/Chirp/Chirp/Components/NoteRowView.swift:136` - Low - FIX: any valid JSON object was treated as a repost, so a kind:1 note containing JSON could be relabeled/hidden. Fixed in `3b7a3e1b` by requiring event-shaped JSON before unwrapping.
- `ios/Chirp/project.yml:12` and `ios/Chirp/Chirp.xcodeproj/project.pbxproj:312` - Low - FIX: `project.yml` is the source of truth for `DEVELOPMENT_TEAM=456SHKPP26`. Running `xcodegen generate` preserved that and exposed one current-master drift: `RelayDetailView.swift` was missing from the generated project. Fixed in `3b7a3e1b`.

## What codex fixed in-place

- `3b7a3e1b fix(codex): tighten Chirp repost unwrap and project regen`
  - Tightened `effectiveContent` so invalid JSON and non-event JSON render unchanged; nested repost content is displayed literally, with no recursive parse path.
  - Regenerated `ios/Chirp/Chirp.xcodeproj` with `xcodegen`, adding `RelayDetailView.swift` to the project while preserving `456SHKPP26`.

## REPORT-class follow-ups for orchestrator

- Decide whether Pulse/Stress host-decode failures should become model-visible toast/status fields or structured logs, then remove the silent `try?` path.
- Extract one Swift T103 envelope decoder used by Chirp, NmpPulse, and NmpStress; preferably return a typed `snapshot/update/ignored` result.
- Move kind:6 display normalization out of Chirp UI into the kernel/shared content projection before other apps need repost rendering.

## Doctrine compliance

- D0 inverse: PARTIAL. T103 unwrap stays bridge-side, but repost normalization is app-side protocol parsing and should move into the kernel/shared projection.
- D1: PASS/PARTIAL. Invalid JSON and non-event JSON now fall through unchanged; event-shaped repost JSON unwraps best-effort. The remaining partial is that repost normalization lives in Swift rather than a shared projection.
- D6: PARTIAL. No exceptions cross FFI and malformed frames do not crash, but Pulse/Stress still swallow decode failures silently instead of surfacing a toast/status/log.
- D7: N/A. No capability policy decision added.
- D8: PASS for snapshot reactivity. The fix restores snapshot application; discrete `update` frames are ignored by current Swift renderers and need a future typed-consumer decision.

## File size

- Hand-authored touched files remain below the 500 LOC hard cap. `NoteRowView.swift` is 272 LOC; `NmpPulse` and `NmpStress` bridges are below 300. `Chirp/Bridge/KernelBridge.swift` is 318 LOC on current master, a soft-cap follow-up for the bridge-helper extraction. Generated `project.pbxproj` is exempt.

## Verification

- Read `/tmp/codex-prompts/diff-8f7bbad.txt`.
- Inspected live Swift bridge/model/root files and `crates/nmp-core/src/update_envelope.rs`.
- Ran `xcodegen generate` in `ios/Chirp`; no app build.
- Ran `git diff --check`.
- Did not run cargo tests or xcodebuild per budget.
