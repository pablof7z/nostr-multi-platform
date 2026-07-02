---
title: Action Registration and Dispatch
slug: action-dispatch
topic: action-dispatch
summary: Registering an action means implementing one trait; the framework owns dispatch
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
---

# Action Registration and Dispatch

## Action Dispatch

Registering an action means implementing one trait; the framework owns dispatch. The FFI-free dispatch core lives in `nmp-native-runtime::action_dispatch` as `dispatch_action_bytes_typed`, returning a typed `DispatchOutcome`; both C-ABI and UniFFI consume the same implementation. `DispatchOutcome` carries `{correlation_id, error, code}`; coded rejections are load-bearing in tests. `ActionRegistry` has no drain/clear gate for the result observer (it holds a mutex across the call); this gap is tracked as #2429 and must be resolved before M14-D deletes the C-ABI.

App actions cross the FFI boundary as a FlatBuffers `DispatchEnvelope` through `dispatch_action` — the single command lane (ADR-0064), identical in shape across native and browser (wasm); the JSON doorway was deleted. `nmp-core` receives the envelope and dispatches to the per-NIP adapter that owns that action. `ActionModule::execute()` is the sole write path (ADR-0027); `ActionModule` is the public trait in `nmp-core` that implements `execute()` for a per-NIP action adapter, the only path through which an app's typed action enters the kernel. `ActionRegistrar::register_action` is the public composition-time seam where an app registers a custom `ActionModule` for its own kind at its composition root.

<!-- citations: [^3c942-d77aa] [^3c942-3b05e] [^898a4-3312e] -->
