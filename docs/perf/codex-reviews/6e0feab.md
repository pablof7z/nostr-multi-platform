# Codex Post-Merge Review — 6e0feab

**Commit:** `6e0feab fix(bench): profile_thrashing drain-timing — extend wait, poll-until-update, log on timeout`
**Reviewed at:** 2026-05-18

## Fixed

None. No commit or push (codex sandbox write-outside-project restriction).

The following rustfmt-style fixes were identified by codex and applied manually by the bench-drain-fix agent in a follow-up commit:
- Collapse multi-line `eprintln!(...)` in `drain_until` to single line
- Expand long `assert!`/`assert_eq!` calls to multi-line in test module (rustfmt canonical form)
- Reorder `use crate::report::ScenarioMetrics` before `use crate::scenarios::...` in `cold_start.rs`
- Collapse long `Some("...")` argument in `gate_max` call in `profile_thrashing.rs`

## Checks Passed

- `cargo clippy -p nmp-testing --bin firehose-bench --tests -- -D warnings` clean
- `cargo test -p nmp-testing --bin firehose-bench` passed: 3/3 drain_until tests

## Follow-up (REPORT-class)

**correctness, needs test** — `live/mod.rs:34` + `profile_thrashing.rs:97`

`drain_until` waits for the first update to arrive, then drains only updates already queued at that moment. It does NOT prove actor command-queue quiescence after the burst. `final_update` can still be a pre-final snapshot if the actor emits a first update while still processing queued claims/releases, and a later update with the true final state arrives after the drain completes.

Suggested approach: add an identifiable actor barrier message (e.g. a Shutdown-ack or quiescence sentinel), OR implement a quiet-window drain (wait for first update, then poll with a short window until no further updates arrive for N ms). Add a regression test where stale and final updates arrive separated in time to demonstrate the helper returns the final one.

This is not a blocker for the T51 fix (which specifically addresses the `None` → `usize::MAX` crash path), but is worth a follow-up task for rigorous quiescence.
