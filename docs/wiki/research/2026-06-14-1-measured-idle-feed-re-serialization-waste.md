---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 9898-9925
agent_attribution: a85df4db283078c1e (Design Tier-1 feed gating rung)
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Measured idle feed re-serialization waste: 58.8 KB/tick byte-identical across 40 idle ticks, feed ~6× rest of frame, release 129µs vs debug 2266µs encode; verdict decisive waste, recommendation staged Option A (rev-gating) then maybe B (row-deltas)"
captured_at: 2026-06-14T14:17:15Z
---

Measured idle feed re-serialization waste: 58.8 KB/tick byte-identical across 40 idle ticks, feed ~6× rest of frame, release 129µs vs debug 2266µs encode; verdict decisive waste, recommendation staged Option A (rev-gating) then maybe B (row-deltas)

---

User: [Agent task result: Agent "Design Tier-1 feed gating rung" completed]
Working tree is clean — no production code landed, the throwaway spike is deleted, no PR opened.

📋 SUMMARY: Measured the idle-feed re-serialization waste, designed the Rung-6 Tier-1 gating two ways, got a codex review, and posted a greenlight-gated plan to issue #1415.

🔍 ANALYSIS: The measurement was decisive. The home feed re-serializes a byte-identical ~58.8 KB payload on every idle 4 Hz tick (confirmed byte-identical across 40 idle ticks), and that payload is ~6× the entire rest of the frame. The production closure at `op_feed_defaults.rs:267` snapshots + encodes the engine unconditionally every tick, and the engine has no rev/dirty/generation counter today. The release-vs-debug timing (129 µs vs 2,266 µs, a 17.6× factor) is itself a load-bearing finding: the idle byte waste is large and real, but the CPU-jank attribution is not yet proven in a release build.

⚡ ACTIONS: Read the Rung-3 ADR + pr-ladder + the feed build path; wrote and ran a throwaway crate-level spike against the real op_feed engine (then deleted it); ran a codex review at high reasoning; posted the plan via `gh issue comment 1415`.

✅ RESULTS: Plan posted at https://github.com/pablof7z/nostr-multi-platform/issues/1415#issuecomment-4701727623. Recommendation (mine and codex's): staged A-then-maybe-B — extend rev-gating to the feed plus cheap whole-value gating for `claimed_event_embeds`/`nip46_onboarding` now; defer the expensive row-delta Option B behind a release/device measurement gate.

📊 STATUS: Design + measurement + review complete. No production code, no PR. Working tree clean.

📁 CAPTURE: Empirical anchors for #1415 — feed payload ~58.8 KB/tick, 80-card window, byte-identical on idle, ~129 µs release / ~2,266 µs debug encode; feed is ~6× the rest of the frame and is absent from the bare S6 harness (so Rung-3's 18%/68.8% understated the whole-product win). Cardinal trap for Option A: omit==retain host-side, so a missed generation bump = permanently stale feed UI; the generation counter must be content-correct and exhaustively tested.

➡️ NEXT: Await owner greenlight. If approved, the R6-S1 first step is a Rust-owned visible-window generation/epoch on the op_feed engine; before committing to Option B, run the R6-S5 release/device measurement to confirm mutating-feed frames (not idle churn or SwiftUI broad re-render) are the remaining bottleneck.

📖 STORY EXPLANATION:
1. The fundamental constraint here is that the kernel emits a full snapshot at 4 Hz, and Rung 3 only taught the Tier-2 built-ins to stay quiet when unchanged — the feed never learned that lesson.
2. I measured before designing: a throwaway spike on the real op-feed engine showed the feed re-emits a byte-identical ~58.8 KB payload on every idle tick — pure waste against the dominant payload.
3. That payload is roughly six times the entire rest of the frame, and it never appeared in the Rung-3 numbers because the bare measurement harness doesn't even register the feed — so the prior win was understated.
4. The engine has no notion of "did anything change" — every tick it sorts all roots and clones every visible card from scratch, which is the architectural gap both options must close first.
5. Option A makes the feed a rev-gated key so an unchanged feed is omitted and the host cache retains it — simple, composes with Rung 3, and kills the idle waste outright.
6. Option B sends per-row deltas so one new event ships one card instead of the whole window — a bigger win on busy feeds, but it needs stable row identity, base-rev validation, and far more correctness surface.
7. Codex pressure-tested both and agreed: do A first, give the two small non-list keys cheap gating immediately, and gate B behind real release-build evidence — because the 17.6× debug factor means the jank may live in the Debug build or SwiftUI, not the encode.
8. The plan is posted to #1415 with a sub-step ladder and an explicit await-greenlight note; no production code was written, the spike was deleted, and the tree is clean.

🎯 COMPLETED: Measured the feed idle waste, designed Rung 6, posted the greenlight-gated plan.
