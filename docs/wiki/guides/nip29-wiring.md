---
title: NIP-29 Wiring
slug: nip29-wiring
topic: marmot
summary: A feature belongs in an NMP crate when it is a general Nostr building block that any Nostr app (or a meaningful subset) could use directly; the test is 'would t
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-06-19
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:64c4fde3-6f5e-456a-b4bb-9f17517e301c
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:019edbff-8164-7a20-abc2-c977bc495d49
  - session:019edc00-f3a6-77f3-b21a-d6b45f5f6cab
  - session:019edc01-fdde-7b20-a348-5a2a9ce1a0f9
  - session:019edc0c-2dd1-7b80-b737-7499340e1b49
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edc3c-53d4-73a0-8c42-a6b88a318e8c
  - session:019edc59-7035-7ba3-95cc-789d362adff2
  - session:019edc92-b628-7ce1-be8a-c3d1013f2969
  - session:019edcba-b578-71f3-be33-f670962f11a7
---

# NIP-29 Wiring

## Wiring Location

A feature belongs in an NMP crate when it is a general Nostr building block that any Nostr app (or a meaningful subset) could use directly; the test is 'would this crate be useful to a completely different Nostr app?'. App Rust crates (apps/<app>/) hold features specific to that application's domain that would not generalize to other Nostr apps; NMP does not accumulate app-specific logic. The line between NMP and app crates is generic Nostr building block vs. this app's proprietary domain, not protocol vs. product; product-level features like NIP-29 group chat or Marmot MLS encrypted groups belong in an NMP crate if other Nostr apps would use them. NIP-29 wiring code must not live in nmp-app-chirp because it contains zero Chirp-specific nouns and violates the thin-shell rule (D0 doctrine). The canonical implementations of wire_group_chat, wire_group_discovery, and register_actions live in the nmp-nip29 crate, not in Chirp. Chirp's nmp_app_chirp_register_group_chat, nmp_app_chirp_register_group_discovery, and register_nip29_actions are thin one-liner delegates to nmp-nip29's wiring functions.

nmp-defaults is a reusable NMP composition library (crate-boundaries.md §9), NOT a leaf app; it must not own operator policy — relay URLs, bootstrap relays, seed pubkeys, auto-follow lists, or signer perms. Hardcoded operator relays, seed pubkeys (including DEFAULT_FOLLOWS with fiatjaf), default app relays, and nostrconnect bootstrap relay must not live in nmp-core/nmp-defaults; they belong ONLY in app-level code.

NMP owns protocol primitives, raw projections, routing/storage/search seams, and app-neutral write builders; the app owns product concepts including rooms defaults, vault organization, podcast resume, relay.highlightler.com policy, wifi preferences, and onboarding choreography. NIP-25 owns public reaction semantics (kind:7); NIP-29 owns group-tagged reactions (kind:7 with h tag); protocol crates must not import one another, and cross-protocol composition belongs in the app crate. nmp-nip01 must not own reaction protocol state.

Before any new NMP crate, helper, schema, or action is accepted, the implementer must prove the existing NMP seams (open_interest, open_uri, claim APIs, projections, ActionModule, PublishRaw, ProtocolCommand) cannot already express the need. App-reported issues are demand signals, not architecture specs; NMP ownership decisions must be made by extracting reusable Nostr/framework seams, not by implementing app-reported needs upstream as written.

Issue #1554 (nmp_nip21_decode_uri FFI decoder) belongs in NMP and is a clean, reusable, low-risk first win; implementers must prove nmp_app_open_uri does not already satisfy the Highlighter path before adding new NMP code. Issue #1557 (ProfileCard raw fields + unknown kind:0 round-trip) belongs in nmp-nip01/profile; it is safe but schema/regen-heavy. Issue #1555 (viewer-reaction projection) belongs in NMP but must use NIP-module command seams (NIP-25 owns public kind:7 reaction semantics) rather than hardcoding ActorCommand::Unreact or NIP-25 nouns into nmp-core; nmp-nip01 must not own reaction protocol state. Issue #1556 belongs in NMP only for the generic account/create-key lifecycle and capability seam; the default bootstrap policy remains app-owned, and if it changes C-ABI, an additive/versioned ABI entry point is preferred, or all host callers must update in the same PR. Issue #1558 must be split: NMP gets only the generic nmp-nip78 app-data mechanics and relay-role vocabulary; rooms, relay.highlighter.com, wifi-only, cache-stat UX, import UX, and Highlighter-specific keys stay in hl. Issue #1559 (NIP-29 admin actions) belongs in NMP but requires an ADR before implementation. Issue #1560 belongs in NMP only if neutralized around h-tagged group events, not Highlighter artifact product language. Issue #1561 (NIP-50 content search) belongs in NMP, must be anchored to ADR-0020, and should likely be split into a search primitive and a kind:10007 relay-list routing module. Issue #1562 bookmark mechanics/raw projection belong in NMP, but vault/product organization stays in hl; core bookmark command variants must not be added unless explicitly ADR-blessed, and a D0 rewrite is required. Issue #1563 was closed as no NMP change: a bare #i filter is already expressible via the existing InterestShape/filter path (nmp_app_open_interest with {"#i":[...]}), so no new NMP module is needed; the only reason to reopen would be a proven generic gap in cache-serve cold-start, tag indexing/perf, planner routing, or NIP-73 validation.

GitHub issues #1554 through #1564 must be treated as app migration demand signals, not as architecture specs; their literal issue text overfits to Highlighter's product model and would pull app vocabulary into NMP. Implementation must not start from the app-generated issue bodies as-is; they must be edited first to encode the real NMP/app boundaries. Issue #1564 serves as the single boundary-correct tracker, edited in place before implementation, with lanes for safe substrate, rewrite-before-work, ADR-before-code, and dependency ordering; it must not be executed as originally written. All GitHub issues #1554 through #1564 were edited in place to replace app-generated implementation dumps with architecture contracts, rename titles to reflect NMP boundaries, mark #1556/#1559/#1561 as ADR-first, and rewrite #1555/#1562 to avoid hardcoded NIP nouns in generic nmp-core.

NMP core should not grow NIP-specific enum verbs like ActorCommand::Unreact or BookmarkAdd; prefer NIP crate action modules and ProtocolCommand seams instead, unless an ADR explicitly carves an exception. Highlighter-specific concepts that must not become NMP work include: rooms semantics as framework vocabulary, relay.highlighter.com invariants, wifi-only policy, cache-stat UX, podcast resume store, vault organization, app-specific import flows, and core enum variants for every NIP action.

Safe substrate issues should land first in order: #1554, then #1557, then rewritten #1555, then ADR-backed #1556, then minimal rewritten #1558, merging one ABI/schema PR at a time. Later waves (#1559, #1560, #1561, #1562) should be deferred until waves 1–2 actually unblock hl, not blindly run from the tracker.

nmp-nip60/src/relay.rs is identified as over-engineering (a self-contained second framework inside a Layer-4 crate) but is not actively being worked in this campaign. <!-- [^11850-162] -->

Polling is forbidden at every layer: no sleep+check loops, no Timer.scheduledTimer querying state, no try_recv+sleep spin loops, no Task{while !cancelled{sleep;checkState()}} tasks. <!-- [^019ed-159] -->

NIP-78 application data crate (nmp-nip78) must be a Layer-4 protocol crate with a KernelEventObserver projector and pure builder/read APIs; no core integration is needed because apps register the observer and use existing OpenInterest/PublishUnsignedEvent paths. <!-- [^019ed-160] -->

<!-- citations: [^019ed-76] [^64c4f-3] [^1670f-10] [^1670f-11] [^1670f-15] [^1670f-16] [^1670f-17] [^1670f-18] [^86221-6] [^019ed-17] [^019ed-23] [^019ed-46] [^11850-7] [^11850-14] [^019ed-93] [^11850-73] [^019ed-121] [^11850-120] [^019ed-127] [^019ed-129] [^019ed-133] [^019ed-137] [^019ed-139] [^11850-160] -->
## Registration

nmp_nip29::register::wire_group_chat registers GroupChatProjection as both a KernelEventObserver and a snapshot projection under the key "nmp.nip29.group_chat". nmp_nip29::register::register_actions binds all 5 NIP-29 ActionModules (PostChatMessageAction, ReactInGroupAction, CommentInGroupAction, DiscoverGroupsAction, JoinGroupAction).

The router is a single generic algorithm with an explicit-target override on ActionContext (RoutingContext::explicit_targets); NIP crates do NOT register RoutingRule implementations with the router. The per-NIP `classify_kind` table in the generic router must be removed; instead, thread EventClass through RoutingContext from the NIP-aware caller.

NIP-17 DMs pass explicit relay targets from nmp-nip17's own DmRelayCache via ActionContext::explicit_targets; the router does not know what kind:1059 or kind:10050 is.

NIP-29 group posts pass explicit relay targets from group state via ActionContext::explicit_targets; the router does not know NIP-29 exists.

Rerouting NIP-29 publish through RoutingContext::explicit_targets is not correctly actionable without editing the kernel publish path (PublishTarget, publish engine, route_publish, actor dispatch), which is outside the allowed edit scope for this lane. The only minimal-yet-architecturally-correct in-scope change is adding RoutingSource::Nip17DmRelay to the selection bypass predicate; NIP-29 PublishPlan should be left as-is, and publish-side explicit_targets cleanup should be reported as a separate publish-path lane decision.

P7 Finding #1 (NIP-17 named three levels deep → relay_pin) is NOT a defect — relay_pin is a static single-URL hard pin, while Nip17DmRelays is a dynamic per-#p lookup against the kind:10050 DmInboxRelayLookup cache that must fail-closed when unknown; the RoutingSource distinction is correct.

DmInboxLookup on ProtocolCommandContextParts is left as-is — it is a Noop D15 capability, not a real D0 violation.

NIP-29 admin actions PutUser (kind 9000) and CreateInvite (kind 9009) must be structurally validated and host-pinned, with 9009 fanout capped at 10 codes per event. <!-- [^019ed-161] -->

<!-- citations: [^11850-74] [^64c4f-4] [^1670f-12] [^1670f-13] [^1670f-14] [^019ed-1] [^11850-137] [^11850-161] -->
## Observer Strategy and Test Seam

GroupChatProjection uses KernelEventObserver (not RawEventObserver), making it reachable via IngestPreVerifiedEvents for hermetic round-trip testing without a relay. This contrasts with DmInboxProjection, which uses RawEventObserver and is NOT reachable via IngestPreVerifiedEvents, creating a test-seam gap that NIP-29 does not have. nostr-relay-builder is not present in the workspace Cargo.lock, so no in-process relay harness is available for two-instance relay-connected tests. <!-- [^64c4f-5] -->

## Read Path

The architecturally honest read path for NIP-29 projections is nmp_app_set_update_callback — the same path iOS KernelBridge.swift uses — not a test-only pub accessor on NmpApp. <!-- [^64c4f-6] -->

## Round-Trip Testing

The NIP-29 group-chat round-trip test lives in crates/nmp-nip29/tests/group_chat_round_trip.rs, not in the Chirp app crate. <!-- [^64c4f-7] -->

## Projections and Shell Formatting

DiscoveredGroup in nmp-nip29 exposes raw fields (name: Option<String>, group_id, public: bool, open: bool, member_count, admin_count) and no longer provides Rust-side formatted display_name, initials, or subtitle fields; the deleted Rust finalize_display_fields fn and its 7 associated tests were removed because formatting now belongs on the native shell side, consistent with the doctrine that projections are raw and shells format (aim.md §2). The original Rust display_name fallback used name only when it was non-empty (not merely Option::is_some), falling back to group_id when name is Some(""). The original Rust subtitle format used '·' as the separator (e.g., '# Public · Open · N members' / '🔒 Private · Closed · 1 member'). The iOS DiscoveredGroup Swift extension derives displayName as (name ?? group_id), initials as the first two uppercased characters of the display source, and subtitle as a string combining privacy/public icon, open/closed status, and pluralized member count, so that views like JoinGroupView require no changes; the Swift extension's use of prefix(2).uppercased() can expand Unicode scalars (e.g., ßx → SSX), diverging from the deleted Rust logic that capped to one uppercase scalar per source char (ßx → SX). Android has no NIP-29 DiscoveredGroup render path and references Marmot rows instead of NIP-29 DiscoveredGroup, so the absence of an Android formatting update for DiscoveredGroup is intentional rather than a parity gap. <!-- [^019ed-105] -->

The NIP-29 joined_groups projection must derive membership/admin status only from latest relay-signed 39001/39002 events, never from 9000 actions. <!-- [^019ed-162] -->

Invite code validation must reject uppercase hex and any whitespace in invite codes; the wire policy must preserve exactly the validated code string without silent trimming. <!-- [^019ed-163] -->
