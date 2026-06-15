---
title: Instance-Scoped Registration
slug: instance-scoped-registration
topic: nmp-app-integration
summary: "Instance-scoped module registration replaces type-based register_action::<M>() with instance-scoped register_action(&mut self, module: M), where extension modul"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Instance-Scoped Registration

## Instance-Scoped Registration

Instance-scoped module registration replaces type-based register_action::<M>() with instance-scoped register_action(&mut self, module: M), where extension modules are instance-scoped values (not types) with &self methods on start/execute. ActionModule's stateless-by-construction trait (static start/execute with no &self) forces process-global module state via OnceLock, creating an instance-scoped vs process-scoped mismatch. ProtocolCommand::run(&self, ...) carries its dependencies captured at composition time, deleting the OnceLock globals and restoring multi-instance correctness. Instance-scoped registration replaces OnceLock globals and enables multi-app correctness. K2 (instance-scoped registration) through the global-hook slots has the highest deletion-per-effort ratio in the program; it discharges P5 wholesale, enables deleting kernel_mut(), merges the duplicate write seams, makes the previously impossible two-instance interop test pass, and enables the D20 lint and kernel_mut deletion. Per-app slots, created in nmp_app_new, replace the five process-global hook/driver singletons (GLOBAL_BROKER, bunker_hook, external_signer_hook, GLOBAL_DRIVER, and the nmp-core/external_signer_hook.rs HOOK); all five are deleted. DispatchHostOp is merged into the Protocol seam (preserving its panic-isolation and persistent-handler semantics); the single open write seam is Protocol. K2 rung 5.5 deletes ProtocolCommandContext::kernel_mut(); genuine kernel services are promoted to narrow capability traits, and lnurl_for_pubkey is relocated off the generic context to a capability the zap command carries. The D21 no-ambient-authority doctrine-lint bans OnceLock/lazy_static/static Mutex/RwLock/AtomicPtr holding non-const state in production nmp-* crates, with a justification-required allowlist that burns to zero. Two NmpApp instances in one process pass an interop test with separate wallets, bunker sessions, and signer ports. ActionTicket is a #[must_use] linear type whose Drop records Failed{dropped}; the ~15 hand-patch sites and their broken-promise-fix comments are deleted. The three correlation-id regimes are collapsed into one (the ticket IS the identity; event_id becomes payload). Spawn-at-start: nmp_app_new returns a passive handle; nmp_app_start moves config into the spawned actor, deleting the preflight kernel, the #601 rev hack, and the first-command-of-any-type trap. Action feedback is collapsed to a single mechanism (action_lifecycle) with TTL-anchored retention; ack is early-dismiss only; the action_results drain and action_stages ack-mirror are deleted. K2 ADR-0052 is merged as PR #1323; register-by-value + ACTIVE_WALLET_RUNTIME deletion as PR #1326; per-app bunker/NIP-55 ports + four hook/driver globals deleted as PR #1344.

<!-- citations: [^2e544-20] [^2e544-21] [^2e544-22] [^2e544-23] [^2e544-24] [^2e544-25] [^2e544-26] [^2e544-27] [^2e544-28] [^2e544-60] [^2e544-350] [^2e544-370] [^2e544-392] [^2e544-465] [^2e544-482] -->
