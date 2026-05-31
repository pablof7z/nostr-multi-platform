---
title: Marmot Welcome Ingestion & Tap Observer
slug: marmot-welcome-ingestion
summary: "Kind:1059 gift-wrap Welcomes arrive automatically via the kernel's relay subscription and tap observer, not via Swift-side polling"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-29
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Marmot Welcome Ingestion & Tap Observer

## Welcome Ingestion

Kind:1059 gift-wrap Welcomes arrive automatically via the kernel's relay subscription and tap observer, not via Swift-side polling. Upon sign-in, the kernel sends a tailing REQ subscription for `{"kinds":[1059],"#p":["<user_pubkey>"]}` to the user's NIP-65 inbox relays so gift-wrapped Welcomes arrive event-driven. The `pollInbox` function and 'Poll Inbox' button are only a developer diagnostic escape hatch, never part of normal UX. The key-package fetch was previously mis-wired through a dead OpenView kernel stub, causing group invites to silently fail for any peer not already in the in-process cache; it is now correctly routed through the proven `push_interest` pattern, consistent with the working welcome/group-message legs. The D10 lint enforces provenance for gift-wrapped events: they must route only through `recipient_dm_relays` or `PublishTarget::Explicit`. Marmot's empty-relay auto-fallback complicates D10 provenance because gift-wraps could leak to the general outbox, so the guard must be in `publish_signed_event` itself, not just caller-side. The NIP-59 tag preservation test at `preservation.rs:58` claims to cover the Marmot Welcome shape but uses `p`/`subject`/`relays` tags with no `e` tag.

<!-- citations: [^fe79b-7] [^1c093-9] [^4edd4-13] -->
## See Also

