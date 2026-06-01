---
title: NMP Interest Subscriptions & Deduplication
slug: nmp-interest-subscriptions
summary: "Apps declare event subscriptions via `NmpApp::push_interest(LogicalInterest)`"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-23
updated: 2026-06-01
verified: 2026-05-23
compiled-from: conversation
sources:
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:64f3e239-c4c1-4c32-82de-458516b28418
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:37035e20-9c1c-418f-88f1-68e464b51ec7
---

# NMP Interest Subscriptions & Deduplication

## Declaring Event Subscriptions

Apps declare event subscriptions via `NmpApp::push_interest(LogualInterest)`. The Core Interest Registry manages single-writer logical subscriptions, deduplicating across owners using a `(owner, key, scope)` triple and performing refcount garbage collection when the last owner drops. Each subscription carries a stable `InterestId` that deduplicates on re-push. Scopes control routing: `InterestScope::ActiveAccount` re-routes subscriptions when the user switches accounts, while `InterestScope::Global` is account-agnostic. The `LogicalInterest` struct includes an `is_indexer_discovery` sentinel field that gates routing to `bootstrap_indexer_relays`, independent of `InterestLifecycle`.

`push_interest` and the `open_*` feed functions (`open_author`, `open_thread`, `open_firehose_tag`) do not need a `force` flag because they are open-ended streams or deduplicated subscription registrations, not targeted fetches of a specific replaceable event.

The core should remain kind-agnostic, with the app layer explicitly composing calls (e.g., `open_interest` + `claim_*`) rather than having NIP-crate helpers that wrap the composition. A background Opus agent should be launched to propose a concrete redesign of the subscription primitives: a generic filter-based C-ABI, which named primitives survive or die, and how `claim_*` fits in.

`open_firehose_tag`, `open_author`, and `open_thread` are a legacy path that bypasses the `InterestRegistry`, and should be deleted and replaced by the generic filter-based `nmp_app_open_interest` / `nmp_app_close_interest` C-ABI. Once the F-TTL PR merges, the M2 migration should proceed immediately: delete the legacy primitives (`open_firehose_tag`, `open_author`, `open_thread`), add `nmp_app_open_interest` / `nmp_app_close_interest`, and migrate Chirp call sites to explicit composition. `open_uri` survives the M2 migration as a URI router, which is a genuinely different concept from filter-based subscriptions.

`claim_profile` and `claim_event` are kept separate from `open_interest` because they are refcounted oneshots that drive `claimed_*` projection keys, which have a different lifecycle than tailing feeds.

At login, NMP registers Tailing bootstrap interests for the logged-in user's own kinds 0, 3, 10002, 10000, and 10006, keeping subscriptions open after EOSE. The default set of bootstrap self-kinds (0, 3, 10002, 10000, 10006) can be overridden by apps before `nmp_app_start`. Bootstrap self-kind subscriptions do not use `limit:1` — relays automatically send the correct replaceable event.

Timeline rows claim visible note relations on `.onAppear` and release them on `.onDisappear` so the kernel keeps relation data hydrated.

<!-- citations: [^1670f-8] [^64f3e-3] [^54ae9-19] [^64f3e-2] [^54ae9-18] [^37035-7] -->
## Reactive Follow-List Processing

When a new kind:3 event arrives on the open Tailing subscription, `ingest_contacts` fires, `sync_follow_feed_interests` updates follow interests, the `FollowListChanged` trigger causes the planner to close removed follow subs and open new ones, and a snapshot is emitted — all without app code involvement. <!-- [^64f3e-4] -->
