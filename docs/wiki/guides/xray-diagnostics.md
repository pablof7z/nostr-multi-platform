---
title: X-Ray Diagnostic Tool
slug: xray-diagnostics
topic: dx-proof
summary: X-Ray is a developer diagnostic tool that answers questions like "why is my feed empty?" or "why did this subscription close?" using recorded receipts
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-03
updated: 2026-07-03
verified: 2026-07-03
compiled-from: conversation
sources:
  - session:b46b47eb-a058-4f19-9451-13531c02c3bb
---

# X-Ray Diagnostic Tool

## Purpose

X-Ray is a developer diagnostic tool that answers questions like "why is my feed empty?" or "why did this subscription close?" using recorded receipts. The Chirp X-Ray pane (chirp#30) is the flagship consumer UI of X-Ray diagnostics. <!-- [^b46b4-9bd0f] -->

## Phased Roadmap

X-Ray is delivered in five phases:

- **Phase A (ADR + rules):** Blesses `nmp-devtools` as a dev-only sidecar crate and updates doctrine-lint.
- **Phase B (receipt stream):** Ordered open/close/refresh events with an NMP-owned data shape.
- **Phase C (agent-facing CLI):** A headless runner plus MCP prover so an agent can debug without any UI.
- **Phase D (Chirp pane):** The visual X-Ray panel in the app.
- **Phase E (time travel):** Records and scrubs reconciliation transaction-by-transaction. <!-- [^b46b4-84d38] -->

## Remaining Work Order

The remaining X-Ray work proceeds in order: wire the live reconciler into the receipt stream (Phase B), then agent-facing CLI/MCP (Phase C), then the Chirp pane (Phase D), then time travel (Phase E). Time travel is a committed deliverable, not a stretch goal. <!-- [^b46b4-33d80] -->
