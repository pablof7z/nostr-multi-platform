---
title: Instance-Scoped Registration
slug: instance-scoped-registration
topic: nmp-app-integration
summary: Instance-scoped module registration (register_action by value with &self methods) replaces stateless-by-construction ActionModule trait, and per-app slots repla
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Instance-Scoped Registration

## Instance-Scoped Registration

Instance-scoped module registration (register_action by value with &self methods) replaces stateless-by-construction ActionModule trait, and per-app slots replace OnceLock globals. The extension one-change fix is making extension traits instance-scoped: register_action(&mut self, module: M) registers a value, ProtocolCommand::run(&self, ...) operates on instance state, deleting all OnceLock globals.

K2 (instance-scoped registration) is in flight: rung 5.1 (ADR-0052) is merged on master, and rung 5.2 (register-by-value trait change, delete ACTIVE_WALLET_RUNTIME, two-instance interop oracle) is being implemented. ADR-0052 adversarial review found a fifth ambient-authority global (nmp-core/src/external_signer_hook.rs HOOK) and confirmed DispatchHostOp ≠ Protocol (it has panic-isolation + persistent-handler semantics that must be preserved before merging). Rung 5.2 design was expanded: the WalletRuntimeHandle is also consumed by the WalletInterceptor (not just action modules), and the composition root already threads it by value into the interceptor, so deleting ACTIVE_WALLET_RUNTIME extends the same pattern. Rung 5.2 closes #619 and supersedes #1312 by construction: when the wallet module is a value owning its handle, composition IS installation and there is no separate install step to order.

K2 runs after K1 and before K3, with K3 gated behind K2.

Action_lifecycle is the sole host-facing action feedback projection, replacing action_results drain and action_stages ack-mirror.

gc_step receives a derived pinned HashSet instead of the persisted claims sub-db, deleting ClaimerId and OverPinned machinery.

ActionTicket is a linear #[must_use] type with a Drop bomb that records Failed{dropped} through the actor channel, replacing the three correlation-id regimes.

DispatchHostOp is merged into Protocol, leaving one open write seam. (Previously: DispatchHostOp ≠ Protocol — panic-isolation + persistent-handler semantics must be preserved before merging.)

kernel_mut() is deleted from ProtocolCommandContext; genuine kernel services are promoted to narrow capability traits.

The per-dispatch snapshot emit path is conflation-safe only after drain-once state (action_results, signed_events) is removed from the snapshot.

No LateWiring runtime diagnostic is built for #618 because spawn-at-start makes the failure inexpressible.

<!-- citations: [^2e544-20] [^2e544-21] [^2e544-22] [^2e544-23] [^2e544-24] [^2e544-25] [^2e544-26] [^2e544-27] [^2e544-28] [^2e544-60] -->
