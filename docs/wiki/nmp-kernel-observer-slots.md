---
title: NMP Kernel Observer Slots
slug: nmp-kernel-observer-slots
summary: The KernelEventObserverSlot on Kernel is the chosen mechanism for per-app projection fan-out, implemented as a sibling of the existing LifecycleObserverSlot.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-30
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:423f3c56-7275-4e62-998e-e8f37be564da
  - session:d27a4f61-511b-4086-845d-335493f9b464
  - session:12b3f443-3c2d-4e47-976a-7f4ceab75343
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:855be2a2-4866-4d8d-ad4f-145309da56bc
  - session:c3f757f1-6292-4e52-b520-5bb52e7de2bf
---

# NMP Kernel Observer Slots

## KernelEventObserverSlot

The KernelEventObserverSlot on Kernel is the chosen mechanism for per-app projection fan-out, implemented as a sibling of the existing LifecycleObserverSlot. The KernelEventObserver trait must be documented with its single on_kernel_event method as the mechanism for event-driven views in v1. Fine-grained per-event reactions use `nmp_app_register_event_observer()` which fires per-ingested event. Registered event observers receive all events matching any active interest — filtering per-observer is the observer's responsibility, not the kernel's. The raw-event observer tap is a separate additive observer that does not mutate KernelEvent or touch M2 subs/projection, providing full signed nostr::Event data that MDK requires. The `register_raw_event_observer` function is an escape-hatch that bypasses framework guarantees (D1, D3, D5, D8) and must be documented as such. The `NmpSnapshotProjector` callback type is `unsafe extern "C" fn() -> *const c_char` — it takes zero arguments and has no access to kernel state, which blocks non-Chirp apps from reading kernel state through the snapshot seam. Only `nmp_app_chirp_snapshot` exists as a snapshot pull function — there is no generic `nmp_app_get_snapshot` for non-Chirp apps. run_publish_engine does not pre-store events in the kernel store; they are only stored upon relay echo, which causes published kind:3 and kind:10002 events to notify observers when echoed back.

<!-- citations: [^423f3-8] [^d27a4-8] [^12b3f-14] [^4f377-17] [^1670f-9] [^855be-6] [^c3f75-11] -->
## See Also

