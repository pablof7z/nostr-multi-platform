---
type: research-record
date: 2026-06-29
session: 3c942260-311d-4e00-8bcc-204045ea87b3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/3c942260-311d-4e00-8bcc-204045ea87b3.jsonl
source_lines: 2143-2152
agent_attribution: main
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "UniFFI vs C-lane byte transport benchmark: COLLAPSE verdict — surcharged weighted-p99 delta 2,142 ns is 390× below pre-registered threshold across 3 runs"
captured_at: 2026-06-29T11:50:11Z
---

UniFFI vs C-lane byte transport benchmark: COLLAPSE verdict — surcharged weighted-p99 delta 2,142 ns is 390× below pre-registered threshold across 3 runs

---

Assistant: **The benchmark verdict is in: COLLAPSE — pure UniFFI, no internal byte lane needed.** This is the hard confirmation behind your "universal UniFFI" call:

| | C-lane p99 | UniFFI p99 | |
|---|---|---|---|
| Small (256B–2KB) | 91 ns | 945 ns | sub-µs, negligible |
| Weighted | 347 ns | 2054 ns | |

Surcharged weighted-p99 delta = **2,142 ns = 0.013% of the 16.67ms 60fps budget — 390× below the pre-registered COLLAPSE threshold**, even with a 3× JNI/ARC penalty, robust across 3 runs. The "UniFFI too slow for the push lane" assumption was wrong by ~390×. So **all 4 byte-lane "exception" candidates migrate cleanly — zero internal-ABI exceptions**, including the update-sink hot path.

The review confirms `methodology_sound: true` but `request_changes` on **honesty issues that don't flip the verdict**: the `p99` fields are actually percentiles-of-1000-frame-*means* (smooths the tail — needs relabeling), and the COLLAPSE should carry the explicit "foreign boundary is synthetic/unmeasured" caveat. Real but non-blocking. Dispatching a fix so PR #2391 is honest and ready for the head-coordinator (I won't merge):
