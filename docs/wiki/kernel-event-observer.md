---
title: KernelEventObserver — Event-Driven View Extension Seam
slug: kernel-event-observer
summary: "KernelEventObserver (actor/commands/event_observer.rs:189) is the actual v1 mechanism for event-driven views and must be documented as an extension seam"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-21
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:c3f757f1-6292-4e52-b520-5bb52e7de2bf
  - session:4eb4e0e2-a9b3-4347-a92b-a073af7adfc0
  - session:4f37753c-0654-4478-9c19-e799f1b10d39
---

# KernelEventObserver — Event-Driven View Extension Seam

## KernelEventObserver

KernelEventObserver (actor/commands/event_observer.rs:189) is the actual v1 mechanism for event-driven views and must be documented as an extension seam. It exposes a single method, on_kernel_event. [^c3f75-12]



Fine-grained per-event reactions use nmp_app_register_event_observer(). [^4f377-12]
## KernelEvent Struct Reference

The KernelEvent struct fields are listed in one consolidated reference location. [^4eb4e-1]
## See Also

