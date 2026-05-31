---
title: NMP Class Routing & Relay Selection
slug: nmp-class-routing
summary: "Event routing is kind-driven with an intent override: the kernel ships a built-in EventClass resolver mapping kinds to classes, and apps can override per-publis"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-23
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:41858cd2-3a5d-4ad1-bdd0-4cbe1df2dd9d
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:d27a4f61-511b-4086-845d-335493f9b464
  - session:50510273-d1c9-424a-b877-179d52fba557
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:1670fcb8-f275-498c-975b-8bd912331ded
---

# NMP Class Routing & Relay Selection

## Core Routing Model

Relay routing is a per-kind dispatch table, not a single NIP-65 algorithm — DMs route to kind:10050 inbox of recipient, NIP-29 groups via h-tag relay, Marmot via group relay, drafts via personal private storage relay, and kind:10002 is just the default. The router is one generic algorithm with no RoutingRule registry; NIP crates that already know their relay set (NIP-17 from DmRelayCache, NIP-29 from group state, Marmot from MLS group relay) pass it via RoutingContext::explicit_targets, and the generic algorithm is skipped entirely. There is no standalone nmp-nip65 crate; it is too thin to justify, so the ActionModule content belongs in the relay routing crate. PublishTarget::Auto is upgraded in place to be class-aware from day one; no new AutoByClass variant is needed. Interaction event kinds 7, 1111, and 9735 must be dispatched to their respective decoders in the ingest catch-all (kernel/ingest/mod.rs:259-298), and each crate's register() must be called from core init. ActorCommand::PushInterest allows any protocol crate to register a relay subscription without touching Swift code. The has_role function must not treat the "indexer" role as semantically including the "write" role. The is_discovery_kind function returns true for kind 0, kind 3, and kinds 10000 through 19999. PublishTarget::Explicit threads explicit relay targets through the signed-publish path so kind:445 group messages route to the pinned group relay and kind:1059 gift-wraps route to the recipient inbox.

<!-- citations: [^41858-1] [^d27a4-7] [^57528-13] [^50510-1] [^fe79b-9] [^1670f-6] -->
## OutboxResolver API

The OutboxResolver trait gains two methods for class routing: class_relays_personal(class) for self-keyed NIP-51 lists (Search, Draft, blocked) and class_relays_for_author(class, author) for publisher-keyed lists (Wiki). OutboxResolver::resolve accepts a kind: u32 parameter so the resolver can differentiate event routing. When a REQ specifies authors, class routing partitions per author so that each author's NIP-51 class-relay list is used for their respective kinds.

<!-- citations: [^41858-2] [^50510-2] -->
## Wiki Relay Lifecycle

Per-author kind:10102 wiki relay fetches are lazy, cached, and evicted: fetched the first time a Wiki interest names an author, kept alive while any class-routed interest references them, and dropped when the last one ends. [^41858-3]

## Blocked-Relay Filter

The planner applies the blocked-relay filter (kind:10006) as a post-processing pass that subtracts blocked relays from the final target list; blocking is non-bypassable. If the blocked-relay filter subtracts every relay from a plan, the planner fails loud with PlannerError::AllRelaysBlocked rather than silently emitting an empty plan. [^41858-4]

## NIP-51 List Mapping and Decryption

The NIP-51 fact-stream struct includes search (kind:10007), blocked (kind:10006), wiki (kind:10102), drafts as Option (kind:10013, nip44-encrypted), and dm fields. Draft relays are sourced from NIP-51 kind:10013, which is a nip44-encrypted list. Wiki relays are sourced from NIP-51 kind:10102 (Good wiki relays). EventClass::Draft maps to both kind 31234 (NIP-37 parent draft) and kind 1234 (NIP-37 checkpoint). NIP-44 self-decryption is a load-bearing kernel dependency for class routing because kind:10013 is the first NIP-51 list that requires the active signer to be decrypted. The drafts field in the NIP-51 fact-stream is Option<Vec<_>> to distinguish 'encrypted-but-undecryptable' (signer not ready) from 'decrypted-but-empty'. A draft being written without a signer being ready is a nonsensical scenario; the boot-ordering concern about missing signers at draft time is dropped. [^41858-5]

## GroupMessage Exclusion

EventClass::GroupMessage is kept for diagnostics but never participates in class_relays routing, as NIP-29 events route via relay_pin. [^41858-6]
## See Also

