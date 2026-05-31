---
title: NIP-77 Full-Reseed & Negentropy Deserialization Gap
slug: nip-77-reseeds
summary: "NIP-77 resume always performs full-reseeds at reconciler.rs:146-151 because negentropy 0.5 exposes no public deserializer."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-30
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:2c4adc99-0b1b-430c-8594-834da3ab4cef
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:c3f757f1-6292-4e52-b520-5bb52e7de2bf
---

# NIP-77 Full-Reseed & Negentropy Deserialization Gap

## Reconciliation Behavior

NIP-77 resume always performs full-reseeds at reconciler.rs:146-151 because negentropy 0.5 exposes no public deserializer. The canonicalise closure is caller-supplied at planner_gate.rs:70, creating a divergence risk. The negentropy protocol crate (nmp-nip77, 2,298 LOC) is deleted as dead code with zero shipping callers; DomainModule::ingest_kinds is likewise deleted as dormant with zero call sites. Only the coverage hook seam remains in the codebase.

V-71 tracks that the nip65_resolver module doc claims tracing that the code never performs (false documentation). [^cd2b6-12]

Section §13 must include a warning note where it references code blocks that no longer exist in nmp-nip77 (run_sync.rs, capability_domain.rs). [^c3f75-8]

<!-- citations: [^57528-10] [^57528-11] [^2c4ad-6] -->

## PlanCoverageHook & D2 Pipeline

PlanCoverageHook runs after the M2 compiler resolves relay routing but before plan_diff emits WireFrame::Req, allowing a negentropy implementation to rewrite per_relay pairs based on reconciliation. The D2 ordering (negentropy before REQ) is not enforced in production; the kernel never calls set_coverage_hook and all plans flow straight to raw REQs. [^2c4ad-7]

## NIP-77 Probe State

Each relay carries a nip77_probe_state string (unknown, probing, supported, unsupported) projected into RelayStatus::nip77_negentropy and surfaced in the kernel snapshot. set_nip77_probe_state() exists on Kernel for driving probe state from the shell's CapabilityCache but is not yet wired in production. [^2c4ad-8]
## See Also

