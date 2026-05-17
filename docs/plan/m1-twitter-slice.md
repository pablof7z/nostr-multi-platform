# M1 — Read-only Twitter slice on iOS *(LARGELY DONE; final hardening in flight)*

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
