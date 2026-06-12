---
title: Interest Compiler and Feed Subscription
slug: interest-compiler
topic: interest-compiler
summary: The kernel is kind-agnostic; app-level kind decisions (e.g., `{1, 6}` for social feeds) belong in the Swift/app layer, not the FFI substrate.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-01
updated: 2026-06-12
verified: 2026-06-01
compiled-from: conversation
sources:
  - session:37035e20-9c1c-418f-88f1-68e464b51ec7
  - session:b4fe9cec-eb86-47f7-bc1d-3c28a18d5fcf
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
---

# Interest Compiler and Feed Subscription

## Architecture

The kernel is kind-agnostic; app-level kind decisions (e.g., `{1, 6}` for social feeds) belong in the Swift/app layer, not the FFI substrate. PR #911 tracks V-68 Stage 2: the three nmp-ffi sites (`open_timeline`, `open_author`, `open_thread`) hardcoding kinds {1, 6}, requiring an ADR-gated migration to `nmp_app_open_interest` or a `kinds` parameter. (Previously: The only real problem in the FFI surface is the three `open_*` functions that hardcode social kinds {1, 6}, which should accept a kinds parameter or be replaced with `nmp_app_open_interest`.) The M2 migration unifies two parallel subscription machines: the legacy path (`open_author`/`open_thread`/`open_firehose_tag`/`claim_profile`) that bypasses the `InterestRegistry`, and the modern path (`claim_event` + `SubscriptionCompiler`). Thread/author context hydration (fetching the root event or author profile) must be explicitly composed by the app via `claim_event`/`claim_profile` alongside `open_interest`, not handled by a fused kernel primitive. `claim_profile`/`claim_event` remain separate from `open_interest` because claims are refcounted oneshot fetches that drive `claimed_*` projection keys — a different lifecycle and visibility concern from tailing feed subscriptions. The subscription `SubKey` derivation for `open_interest` uses the canonical hash of the parsed `InterestShape`, so two call sites passing the same filter JSON with different key ordering dedup onto one slot. The `should_store_event` kernel gate has been generalized to admit events matching any active interest's wire filter, not just follow-set/view/sub-id-prefix clauses. Matched non-followed events reach the store and feed-engine fan-out but must not pollute the follow-only home timeline insertion.

<!-- citations: [^37035-7] [^37035-8] [^37035-9] [^37035-10] [^37035-11] [^37035-12] [^37035-13] [^b4fe9-2] -->
## API

The M2 migration replaces named feed primitives with a generic `nmp_app_open_interest(app, filter_json, consumer_id, scope)` / `nmp_app_close_interest(...)` pair, where `filter_json` is a standard Nostr REQ filter string parsed core-side. <!-- [^37035-14] -->

`nmp_app_open_interest` is backed by `EnsureInterest`/`DropInterestOwner` (not `PushInterest`/`WithdrawInterest`) to get `(owner, key, scope)` dedup and refcount semantics. <!-- [^37035-15] -->

## Deleted Primitives

`nmp_app_open_firehose_tag` is deleted — it is pure sugar for `{"kinds":[1],"#t":[tag]}`, and the compiler's Case-D already routes that filter shape identically. <!-- [^37035-16] -->

Legacy `nmp_app_open_author`/`close_author`/`open_thread`/`close_thread` C-ABI symbols and the `AuthorViewState`/`ThreadViewState` state machine, `author_view`/`thread_view` FlatBuffers schemas, and generated Swift readers are deleted in v0.4.0, replaced by `nmp_app_open_interest` with NIP-01 filter JSON. <!-- [^da6b1-88] -->
