---
title: FixedClock Test Seam for Deterministic Timestamp Tests
slug: fixed-clock-test-seam
topic: test-seams
summary: "The flaky test `auto_arm_finalizes_before_parking_remote_sign` (#2962) is caused by a real wall-clock race: two live `kernel.now_secs()` reads straddle a second"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
---

# FixedClock Test Seam for Deterministic Timestamp Tests

## Root cause: wall-clock race in publish tests

The flaky test `auto_arm_finalizes_before_parking_remote_sign` (#2962) is caused by a real wall-clock race: two live `kernel.now_secs()` reads straddle a second boundary because the kernel default clock is `SystemClock`. The identical timestamp-race bug in the sibling test `explicit_arm_finalizes_before_parking_remote_sign` (`publish_unsigned_to.rs:260`) is fixed in the same PR as #2962. <!-- [^d8bc6-4e31a] -->

## Fix: pin the kernel clock with FixedClock

Flaky publish tests are fixed by pinning the kernel clock via the existing `Kernel::set_clock` + `FixedClock` test seam before driving each publish, rather than using sleeps, retries, or weakened assertions. <!-- [^d8bc6-6f618] -->

The `FixedClock` test seam pattern (`Kernel::set_clock`) is the established convention for deterministic timestamp tests in the codebase, already used in `kernel_reducer/command_apply_contacts_tests.rs` and `command_apply_publish_timestamp_tests.rs`. <!-- [^d8bc6-6f618] -->
