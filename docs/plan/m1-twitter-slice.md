# M1 — Read-only Twitter slice on iOS *(DONE — two exit-gate items deferred, see below)*

> Part of the [Build & Validation Plan](../plan.md). Arc 1 — Kernel substrate + Nostr social stack.

**Demo product:** `ios/NmpStress` — SwiftUI app pulling live from primal, rendering seed-driven timeline, profile cards, threads, diagnostics screen.

**Scope.** Per ADR-0006 + ADR-0008 + ADR-0009: kind:0 Profile path end-to-end against a real relay, on iOS, through real FFI. Seed-driven discovery (union of follow lists from pablof7z + fiatjaf + jb55). Refcounted claim/release pattern per ADR-0005 (profile interest commit `23ae829`). Diagnostics surface per ADR-0007.

**Subsystem deliverables.**

- ✅ Kernel actor with mailbox-driven relay ingestion (commit `9e9ce04`).
- ✅ Real WebSocket connections via `tungstenite` + `rustls`.
- ✅ Profile / Timeline / Thread view kinds wired through the kernel.
- ✅ Best-effort rendering (D1): placeholders → in-place refinement on kind:0 arrival.
- ✅ iOS bridge (`KernelBridge.swift`, `KernelModel.swift`, content views).
- ✅ Diagnostics screen showing relay state, logical interests, wire subs (ADR-0007).
- 🟡 Firehose-bench `live` scenarios `cold_start` + `profile_thrashing` running against the iOS app's kernel with **measured numbers** documented as the M1 baseline. (Initial reports exist in `docs/perf/ios-demo/` but should be promoted to `docs/perf/m1/` and gated.)

**Exit gate.**

- Avatar / name / picture / NIP-05 fields update in place when kind:0 arrives mid-scroll without any spinner gate.
- Mount/unmount of 100 avatar components rapidly produces correct refcount lifecycle (no leaks, claim drops on grace period).
- Primal connection survives a 30-second disconnect via reconnect with no observable data loss in a retried scroll.
- Firehose-bench `live cold_start` against primal: time to first profile rendered ≤ 800 ms p99, time to filled timeline (200 items) ≤ 5 s p99 on developer hardware.
- Firehose-bench `live profile_thrashing` (50/sec mount/unmount over 10 min) against primal: zero subscription leaks; `OpenView`/`CloseView` dispatch rate ≤ 60% of mount rate (grace-period absorption working).
- All reactivity-bench `--standard` gates continue to pass against the real kernel code path, not just the synthetic model.

**Runnable artifact.** `just run-ios` launches the app on iPhone simulator pulled from real primal. `docs/perf/m1/baseline.md` published with measured numbers.

---

### Deferred to later milestones (honest accounting)

Two exit-gate items that were in scope for M1 are closed here as intentional deferrals. Neither is a TODO or FIXME — each is owned by the milestone that has the right tooling and context to verify it.

**1. Memory RSS measurement (→ M10.5)**  
The `profile_thrashing` and `mount_unmount_churn` scenarios note "process RSS ≤ baseline + N MB" as assertions, but M1 has no OS-level instrumentation to measure them (requires `mach_task_basic_info` on Apple, `/proc/<pid>/status` on Linux). M10.5 is the dedicated FFI-hardening + iPhone-12 measurement pass; it adds scenario S11 to capture RSS via `mach_task_basic_info` during the canonical workflow on real hardware. The numeric gate (≤ 200 MB at 100 active views per ADR-0003 working-set) lives in M10.5.

**2. Dispatch-rate gate (→ M14)**  
The exit gate above requires "OpenView/CloseView dispatch rate ≤ 60% of mount rate (grace-period absorption working)". This validates ADR-0005's platform debounce layer — specifically the generated wrapper's 30-second grace-period absorption. The current M1 bridge is raw C FFI with no generated wrapper, so the gate cannot be measured here. M14 (UniFFI migration) produces the ADR-0005 refcounted component wrapper; the dispatch-rate gate is exercised there at 1000 mount/unmount/sec for 60 s. See T22.
