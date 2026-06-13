---
title: Zap Scope
slug: zap-scope
topic: zap-scope
summary: Zap work is declared post-v1 by owner decision; issues #1008, #999, and #967 are deferred to post-v1 and their needs-decision labels should be dropped
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# Zap Scope

## Scope

Zap work is declared post-v1 by owner decision; issues #1008, #999, and #967 are deferred to post-v1 and their needs-decision labels should be dropped. Issue #980 proceeds with F-CR-02 but keeps status:blocked; its stale needs-decision label should be dropped.

The bolt11 invoice amount is validated against the requested amount before auto-pay, using the in-crate amount_msats parser. This fix (point 1) has landed on master as PR #1189.

Tri-state payment records (PaySent, Succeeded, Failed, Unknown) are persisted before the 23194 frame, and TTL sweeps transition to Unknown never Failed. (Previously: unknown payment outcomes were reported as failed, enabling double-pay, funds-in-flight state was memory-only, NWC lacked lookup_invoice making reconciliation unimplementable, and the fetched bolt11 amount was never validated before auto-pay.)

NwcMethod::LookupInvoice is added for post-reconnect/startup reconciliation of Unknown payments.

The preimage (proof-of-payment) is retained in the payment record rather than discarded.

No saga coordinator is built for zaps; the tri-state payment record with lookup reconciliation is the complete fix.

Linear ActionTicket (P3 step 3.1) is #[must_use] with a Drop bomb recording Failed{dropped}, replacing the ~15 hand-patch sites that manually ensure terminal stages.

Spawn-at-start (P3 step 3.2) makes late config inexpressible by removing setters after start; the actor blocks on command_rx.recv() before Start, and config rides in the Start message as data.

NIP-57 zap receipt `amount_from_embedded_request` uses `?` operator inside a for-loop over tags, causing a non-array tag element (e.g., null) to short-circuit the entire function to None, suppressing the amount-mismatch forgery guard and allowing a hostile relay to inject a forged sender pubkey. A secondary `?` short-circuit bug exists in the zap receipt decode: `arr.first()?.as_str()?` and `arr.get(1)?.as_str()?` exit the function on non-string tag elements, suppressing amount recovery for tags like `["amount", 1000]` or `[1, "amount", "1000"]`.

NIP-57 `description_hash` collects the entire relay-supplied invoice into an uncapped `Vec<Fe32>`, allowing large allocations on the ingest path.

The zap-receipt forgery fix replaces all three `?` operators inside the `for t in tags` loop with `let Some(x) = ... else { continue }`, so malformed tag arrays in zap receipts are skipped rather than propagated as `return None`, preventing a hostile relay from forging the sender and amount by bypassing the `description_contradicted` guard.

The `description_hash` DoS fix adds an early-return size guard: if `trimmed.len() > 8192`, return `None` immediately before allocating.

<!-- citations: [^da6b1-40] [^2e544-39] [^2e544-40] [^2e544-41] [^2e544-42] [^2e544-43] [^2e544-44] [^02745-20] [^02745-49] [^2e544-71] [^02745-94] -->
