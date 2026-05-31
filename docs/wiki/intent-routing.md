---
title: Intent Routing — OutboxResolver, Class Routing, and Planner
slug: intent-routing
summary: The kernel uses a kind-driven EventClass resolver with intent override as the routing approach (not purely intent-tagged), so the default does the right thing f
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-26
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:41858cd2-3a5d-4ad1-bdd0-4cbe1df2dd9d
  - session:d4b109a1-b655-4952-9e89-9a8a1438d6a2
  - session:50510273-d1c9-424a-b877-179d52fba557
  - session:fbebb78b-07ed-4e26-8e2e-56fb66929a63
---

# Intent Routing — OutboxResolver, Class Routing, and Planner

## Intent Routing

The kernel uses a kind-driven EventClass resolver with intent override as the routing approach (not purely intent-tagged), so the default does the right thing for 90% of cases while allowing per-publish overrides. PublishTarget::Auto is upgraded in place to be class-aware; no separate AutoByClass variant is needed, and every existing publish call site inherits class routing. Class routing is additive for specialized classes (Draft, Wiki, Search) and only overrides default for those; PublicNote, Profile, LongForm, and RelayList stay on NIP-65.

OutboxResolver::resolve takes a kind: u32 parameter to differentiate event routing.

The OutboxResolver trait must return Vec<ResolvedRelay> (carrying both url and reason) instead of BTreeSet<RelayUrl>. ResolvedRelay must be a named struct with url: RelayUrl and reason: String fields.

Nip65OutboxResolver must annotate each resolved relay with a reason: 'NIP-65 write relay' for author kind:10002 write relays, 'App relay (local config)' for local_write_relays fallback, 'Discovery indexer (kind {n})' for indexer relays on discovery kinds, 'Inbox relay for {short_npub(recipient)}' for p-tag inbox fanout, and 'Explicit relay' for PublishTarget::Explicit. When the same relay URL appears via multiple resolver paths, the reason from the first resolution path must be preserved (deduplication).

The relay_reason field must be trivially renderable by any app shell (iOS, Kotlin, web) without understanding NIP-65 or p-tag fanout logic.

The implementation must span 6 steps in order: ResolvedRelay trait change, Nip65OutboxResolver annotation, InFlight.relay_reasons, PublishOutboxRelay.relay_reason projection, OutboxLine.relays snapshot, and TUI AppState + detail pane UI.

The planner merges InterestShape objects with different `search` values by refusing the merge (same as Rule 9 on `relay_pin`).

When using UserPreferred search targets, the kernel queries exactly one relay from the user's NIP-51 kind:10007 list per call (round-robin or first-listed), not the full set, to prevent aggregate intent leak.

The class-to-relay mapping priority is: 1) App override (Explicit), 2) User's NIP-51 lists, 3) Kernel default policy (operator-configured fallbacks per class).

NIP-51 lists become a second fact stream into the OutboxResolver, parallel to NIP-65, enabling the resolver to answer 'given this event's class, where does the active account publish/subscribe?'

The OutboxResolver trait gains `class_relays_personal(class)` for self-keyed lists (Search, Draft, blocked) and `class_relays_for_author(class, author)` for publisher-keyed lists (Wiki).

Per-author class routing partitions wiki interests so that a REQ with authors:[bob, alice] uses bob's kind:10102 for bob's wiki kinds and alice's kind:10102 for alice's.

Per-author kind:10102 fetches are lazy, cached, and evicted: fetched the first time a Wiki interest names an author, kept alive while any class-routed interest references them, dropped when the last one ends.

When a user has no NIP-51 list for a class (search, wiki, drafts, etc.), the kernel falls back to an app-level default relay list provided at kernel init, same pattern for every class.

A draft being written without a signer being ready is nonsensical and not a design concern; if it occurs, the publish engine should return NoTargets failure.

EventClass::GroupMessage is kept in the enum for diagnostics but never participates in class_relays routing; NIP-29 events route via relay_pin (ADR-0012).

A kind:1 event routes identically to today (NIP-65 outbox, then blocked-relay filter), since PublicNote has RoutingFamily::None and skips case_g_class_routed.

A RelayScore trait seam exists in the outbox planner with a single method returning Option<f32> from a &ResolvedRelay, with a default implementation returning None.

NIP-66 data is not fed into the outbox scoring seam; it is reserved as a future provider.

Background subscriptions to monitor pubkeys and NIP-66-to-score fusion are deferred until a concrete user-visible need arises.

Routing decisions are fed from local RTT and NIP-65 first, before considering NIP-66.

<!-- citations: [^41858-1] [^41858-2] [^41858-3] [^41858-4] [^41858-5] [^41858-6] [^41858-7] [^41858-8] [^41858-9] [^41858-10] [^41858-11] [^41858-12] [^d4b10-1] [^d4b10-2] [^d4b10-3] [^d4b10-4] [^50510-2] [^fbebb-3] -->
## Blocked-Relay Handling

The planner filters the final per-relay plan against blocked_relays() (kind:10006) as a single post-processing pass, never silently dropping without a diagnostic lane bump. [^41858-13]

When the blocked-relay filter subtracts every relay from a plan, the planner fails loud with PlannerError::AllRelaysBlocked surfaced as a publish/subscribe-time error the UI must handle. [^41858-14]

## Kind Mappings

Kind 31234 (parent draft) and kind 1234 (checkpoint) both map to EventClass::Draft. [^41858-15]

Kind 10013 is the NIP-51 'Draft relays' list with nip44-encrypted content, requiring the active NIP-44 keypair for decryption. [^41858-16]

Kind 10102 is the NIP-51 'Good wiki relays' list for routing wiki events. [^41858-17]

## Indexer and App Relays

Indexer relays are additive for kind:0, kind:3, and kind:10000–19999 (is_discovery_kind returns true for these), regardless of NIP-65 status — both read and write. The has_role function must not treat 'indexer' as semantically including 'write'; indexer relays are only matched when explicitly requested as 'indexer'. All events must be published to write relays; kind:0, 3, and 1xxxx events must additionally be published to indexer relays.

App relays serve as a fallback when a user has no NIP-65 relay list and are additive alongside NIP-65 at login; they are operator-configured at kernel init and user-mutable in settings.

RoutingSource gains Indexer and AppRelay as top-level variants (distinct from UserConfigured), with Indexer being kind-gated (kind:0/3/1xxxx) and AppRelay being fallback + cold-start additive.

NMP reads kind:10086 (Indexer relays with 'relay' tags) per the NIP spec format, but applies those relays to a broader kind range (0, 3, 10000–19999) than the spec scopes them to (0 and 10002); this is a policy choice noted in the ADR.

The existing BOOTSTRAP_DISCOVERY_RELAYS hard-coded constant becomes the default operator value for the new KernelConfig fields.

Nip65OutboxResolver holds an Arc<Mutex<Vec<String>>> for indexer relays that the kernel keeps current via set_relay_edit_rows.

Documentation in subsystems.md, outbox.md, and nip65/mod.rs must reflect that discovery-kind events fan out to indexer relays, while the 'never the indexer set' constraint is preserved for PrivateToRecipients.

<!-- citations: [^41858-18] [^41858-19] [^41858-20] [^41858-21] [^41858-22] [^50510-1] -->
## Diagnostics

Worker-level RelayRole and planner-level RoutingSource are different abstraction levels: RelayRole is a transport-lane diagnostic bucket, RoutingSource is a planner-layer 'why this relay was chosen' tag. [^41858-23]

ADR-0020 (ClassRouted) and ADR-0021 (Indexer + AppRelay) introduce three new RoutingSource lanes simultaneously in the same PR, with the diagnostic-discipline doc updated once for all three. [^41858-24]

The diagnostic section describes seven lanes (NIP-65, Hint, Provenance, UserConfigured, ClassRouted, Indexer, AppRelay). [^41858-25]
## See Also

