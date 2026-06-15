---
title: Outbox Resolver
slug: outbox-resolver
topic: relay-routing
summary: The OutboxResolver must apply the blocked-relay filter on publish, not just subscribe, to prevent publishing to user-blocked relays
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:bf035812-6f7a-46ec-a11d-30fc7369342f
  - session:ab8061fc-b277-4ba4-bf55-1532bcb1aa90
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
  - session:78b50727-bccd-4088-8493-a07624a4fa83
---

# Outbox Resolver

## Blocked-Relay Filter Bypass

The OutboxResolver must apply the blocked-relay filter on publish, not just subscribe, to prevent publishing to user-blocked relays. The publish path applies this filter across all lanes including explicit targets. The publish-engine holds an Arc<dyn BlockedRelayLookup> resolved per publish so that publish and subscribe share one blocked-relay cache.

Follow read-your-writes (local publish routed through the same ingest + observer fan-out as relay events) landed as PR #1199. Previously, tapping Follow updated the router immediately but never reached the follow-list projection or ActiveFollowSet until restart, because local publish skipped observer fan-out and the relay echo deduped as Duplicate. <!-- [^2e544-398] -->

<!-- citations: [^02745-13] [^02745-40] [^02745-61] -->
## URL Canonicalization

Relay URL canonicalization must be shared between kind:10002 (ingest) and kind:10006 (blocked relays) so that casing and trailing-slash mismatches resolve to the same key.

<!-- citations: [^02745-14] [^02745-41] [^02745-62] -->
## Lane 6 Indexer-Relay Discovery Kinds

Lane 6 must record a per-relay discovery-kind scope so that mixed interests (e.g., [1, 3]) only expose kind:3 to indexer relays while all-discovery-interests leave no override, rather than broadcasting all-interest kinds to indexer relays. The kind:0 'indexer-only, must not leak to app relays' contract is obsolete; claim interests use the compiler's default generic routing (app_relays + indexer for uncached authors), so kind:0 claims route to app/content relays at cold start using the default generic routing (app relays + indexer when cold, author's own relays when warm). No BootstrapFallback::IndexerOnly compiler extension is needed.

<!-- citations: [^02745-42] [^02745-63] [^ab806-47] [^ab806-117] [^ab806-160] [^ab806-196] -->
## Greedy Merge Deterministic Sort

Greedy merge must use a total canonical sort key (not just the spec tuple) to produce deterministic REQ output regardless of input order. <!-- [^02745-43] -->

## Wildcard-Kinds Merge Refusal

Wildcard-kinds × concrete-kinds merges must be refused (mirroring Rule 9) to prevent an all-kinds privacy/bandwidth leak. <!-- [^02745-44] -->

## Auth-Relay Publish Parking

Publishing to AUTH-required relays must park the event via the availability-gate seam and re-dispatch on Authenticated instead of failing terminal (landed as PR #1192). The AUTH handler re-dispatches via mark_publish_relay_available. Previously, publishing to an AUTH-requiring relay raced the handshake and settled FailedAfterRetries because the one-shot reauth budget was consumed within a 250ms tick.

Failed publish terminals must not double-report into a last-writer-wins global slot that clobbers unrelated error toasts. <!-- [^2e544-453] -->

<!-- citations: [^2e544-65] [^2e544-353] [^2e544-399] [^2e544-436] -->
## Score Map Ingest from Mainline

The score map must be fed from mainline ingest—recording Hit on every EVENT attributed to (author, relay_url)—rather than only from claims. This activates the dormant W4 warm filter and triggers the record_failure decrement path. <!-- [^2e544-66] -->

## Offline Publish Fail-Closed

The offline publish path (reply/react/follow/unfollow) must fail closed with honest error modes rather than silently dropping or succeeding partially. <!-- [^bf035-168] -->

## Follow/Unfollow Contact-List Guard

Follow/unfollow must use try_current_follows returning Option to distinguish not-loaded from empty, preventing silent wiping of the user's contact list on a redundant SetSigner or incomplete follow list. <!-- [^bf035-169] -->

## Probed-Mailboxes Epoch-Gated Retry

The probed_mailboxes retry mechanism must use an epoch-gated model that bumps the probe epoch on indexer reconnect and on new-indexer-added, so uncached authors are re-probed after any relay-availability change without a per-recompile storm. <!-- [^ab806-15] -->

## Higher-Order Feature Obliviousness to Relay Lists

Higher-order feature code (follows feed, profile claim) must be completely oblivious to relay lists; they only declare authors:[...] and kinds:[...] and inherit the 10002 acquisition and per-author routing from the underlying subscription/routing infrastructure. <!-- [^ab806-16] -->

## Self-Sealing Indexer Dependency

The kernel's outbox router supports per-author NIP-65 relay routing (Lane 1) but third-party kind:10002 (relay lists) are never proactively fetched for the follow set or encountered authors, so MailboxCache is empty for nearly everyone and kind:0 queries fall through to indexer-only (Lane 6/7). The kind:10002 discovery fetch hits the same two indexers as the kind:0 fetch; if it fails there, both the relay-list and the kind:0 fail together, creating a self-sealing dependency where the outbox Lane 1 can never recover for that author. Nprofile/nevent relay hints are threaded into the claim interest's hints field (as HintSource::UserConfigured), so a stranger whose kind:10002 is on no indexer still resolves from their nprofile-embedded relay URL. The nip60 wallet's fetch_nip65_relays hardcodes wss://purplepag.es as a fallback, bypassing the kernel's outbox router for kind:10002 discovery—flagged as a minor separate follow-up issue (#1434), not part of the profile-claim migration.

<!-- citations: [^ab806-48] [^ab806-65] [^ab806-81] [^ab806-129] -->
## 7-Lane Outbox Router

The 7-lane GenericOutboxRouter routes per-author NIP-65 write relays (Lane 1), indexer relays (Lane 6), and app-relay fallback (Lane 7). The transport pool dials arbitrary relay URLs on demand via send_outbound/ensure_relay_worker_with_kind, with Temporary connections and 60s idle teardown; no new transport capability is needed for outbox routing to third-party author relays.

Greedy weighted max-coverage set-cover relay minimization already exists in the planner (selection.rs) and runs on every recompile, bounded by select_max_connections/select_max_per_user; no new set-cover implementation or artificial connection cap is needed, as relay-intersection minimization via the existing algorithm already bounds socket count and idle teardown handles cleanup.

drain_pending_reverify uses the same bespoke route_outbox_subscription_relays + req_for_relay pattern and lacks LogicalInterest registration, making it a SHOULD-MIGRATE instance of the same bypass defect. It was migrated to use OneshotApi::request as part of the same fix that corrected claim_profile for third-party authors.

Publish policy is a single declared table (`classify_publish_behavior(kind) -> PublishBehavior` in publish/policy.rs), the only function permitted to compare a publish kind to a named `KIND_*` constant; scattered raw `kind ==`/`!=` `<int>` or `kind ==`/`!=` `KIND_*` guards in publish routing are replaced and banned by a doctrine-lint rule that scans the full publish routing surface and enforces the single-declaration-point invariant. Private/fail-closed events (gift-wrap kind:1059, sealed kind:14) are structurally prevented from routing to public relays: they are rejected at both the action boundary and the universal enforcement point (`dispatch_due`), where `relay_emit_is_sanctioned` checks that `relay_reasons` includes `Explicit` and refused rows are terminally finalized (deleted from the durable store exactly once—never left Pending or re-refused on the next resume), not left pending. No path can Auto-route a private envelope to public relays, including on resume-from-store or manual retry. Kind:0/3 are ReservedBuilderOnly (raw publish refused), kind:1059/14 are PrivateFailClosed.

The universal enforcement point for publish routing policy is the dispatch-emit site (dispatch_due), where initial publish, resume-from-store, manual retry, and availability re-dispatch all converge.

Baseline measurement shows follows' kind:0 resolution at 10.2% with indexer-only queries, 50.0% with the outbox model (indexers ∪ each follow's own write relays), and 88.8% when also adding a broad app relay like nos.lol. (Previously: 10.2% indexer-only rising to 50.0% with outbox routing.) Of follows that an app relay adds beyond the outbox model, 204 of 300 publish no kind:10002 at all and are structurally unreachable by the outbox/NIP-65 path; an app relay is the only way to resolve their kind:0.

<!-- citations: [^ab806-128] [^ab806-89] [^ab806-98] [^ab806-118] [^ab806-134] [^019ec-19] [^019ec-38] [^ab806-239] [^ab806-267] [^78b50-179] [^78b50-194] [^78b50-199] [^78b50-210] [^78b50-222] [^78b50-232] [^78b50-244] -->
