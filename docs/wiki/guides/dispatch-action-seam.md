---
title: Dispatch Action Seam
slug: dispatch-action-seam
topic: ffi-runtime
summary: The dispatch_action seam is structurally vestigial â 70 C-ABI symbols exist but only 3 are routed through dispatch_action
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-06-19
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:45fcf96e-5b37-414f-a080-820b74a4e179
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Dispatch Action Seam

## Coverage and Bypasses

The dispatch_action seam is structurally vestigial — 70 C-ABI symbols exist but only 3 are routed through dispatch_action. The remaining bypasses use direct send_cmd calls. PublishSignedEvent FFI bypasses PublishModule::start validation, allowing malformed signed events to reach the actor without validation. nmp_app_publish_unsigned_event is also an event-producing FFI bypass. Marmot depends on nmp_app_publish_signed_event_to via extern C call across crate boundaries, which was replaced by the internal kernel API NmpApp::publish_signed_explicit. WalletConnect, WalletDisconnect, and WalletPayInvoice ActorCommand variants have no ActionModule implementations and are called via direct send_cmd from ffi/wallet.rs. Do not migrate WalletConnect/Disconnect/PayInvoice to dispatch_action; they are wallet session lifecycle, not event production. The Theme A discriminator is specifically 'generic user/app-authored publish-engine events' — NWC pay-invoice signs kind:23194 but is wallet lifecycle, so it stays bespoke.

V-38 (NWC wallet), V-39 (DM send), V-40 (DM ingest), V-41 (zap LNURL), and V-50 (outbox routing) are all post-v1 violations requiring the open-ActorCommand seam as a shared prerequisite. V-77 (nmp-nwc MakeInvoice) has a fully typed enum variant, params struct, result struct, and builder function but zero runtime dispatch end-to-end. The ActorCommand open seam for write-path protocol commands is ActorCommand::Protocol(Box<dyn ProtocolCommand>); NIP crates dispatch commands without the kernel knowing NIP nouns.

Theme A (One Door Per Capability): if a call produces a signed Nostr event, it goes through dispatch_action; lifecycle/capability/session calls stay on dedicated symbols.

DmInboxLookup on the universal ProtocolCommandContextParts is left as-is — it's a Noop D15 capability among ~10 on the parts struct, not a real D0 violation; a per-command capability set refactor would touch the dispatch arm with risk of overlap.

PublishAction::Cancel is hard-rejected at the validator level and routed through a separate nmp_app_cancel_publish FFI symbol, creating a split control plane where cancel terminals never appear in action_results. Hard-rejecting PublishAction::Cancel at the validator with a message pointing to nmp_app_cancel_publish is the canonical correct shape for lifecycle control on a dedicated symbol.

ShowToast error path in ffi/identity.rs loses correlation_id context when FFI-layer JSON decode fails before dispatch_action is called.

HttpCapability exists on the iOS side but has no Rust-side implementation; it was deleted from the kernel as inert. Do not add HttpCapability without its consumer in the same PR (memory #33-#39 thrashed on this).

Do not add ViewModule or IdentityModule traits; they were deliberately deleted because no registry drove them.

No new C-ABI symbols should be added; new projection fields and dispatch specs route through the existing nmp_app_dispatch_action seam and built-in snapshot projection map. A C-ABI freeze must be enforced via CI lint to prevent further direct C-ABI symbol growth outside dispatch_action. The nmp_app_register_action_executor C-ABI symbol and the register_action_executor Rust method are deleted; action registration is now a single typed app.register_action::<M>() call. The wire_action! macro is also deleted; action wiring no longer requires a paired macro to avoid the two-call footgun.

The ActionModule trait has a required fn execute(action: Self::Action, correlation_id: &str, send: &dyn Fn(ActorCommand)) -> Result<(), String> method, ensuring validator-executor symmetry is a type-level fact rather than a manual two-call contract.

ProfileAction should carry a dispatch: Option<ProfileDispatchSpec> so Swift branches on presence-of-dispatch rather than switching on action.kind, with iconName pre-computed in Rust.

The three parallel publish write paths (nmp_app_publish_signed_event, nmp_app_publish_signed_event_to, and nmp_app_dispatch_action with PublishAction::Publish) converge on a single ActorCommand::PublishSignedEvent variant, representing a HIGH-risk consolidation target.

The dead RoutingContext::explicit_targets seam versus live PublishTarget::Explicit must be tracked as a follow-up issue rather than fixed in-scope. <!-- [^11850-257] -->

<!-- citations: [^47203-5] [^1c093-4] [^1c093-5] [^1c093-6] [^1c093-7] [^1c093-8] [^1c093-9] [^45fcf-1] [^47203-4] [^95d02-5] [^1670f-6] [^cd2b6-3] [^11850-69] -->
## Action Results and Correlation

action_results is a vector (not scalar) that drains all pending terminals each tick; the scalar lastActionResult fallback is documented as deprecated. correlation_id_override is only set for PublishNote; pre-signed PublishAction::Publish uses the event id as terminal correlation_id, preventing spinner matching. PD-036 requires choosing between adding ActorCommand::RecordActionAccepted or flipping is_async_completing to false, because ZapAction success path never records Accepted ActionStage.

<!-- citations: [^1c093-10] [^95d02-6] -->
## Doctrine Lints

D10 provenance lint (PR #207) enforces that inside D10-marked functions, PublishTarget::Auto and publish_signed_event with empty relays are banned, with a doctrine-allow escape hatch requiring a reason. D11 lint was introduced alongside the deletion of nmp_app_publish_signed_event and nmp_app_publish_unsigned_event FFI symbols. kind:1059 can still Auto-route through the generic PublishAction::Publish → publish_signed_event path (empty relays → Auto) even after the NIP-17 handler guard in PR #229. PR-K3 (#239) added a defensive kernel-level guard at publish_signed_event that refuses kind:1059 with empty relays, records a failed terminal verdict under the dispatch correlation_id, and emits a D6 toast. <!-- [^1c093-11] -->

D12 doctrine-lint: any register_executor closure that sends an ActorCommand for an async-completing action must call record_action_stage(id, stage) on dispatch. D12 lint requires a registration marker (e.g., ActionModule::is_async() -> bool) rather than grep-detecting send patterns, because async-completing commands need classification to avoid false positives. <!-- [^1c093-12] -->

## Shipped PRs

PR-G (#230) shipped the action_stages substrate: projections['action_stages'] keyed by correlation_id, ack-based retention via nmp_app_ack_action_stage FFI symbol, and D12 lint. PR-G2 (#241) fixed four codex findings: iOS render-after-ACK race (DispatchQueue.main.async defer), terminal cap eviction (inverted to preserve terminals), D12 multi-line scanner bypass, and executor send-then-panic orphan via new RecordActionFailure ActorCommand. <!-- [^1c093-13] -->

## Read-Path Open Seam

The EventIngestDispatcher + IngestParser trait is the read-path open seam; NIP crates register kind-specific parsers at composition time. This asymmetry (ingest uses registry, routing does NOT) is structural. <!-- [^1670f-7] -->
