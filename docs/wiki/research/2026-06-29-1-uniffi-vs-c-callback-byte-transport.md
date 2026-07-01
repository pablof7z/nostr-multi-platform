---
type: research-record
date: 2026-06-29
session: 3c942260-311d-4e00-8bcc-204045ea87b3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/3c942260-311d-4e00-8bcc-204045ea87b3.jsonl
source_lines: 2127-2152
agent_attribution: subagent (workflow #2388)
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "UniFFI vs C-callback byte-transport A/B microbenchmark — COLLAPSE verdict: pure UniFFI sufficient, surcharged weighted-p99 delta 2,142 ns = 0.013% of 60fps budget, ~390× below pre-registered threshold"
captured_at: 2026-06-29T11:28:02Z
---

UniFFI vs C-callback byte-transport A/B microbenchmark — COLLAPSE verdict: pure UniFFI sufficient, surcharged weighted-p99 delta 2,142 ns = 0.013% of 60fps budget, ~390× below pre-registered threshold

---

User: [Tool result]: 1	{
2	  "summary": "Design+build+review the UniFFI-vs-C-callback byte-push benchmark (#2388)",
3	  "agentCount": 3,
4	  "logs": [],
5	  "result": {
6	    "design": {
7	      "comparison_approach": "In-process Rust A/B microbench replaying ONE deterministic sequence of frame-byte buffers through two transports, so the only variable is marshalling. Lane A (C lane): the real wiring from app_lifecycle_ffi.rs — invoke an `extern \"C\" fn(*mut c_void, *const u8, usize)` through the same `Arc<dyn Fn(&[u8])>` closure shape (UpdateListener), zero-copy ptr/len, PLUS one representative \"shell consume\" memcpy out of the transient slice (mandatory for fairness: the real contract says the slice is valid only for the callback duration, so every real host copies once). Lane B (UniFFI lane): link the REAL `uniffi` runtime crate and exercise its actual lowering path for `Vec<u8>` — `RustBuffer::from_vec` (real alloc + memcpy) + `FfiConverter::<Vec<u8>>::lower` + an indirect vtable call into a Rust

Assistant: The benchmark verdict is in, and it's decisive. Let me grab the review verdict to confirm the PR is sound:

User: [Tool result]: 38	    "review": {
39	      "findings": [
40	        {
41	          "severity": "major",
42	          "issue": "Mislabeled tail metric / overclaim of measured precision. The JSON/MD fields named p50_ns/p95_ns/p99_ns are NOT per-frame wall times — they are percentiles over batch-of-1000 *averages* (time_c_lane / time_uniffi_lane time chunks of SMALL_BATCH_K=1000 and divide, then percentile over the resulting ~100 batch-means per bucket). Averaging over 1000 frames smooths the tail by ~1000x (central-limit), so the reported 'p99' dramatically understates true per-frame p99 (GC pauses, page faults, scheduler hops are averaged away). The issue explicitly asked for 'p99 callback wall time', and the pre-registered absolute gate ('UniFFI SMALL p99 < 250us') is fed the batch-mean p99 (945 ns). The COLLAPSE verdict survives because the margin is ~250x on the absolute gate and 2-3 orders of magnitude on the delta, so the outcome does not flip — but the metric should be relabeled (e.g. p99-of

Assistant: **The benchmark verdict is in: COLLAPSE — pure UniFFI, no internal byte lane needed.** This is the hard confirmation behind your "universal UniFFI" call:

| | C-lane p99 | UniFFI p99 | |
|---|---|---|---|
| Small (256B–2KB) | 91 ns | 945 ns | sub-µs, negligible |
| Weighted | 347 ns | 2054 ns | |

Surcharged weighted-p99 delta = **2,142 ns = 0.013% of the 16.67ms 60fps budget — 390× below the pre-registered COLLAPSE threshold**, even with a 3× JNI/ARC penalty, robust across 3 runs. The "UniFFI too slow for the push lane" assumption was wrong by ~390×. So **all 4 byte-lane "exception" candidates migrate cleanly — zero internal-ABI exceptions**, including the update-sink hot path.

The review confirms `methodology_sound: true` but `request_changes` on **honesty issues that don't flip the verdict**: the `p99` fields are actually percentiles-of-1000-frame-*means* (smooths the tail — needs relabeling), and the COLLAPSE should carry the explicit "foreign boundary is synthetic/unmeasured" caveat. Real but non-blocking. Dispatching a fix so PR #2391 is honest and ready for the head-coordinator (I won't merge):
