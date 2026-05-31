---
title: NMP Codegen & Uniform Action Surface
slug: nmp-codegen-action-surface
summary: Codegen wiring of protocol crates into `nmp-codegen` is deferred because no existing protocol crate yet exposes the `Action`/`Update`/`ViewSpec` aggregate enums
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-31
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:590ca0cd-3665-42f5-96ab-3ea035a79d67
  - session:423f3c56-7275-4e62-998e-e8f37be564da
  - session:12b3f443-3c2d-4e47-976a-7f4ceab75343
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
  - session:2c4adc99-0b1b-430c-8594-834da3ab4cef
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:752b523f-231e-4fca-ab86-748c35b5dd74
  - session:c3f757f1-6292-4e52-b520-5bb52e7de2bf
  - session:c9ae5a7c-0f5e-44ec-94d6-d9b5e31d8991
---

# NMP Codegen & Uniform Action Surface

## Codegen Wiring Deferment

Codegen wiring of protocol crates into `nmp-codegen` is deferred because no existing protocol crate yet exposes the `Action`/`Update`/`ViewSpec` aggregate enums that the codegen template references; this is a uniform-action-surface design question across all protocol crates. No FFI work is needed for new ViewModules because nmp gen modules (ADR-0010) auto-picks them up. A CI codegen drift gate (F-05 scope) is needed to enforce freshness of the `dispatch_actions.md` catalog, which is at risk of silently becoming stale whenever a new `ActionModule` is registered. D0 type-level layering has zero violations; remaining debt is wire-schema namespace casing, UX feedback round-trips, and bunker parity. Swift Codable struct emission is added to nmp-codegen (piloted with ActionOutcome). The Codegen Swift Projections Registry is a static compile-time registry mapping JSON snapshot keys to Swift property names and types for codegen, replacing hand-written Swift `SnapshotProjections`; the codegen for Swift types lives at `crates/nmp-codegen/src/swift.rs`, reads a `ProjectionSchemaDocument` from `nmp-core::codegen_schema`, and uses the `SNAPSHOT_PROJECTIONS` static in `crates/nmp-codegen/src/swift_projections_registry.rs` to control conformances like `Equatable` and `Identifiable`. Host app crates wire their own action modules and snapshot projections into core registries at startup via explicit registration functions. Namespace casing must follow the `nmp.*` convention; `nip29.*` breaks it at `nmp-nip29/src/action/{content,composed}.rs:50,47,90`. Action namespaces follow the `nmp.nip17.*` convention rather than `nmp.dm.*`. A CI rule must fail if any iOS view references a namespace string with no registered ActionModule (D9 doctrine-lint rule enforces the `nmp.` prefix with 14 unit tests and fixtures). Shipped-but-inert UI (Zap, ArticleDetailView, group discovery) must be fixed with a doctrine-lint requiring iOS namespace strings to have a registered ActionModule. Large refactors exceeding one agent session must be staged (e.g., Stage 1 trait extension → Stage 2A-F per-crate migration → Stage 3 deletion) rather than attempted as a single PR. The protocol module recipe (§20) must use the real `register_actions()` pattern and must not reference `register_domain`, `register_view`, or `ModuleRegistry`.

<!-- citations: [^47203-4] [^1c093-13] [^1c093-14] [^590ca-1] [^423f3-7] [^12b3f-7] [^1c093-12] [^47203-3] [^2c4ad-11] [^54ae9-13] [^c3f75-10] [^c9ae5-23] -->
## One-Door Rule & FFI Moratorium

Only ~7 of 38 ActorCommands use `dispatch_action`; there is a moratorium on new bespoke `nmp_app_*` FFI. `nmp_app_publish_signed_event` and `nmp_app_publish_unsigned_event` are both event-producing bespoke FFI symbols that must be routed through `dispatch_action` (`nmp_app_publish_unsigned_event` at `ffi/identity.rs:122-135` is included in one-door cleanup). D11 lint enforces the one-door rule: bespoke `nmp_app_*` FFI building `ActorCommand::PublishSignedEvent`/etc. fails. The discriminator is: generic user/app-authored publish-engine events go through `dispatch_action`; lifecycle/view/control/session events stay bespoke. `PublishAction::Cancel` split control plane is the canonical correct shape; lifecycle stays on a dedicated symbol. Seam asymmetry migration must not move `WalletConnect/Cancel/timeline-open` to `dispatch_action`; they are lifecycle. `PublishSignedEvent` FFI bypasses validation at `ffi/identity.rs` by skipping `PublishModule::start`.

Action registration uses a single typed seam `app.register_action::<M>()` rather than a dual `register_action_module` + `register_action_executor` pair. ADR-0027 (unified ActionModule trait) is fully implemented in master: ActionModule has a typed `execute()` used by ~17 impls across all NIP crates, and the dual `register_action_executor` seam is deleted. Each `ActionModule` implementation provides a typed `type Action` and `fn execute()` method. The `default_registry()` function collapses to a single typed call `registry.register::<PublishModule>()` per module. The `wire_action!` macro is reduced to a one-liner that could be inlined rather than remaining as a multi-seam registration macro. The `nmp_app_register_action_executor` and `nmp_app_register_action_module` C-ABI symbols are deleted from the codebase. The `ClosureModule` adapter is deleted from the codebase. Test ActionModule structs use typed `execute()` methods migrated from closure-based seams via `OnceLock<TodoStore>` where needed.

<!-- citations: [^1c093-15] [^47203-5] [^752b5-7] -->
## Action Stage Tracking & UX Feedback

Spinner/terminal-result UX across 4 surfaces requires a single `PendingActionTracker` plus a uniform correlation_id contract. `ActorCommand::PublishSignedEvent` now carries `correlation_id`, the registry's `Publish` arm threads the minted id, Swift `DispatchResult` enum parses the envelope, and `KernelModel` tracks `pendingActions: Set<String>` synchronously. Pre-signed publishes never close their spinner because `correlation_id_override` is only set for `PublishNote`, not `PublishAction::Publish` at `publish/engine.rs:265-277`; PR-A's audit pointed one layer too low for the spinner fix, the actual asymmetry was the registry's `Publish` arm dropping the id (the engine was already symmetric, so the fix landed at the actual asymmetry in the registry). `ShowToast` error path loses correlation_id at `ffi/identity.rs` ~lines 161/187/293; the host has no way to correlate. `projections["action_stages"]` is keyed by correlation_id with stages `requested | awaiting_capability | publishing | accepted | failed`, and the actor writes on every transition. `action_stages` terminal must persist until the host acks via `nmp_app_ack_action_stage(correlation_id)`, otherwise iOS misses the transition window. `action_stages` retention uses ack-based semantics: the host calls `nmp_app_ack_action_stage(correlation_id)` to drop a stage. D12 lint enforces action stage continuity: async executors must call `record_action_stage(id, ...)`. D12 needs a registration marker (e.g. `ActionModule::is_async() -> bool`) instead of grep-detecting send patterns. [^1c093-16]
## See Also

