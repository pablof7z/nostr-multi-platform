//! T114b — per-dispatch retention audit (PD-021 line-11 M10.5 §G-S2 closer).
//!
//! These tests pin the kernel-side invariant that no actor-routed structure
//! grows once-per-dispatch without bound. The S2 drain analysis
//! (`docs/perf/m10.5/s2-drain-analysis.md`) measured ~38 MiB retained heap
//! after a 30 s flood (0.13 % reclaimed by drain) — genuine unbounded growth.
//!
//! ## Post-fix S2 §G-S2 evidence (`docs/perf/m10.5/s2-retention-audit.md`)
//!
//! | Duration | Dispatches | `retained_heap_after_drain_bytes` | Verdict |
//! |---|---:|---:|---|
//! | 10 s | 100 k  | 519,936 B (~0.5 MiB)  | PASS (≤1 MiB) |
//! | 30 s | 300 k  | 157,956 B (~0.15 MiB) | PASS (≤1 MiB) |
//! | 60 s | 600 k  | 519,748 B (~0.5 MiB)  | PASS (≤1 MiB) |
//!
//! Retention is **flat across 6× the dispatch count** (variance is run-to-run
//! drain-detector noise within the 256 KiB stability band). The kernel is
//! bounded by working set, not dispatch count — the D8 invariant.
//!
//! ## Load-bearing fix (per-dispatch emit gate)
//!
//! The dominant retention source was per-dispatch `emit_now` in `dispatch.rs`:
//! every view command unconditionally serialized a full kernel snapshot and
//! pushed it onto the unbounded kernel→listener `mpsc::channel`. Under flood
//! conditions with no UI consumer (`running=false`, the S2 / S1–S5 harness's
//! Configure-not-Start mode), the channel's internal block free-list retained
//! segments long after the snapshot Strings were processed — measured at
//! ~10 B per dispatch. The fix (`actor/tick.rs::maybe_emit_after_dispatch`)
//! routes view-command emits through a `running=true` gate, mirroring the
//! idle-tick path's existing gating contract. Pinned by the two
//! `view_dispatches_*_when_running` regression tests in `tick.rs`.
//!
//! ## Defense-in-depth
//!
//! The bounded caps pinned by [`profile_claim_cap`] and [`queue_depth`] are
//! **defense-in-depth** — not the load-bearing fix for S2 (the emit gate is).
//! They harden the kernel against adversarial / pathological inputs where a
//! long-lived UI churns unique consumer_ids per pubkey, or an FFI burst
//! saturates the actor channel.
//!
//! | Path                         | Structure                          | Pre-fix    | Bound                                |
//! |------------------------------|------------------------------------|------------|---------------------------------------|
//! | FFI command channel          | bounded `CommandSender` inbox      | unbounded  | `ACTOR_INBOX_CAPACITY=4096` — drop-newest + `command_drops` |
//! | view-command emit gate       | per-dispatch `emit_now`            | unconditional | `maybe_emit_after_dispatch` skips when `running=false` (this fix — load-bearing) |
//! | `resolve_ref`                | `profile_claims[pk]: BTreeSet`     | unbounded  | `MAX_CLAIMS_PER_PUBKEY=256` — drop-newest + `claim_drops_total` |
//! | latency sketch (harness)     | `Vec<u64>` per-sample              | unbounded  | fixed 32-bucket log2 histogram (`s2_dispatch_flood.rs::LatencyHistogram`) — 256 B per thread |
//! | (was: `open_author`)         | deleted — V-112 (ADR-0042)         | —          | —                                          |
//! | (was: `close_author`)        | deleted — V-112 (ADR-0042)         | —          | —                                          |
//! | `release_profile`            | `profile_claims[pk]` (remove)      | bounded    | shrinking only                              |
//! | `open_firehose_tag`          | `diagnostic_firehose: Option`      | bounded    | O(1) — single-slot refcounted              |
//! | `Kernel::log`                | `logs: VecDeque`                   | bounded    | hard cap 80 (`status.rs:314`)               |
//! | `requested_profiles`         | `HashSet<pubkey>`                  | bounded    | keyed by pubkey (O(working-set))            |
//! | `pending_profiles`           | `BTreeSet<pubkey>`                 | bounded    | keyed by pubkey (O(working-set))            |
//! | profile/author/firehose seq  | `u64` counters                     | bounded    | saturating; 8 B fixed                       |
//!
//! Under the S2 spec mix, `claim_drops_total = 0` at flood end — the cap is
//! not being exercised. That is the correct outcome: the working set (50
//! pubkeys × ≤256 consumers) fits inside the bound. The claim cap surfaces on
//! `Metrics` (`update.rs`) for diagnostic visibility; its unit tests in
//! [`profile_claim_cap`] pin the drop-newest semantics for the pathological
//! cases. The FFI command lane is separately bounded by ADR-0029 and exposes
//! its shed-load counter on the shared `CommandSender` / test-support FFI
//! stats path.
//!
//! Production paths that DO populate `wire_subs` (post-`Start`) are bounded
//! by the planner CLOSE diff (`drain_lifecycle_tick` behind `close_interest`)
//! and the interest registry refcount. (V-112: the legacy
//! `close_subscriptions_with_prefixes` view-close path was deleted.)
//!
//! ## T133 — `wire_subs` row eviction
//!
//! Pre-T133 the row table only ever marked closed subs with
//! `state="closed"` / `closed_by_relay`; the row stayed for diagnostic
//! surfacing, so the table grew without bound across long sessions (every
//! profile-claim, thread-ids/replies, and discovery oneshot completes via
//! EOSE→CLOSE — the high-volume retention source the advisor flagged on
//! T114b). T133 evicts rows at every terminal point:
//!
//! | Trigger                          | Action                          |
//! |----------------------------------|---------------------------------|
//! | EOSE for non-keep sub (oneshot)  | `HashMap::remove` after CLOSE outbound |
//! | CLOSED (relay-initiated)         | `HashMap::remove` (no outbound)        |
//! | `relay_closed` (per-URL socket teardown) | `wire_subs.retain(relay_url != …)` |
//! | `relay_closed_all` (global pool drain)   | `wire_subs.retain(role != …)`      |
//! | `relay_failed` (transient)       | no eviction — `state="retrying"` may resume |
//!
//! Pinned by [`wire_subs_eviction`] below; `view_close_evicts_wire_subs_to_zero`
//! deleted (V-112 — used `open_author`/`close_author` which are deleted). The
//! diagnostic-filter call sites at `status.rs:27` / `requests/mod.rs:25,39,80`
//! remain (defense-in-depth — they cost nothing once the row is gone).

mod support;

mod profile_claim_cap;
mod queue_depth;
mod wire_subs_eviction;
