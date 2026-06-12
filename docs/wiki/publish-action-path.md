---
title: Publish Action Path
slug: publish-action-path
topic: publish-action
summary: The PublishNote action variant is deleted from the kernel; the generic PublishRaw (taking kind, tags, content, target) is the only unsigned publish path needed
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:7f143c67-6e46-424a-90a8-5bf844947fee
  - session:d8869714-0ee5-4fe3-94db-1efd068b1c58
  - session:b4fe9cec-eb86-47f7-bc1d-3c28a18d5fcf
---

# Publish Action Path

## Publish Actions

dispatch_action is only for NIP-specific write operations that the kernel doesn't know about in advance (e.g. send_dm, send_zap, react, follow, publish_note), where NIP adapter crates register ActionModule implementations at composition time. The PublishNote action variant is deleted from the kernel; the generic PublishRaw (taking kind, tags, content, target) is the only unsigned publish path needed. Kind:0 and kind:3 are gated to PublishProfile and blocked from PublishRaw. The PublishProfile action variant is retained for now.

<!-- citations: [^7f143-3] [^b4fe9-5] -->
## Threading

reply_tags_for_parent and kick_thread_hydration are deleted from the kernel; the host constructs NIP-10 tags itself using nmp-nip01's builder and passes them to PublishRaw. NIP-10 threading is kind-agnostic — it applies to any event kind, not just kind:1. The reply tag builder's input type must be named EventRecord (not NoteRecord) because NIP-10 threading is kind-agnostic, not kind:1-specific. <!-- [^7f143-4] -->

## Signing and Timestamps

Apps never touch signing keys or build/sign events directly; they dispatch actions and the kernel handles the sign-and-publish pipeline internally. Only the actor thread signs events (D4); the FFI thread never touches private keys. The kernel owns the clock (D7) — it stamps created_at at signing time for unsigned publish actions. Publish commands (`PublishUnsignedEvent`, `PublishUnsignedEventToRelays`) accept `signer_pubkey: Option<String>`; `None` uses the active account (preserving all existing behavior), `Some(pubkey)` signs with the specified registered signer.

<!-- citations: [^7f143-5] [^d8869-2] -->
## Runtime Constraints

The FFI thread never blocks (D8) — execute_action is a channel send only. Every failure in the publish path surfaces as an error object or a terminal stage — never a crash (D6). The host acks terminal publish stages via nmp_app_ack_action_stage(correlation_id) to clear the entry from the action projection. <!-- [^7f143-6] -->
