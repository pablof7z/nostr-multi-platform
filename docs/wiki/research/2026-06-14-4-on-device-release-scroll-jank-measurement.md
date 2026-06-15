---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 11024-11051
agent_attribution: a78dfc8e9ad414985
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "On-device Release scroll-jank measurement: idle flat 60fps zero hitches, 12s Time Profiler zero hangs/hang-risks; verdict felt jank was Debug build, Release scrolls clean"
captured_at: 2026-06-14T20:53:43Z
---

On-device Release scroll-jank measurement: idle flat 60fps zero hitches, 12s Time Profiler zero hangs/hang-risks; verdict felt jank was Debug build, Release scrolls clean

---

User: [Agent task result: Agent "On-device scroll hitch measurement (retry)" completed]
The voice-server endpoint returns 404 (server not running), but `say` delivered the spoken notification per the owner's global instruction. The work is complete.

📋 SUMMARY: On-device Release-build scroll-jank measurement for #1415 is done; the felt jank was the Debug build — Release device scrolls clean.

🔍 ANALYSIS: Idle is a flat 60fps with zero hitches; a 12s on-device Time Profiler scroll trace showed zero hangs, zero hang-risks, and Nominal thermal with no main-thread bottleneck.

⚡ ACTIONS: Built Release core + app, installed/launched on the physical iPhone, instrumented a CADisplayLink hitch counter surfaced via UI, ran contamination-free XCUITest phases, captured a device Time Profiler trace, consulted codex twice, fully reverted the spike.

✅ RESULTS: Device-truth confirmed — Release scrolling has no hang-class problem; the 6.2%/80ms scroll hitches are real-but-minor (3-card rubber-band bounce at 60Hz).

📊 STATUS: Spike reverted, tree clean; the #1415 comment is queued to auto-post when the GitHub rate limit resets (~17 min, background task bpqsmw3e6).

📁 CAPTURE: Key facts for future device work — xcodebuild destination uses hardware UDID `00008150-001E118E3CD2401C` (devicectl uses coredevice `3C438D9B-…`); Instruments works via `xctrace --device-name "Pablo's iPhone" --attach Chirp7z` (process name, not bundle id); `log collect` needs root (unavailable) and libimobiledevice can't see the WiFi-paired device, so surface metrics through the UI; clean device builds need `BuildInfo.generated.swift` present pre-xcodegen (it's gitignored) and `ChirpTests` is broken at HEAD; the fixture account's feed is only ~3 cards.

➡️ NEXT: Stop optimizing the home-feed scroll path for #1415; optional low-severity follow-up for `TypedProjectionGlue.relayDiagnostics*` decoding on the home feed; only a ≥50-card busy-account + forced-120Hz run remains as a stricter (non-blocking) gap.

📖 STORY EXPLANATION:
1. The previously-blocked device path is now open — Pablo's iPhone 17 Pro Max was fully reachable over the WiFi tunnel for install, launch, automation, and Instruments.
2. I built a full Release core and app (no testability override) and installed it on the real device, working around two genuine clean-worktree build potholes (gitignored BuildInfo not compiled; ChirpTests broken at HEAD).
3. A CADisplayLink frame-hitch counter showed idle is a flat 60fps with zero hitches; the only idle hitches came from XCUITest's own accessibility snapshots, a measurement artifact codex confirmed.
4. A contamination-free re-run (read counters once per phase boundary) gave clean deltas: idle 1.4%, scroll 6.2% (max 80ms), recovery 1.1% — but on a thin 3-card feed at 60Hz.
5. The decisive evidence is the on-device Time Profiler trace: zero potential-hangs, zero hang-risks, Nominal thermal, and main-thread work spread thin across SwiftUI layout / AttributeGraph / UICollectionView with no dominant hotspot.
6. One mild surprise — the relay-diagnostics typed projection is decoded on the home feed during scroll (wasted work, not the jank cause) — worth a small follow-up.
7. The verdict is (b): the felt jank was the Debug build; Release on real ProMotion hardware is healthy, corroborating the earlier simulator A/B on actual hardware.
8. The spike is fully reverted (clean tree, no PR) and the detailed #1415 comment is queued to post automatically once GitHub's rate limit resets.

🎯 COMPLETED: Release device scrolls clean; felt jank was the Debug build.
