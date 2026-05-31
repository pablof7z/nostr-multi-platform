---
title: Violation Fix & WASM Sequencing Policy
slug: violation-fix-wasm-sequencing
summary: Architecture violations are fixed and landed first, then WASM work proceeds
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-23
updated: 2026-05-29
verified: 2026-05-23
compiled-from: conversation
sources:
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:594b7c34-efd1-4461-81ad-9fa33a6e76f9
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# Violation Fix & WASM Sequencing Policy

## Violation Fix and WASM Sequencing

The continuous architectural cleanup loop runs until codex agrees that the architecture is in very good standing. Architecture violations are fixed and landed first, then WASM work proceeds. WASM (F-01) is deferred to post-v1; v1 targets iOS, macOS, and Android only. V6 codegen stages 2–3 run in parallel with violation fixes, not after them. The stateful spike (NIP-01 publish + NIP-46 signin with ≤300 LOC Swift and zero new bespoke FFI symbols) waits until all violations land. V-57 P4 documents five concrete WASM publish-path gaps: AppAction variants not wired (publish_path.rs:57), NIP-46 bunker async transport missing (publish_path.rs:81), no native ActorCommand equivalent on WASM (runtime.rs:32), unrecognised signer kinds (signer_slot.rs:32), and zero wasm-bindgen-test coverage (lib.rs:200). WASM public errors are exposed as Result<JsValue, JsValue> rather than the native error contract. No wasm-bindgen-test harness is set up, leaving publish-path and signer-slot dispatch with zero automated coverage on wasm32. V-68 stage 1 removes kind:1/6 social-timeline policy from the planner constructor and the inert mailbox trace; the 2 live author/thread sites require an FFI/ABI change and are deferred to a documented stage 2. Findings that fold into existing violations, are intentional scoping decisions, or are doctrined or dev-tool acceptable fallbacks are classified and skipped rather than tracked as new violations. Dead-code stubs with zero callers are deleted outright rather than added to the backlog.

<!-- citations: [^1670f-20] [^f2605-15] [^cd2b6-22] [^594b7-8] [^42908-25] -->
## See Also

