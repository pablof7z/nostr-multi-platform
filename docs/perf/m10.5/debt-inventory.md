# FFI & iOS Bridge Debt Inventory (M10.5 Audit)

**Audit Date:** 2026-05-18
**Scope:** Rust FFI boundary + iOS bridge code paths
**Files Scanned:** 19 Rust modules + 9 Swift files (5,184 LOC Rust, 1,375 LOC Swift)
**Exit Criteria:** All findings classified; doctrine violations identified and severity-ranked

---

## 1. Summary Table

| File | TODOs | FIXMEs | Panics | Unwraps | Unsafe Unguarded | Casts | Allow(dead_code) | Expects | Notes |
|------|-------|--------|--------|---------|------------------|-------|------------------|---------|-------|
| ffi.rs | 0 | 0 | 0 | 0 | 3 | 2 | 0 | 0 | FFI boundary; 3 unsafe blocks without safety comments |
| actor.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| relay.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| relay_worker.rs | 0 | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 1 allow(unreachable_patterns) |
| app.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| lib.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| kernel/mod.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| kernel/nostr.rs | 0 | 0 | 0 | 0 | 0 | 2 | 0 | 0 | 2 casts (f64 ratio) |
| kernel/status.rs | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 2 | 1 cast; 2 expect() with clear messages |
| kernel/requests.rs | 0 | 0 | 0 | 0 | 0 | 1 | 0 | 0 | 1 cast (u64 saturation) |
| kernel/ingest.rs | 0 | 0 | 0 | 0 | 0 | 2 | 0 | 0 | 2 casts (u64 saturation) |
| kernel/update.rs | 0 | 0 | 0 | 0 | 0 | 5 | 0 | 0 | 5 casts (count→u64/u32) |
| kernel/tests.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ tests only |
| substrate/mod.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| substrate/view.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| substrate/action.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| substrate/capability.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| substrate/domain.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| substrate/identity.rs | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ clean |
| **iOS (Swift)** | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | ✓ all 9 Swift files clean |
| **TOTALS** | **0** | **0** | **0** | **0** | **3** | **14** | **1** | **2** | **20 items; 0 bugs** |

---

## 2. Findings Summary

All 20 items classified. 0 bugs. 4 must-fix documentation items (F1–F4).
14 integer casts (F5–F14) justified by design bounds. 2 expect() calls
(F15–F16) justified by construction invariant. Full findings with code
snippets and recommendations in
[`debt-inventory-findings.md`](./debt-inventory-findings.md).

| ID | Location | Severity | Action |
|----|----------|----------|--------|
| F1 | ffi.rs:75 | Medium | Must-fix: add SAFETY comment |
| F2 | ffi.rs:275 | Medium | Must-fix: add SAFETY comment |
| F3 | ffi.rs:284 | Low | Must-fix: add SAFETY comment |
| F4 | relay_worker.rs:242 | Low | Must-fix: document defensive pattern |
| F5 | kernel/status.rs:117 | Low | Justified: explicit saturation |
| F6–F10 | kernel/update.rs, nostr.rs | Low | Justified: bounded counts |
| F11–F12 | kernel/ingest.rs, requests.rs | Low | Justified: saturation arithmetic |
| F13–F14 | ffi.rs:94,296 | Low | Justified: standard FFI coercion |
| F15–F16 | kernel/status.rs:225,231 | Low | Justified: construction invariant |

Doctrine audits (D0–D5) in
[`debt-inventory-findings.md`](./debt-inventory-findings.md). All six
doctrines pass.

---

## 3. Justified Findings (No code change, no auditability gap)

| Finding | Classification | Justification |
|---------|---|---|
| 14 integer casts (F6–F14) | Justified | All bounded by design constraints; no overflow risk. |
| 2 expect() calls (F15, F16) | Justified | Invariant maintained by construction in `Kernel::new()`. |
| ProfileCard.placeholder (D1) | Design compliance | Correct D1 implementation; refinement in place. |
| Error strings in JSON (D3) | Design compliance | D3-compliant: errors as advisory data, not FFI codes. |

---

## 4. Must-Fix Items for M10.5 (mandatory)

Per the M10.5 zero-shortcut posture, the following are **required cleanup**
before the M10.5 empirical exit gate:

1. **Add SAFETY comments to ffi.rs unsafe blocks** (F1, F2, F3) —
   `crates/nmp-core/src/ffi.rs` lines 75, 275, 284. Missing safety
   comments are auditability debt and violate the M10.5 zero-debt
   posture. Suggested text in `debt-inventory-findings.md`.
2. **Add clarifying comment to relay_worker.rs:242** (F4) — explain
   why `#[allow(unreachable_patterns)]` is defensive against future
   tungstenite enum evolution.

These must land in a single PR titled
`m10.5(ffi): add SAFETY comments + clarify defensive pattern`.

---

## 5. Metrics

| Category | Count | Status |
|----------|-------|--------|
| Rust files audited | 19 | ✓ |
| Swift files audited | 9 | ✓ |
| Total LOC scanned | 6,559 | ✓ |
| Blocking debt markers | 0 | ✓ Clean |
| Code bugs found | 0 | ✓ No bugs |
| Unsafe blocks without safety comments | 3 | Must-fix for M10.5 |
| Integer casts | 14 | ✓ Justified |
| Cardinal doctrine violations | 0 | ✓ Compliant |

---

## 6. Conclusion

The FFI and iOS bridge code is **clean of blocking debt markers**
(TODO/FIXME/unimplemented/panic). All 20 findings are either justified
by design or actionable as documentation cleanup. All cardinal doctrines
(D0–D5) are upheld.

**This audit is necessary but not sufficient for M10.5 exit.** M10.5 exit
requires (per [`docs/plan/m10.5-ffi-hardening.md`](../../plan/m10.5-ffi-hardening.md)):
- Stress harness scenarios passing on simulator and iPhone 12
- All M1–M10 perf reports re-run with no > 5 % regression
- Instruments-recorded zero leaks over 10-min canonical workflow
- UI-scripted scenarios passing (Sonnet-agent + XCUITest)
- Doctrine review signed in writing in `docs/perf/m10.5/doctrine-review.md`

**Audit status:** ✅ static-debt baseline established. The 4 must-fix
items above are tracked.
**M10.5 exit status:** ❌ pending empirical proof + must-fix cleanup.
