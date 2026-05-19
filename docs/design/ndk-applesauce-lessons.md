# Design Note: Lessons from NDK and Applesauce

> **Status:** Draft
> **Date:** 2026-05-18
> **Scope:** High-level product and architecture lessons to preserve before NMP's outbox, relay policy, event store, and action layers are built. This is not an implementation plan.

## 1. Purpose

NDK and Applesauce both solve real Nostr client problems that NMP must eventually solve: relay discovery, outbox routing, cache-backed reads, action-driven writes, derived models, and sane application defaults. They do it with different API philosophies and runtime assumptions.

NMP should learn from both, but it should not port either library's API or internal structure. NMP's constraints are different: Rust owns state and policy, native platforms render snapshots and execute capabilities, and the public framework path should make common Nostr mistakes difficult to express.

The value of this note is to keep the useful lessons durable while the current NMP prototype changes.

## 2. What NDK Gets Right

NDK's strongest lesson is that outbox routing should feel automatic to application developers. A developer should not normally decide which relays to use for a profile, timeline, reply, or publish. The framework should infer the right relay set from NIP-65 metadata, event tags, relay hints, and fallback policy.

NDK also models an important operational truth: relay metadata can arrive late. A subscription may start with incomplete routing information, then improve when relay lists are discovered. The system should be able to refresh or expand active work when better metadata appears instead of requiring the app to tear down and recreate views.

NDK's relay-set abstraction is useful as a product concept. Once routing policy decides where an operation belongs, the transport layer should receive a clear set of relays and carry out the operation. That keeps relay choice distinct from socket mechanics.

NDK's publish policy is also directionally correct: publishing is not just "send to my favorite relays." Public events need the author's write relays. Replies and mentions need recipient reachability. Event relay hints can improve delivery. The publish pipeline must make these choices consistently.

## 3. What NDK Warns Us About

NDK's convenience can blur boundaries. If relay tracking is mostly ephemeral, the framework can forget important routing knowledge between sessions and repeat expensive discovery. NMP should treat relay metadata as durable application state, not merely a short-lived optimization.

Automatic behavior also needs strong tests. Outbox routing bugs are easy to miss because the app often still "kind of works" against large public relays. NMP should test routing decisions directly, not only test that events eventually appear.

NMP should also avoid a monolithic "Nostr client object" that quietly accumulates cache policy, relay policy, subscription grouping, publishing, sessions, and product behavior. The public path can feel simple, but internally the boundaries need to stay sharp.

## 4. What Applesauce Gets Right

Applesauce's strongest lesson is separation. It treats event storage, derived models, relay selection, relay transport, and actions as separable concerns. That is a better fit for NMP's actor-owned app kernel than a single all-knowing client object.

Applesauce's outbox-map idea is especially important at the conceptual level: author-scoped reads should be grouped by the relays that can actually serve those authors. A timeline for many authors should not become one large author filter blasted to every relay. The routing layer should split work by relay responsibility.

Applesauce also keeps pointer relay hints as first-class information. Relay hints from events, tags, and NIP-19 pointers are not the same as NIP-65 relay lists, but they are useful evidence. NMP should preserve that distinction:

- NIP-65 describes a user's declared relay preferences.
- Relay hints describe where a specific event or pointer may be found.
- Seen-relay provenance describes where NMP actually observed an event.
- User-configured relays describe local policy and fallback preference.

Those facts should inform each other without being collapsed into one ambiguous "relays" field.

Applesauce's action-oriented write flow is also useful. Product actions know the intent: update a profile, publish a comment, send a wrapped message, manage relay metadata. Relay delivery should be part of the action pipeline, not a detached utility the app developer remembers to call.

## 5. What Applesauce Warns Us About

Applesauce exposes more explicit routing responsibility to application code than NMP should expose on its safe path. Explicit maps and relay lists are useful internally and for diagnostics, but NMP's normal app developer should ask for a domain operation or a view, not assemble relay routing by hand.

NMP should avoid ambiguous publish APIs. The order and meaning of "event" versus "relays" should be impossible to mix up in the safe API. More importantly, the safe API should encode intent rather than asking callers to provide raw relay destinations.

NMP should also be careful with fallback sequencing. A dead relay hint should not indefinitely block discovery through healthier relays. Fallback policy should be bounded, parallel where appropriate, observable, and cancellable.

Finally, NMP should ensure that internal filter splitting cannot be accidentally undone by caller-supplied filter fields. Once the planner has decided that a relay is responsible for a subset of authors, that split is policy, not an optional suggestion.

## 6. NIP-77 And Sync Lessons

NDK and Applesauce both treat NIP-77 as a bandwidth-efficient reconciliation tool, not as a replacement for ordinary live subscriptions. That distinction matters for NMP.

NMP should preserve the principle that live views start receiving new events immediately. Historical completeness can run in the background through NIP-77 when a relay supports it, and through bounded fallback when it does not. A user should not wait for a full reconciliation before seeing fresh events or cached state.

NIP-77 also implies that cache coverage is real state. The framework needs to know what filter, relay, and time range has been reconciled, and whether that coverage came from NIP-77, a normal relay request, imported cache data, or an unknown source. Without coverage records, the framework cannot distinguish "nothing exists" from "we have not checked yet."

Relay capability should likewise be durable and observable. NDK's capability caching and Applesauce's explicit support checks both point to the same lesson: probing support on every operation wastes time, but assuming support forever is brittle. NMP should remember relay capabilities, refresh them deliberately, and expose degraded paths in diagnostics.

The sync engine should share relay policy with the subscription planner. If live reads for an author go to the author's outbox relays, historical reconciliation for that same view should not quietly use a different relay universe. Sync, live REQ, cache reads, and fallback discovery are different execution modes for the same logical interest.

NMP should also make sync cancellable and bounded. Long reconciliations must respect view lifecycle, app backgrounding, network state, and user intent. A view closing should not leave invisible historical work running without an owning claim or ledger row.

## 7. Subscription Compilation Lessons

NDK's subscription grouping and Applesauce's relay/filter maps both point to a broader design lesson: NMP should treat subscription compilation as a first-class planner stage.

A platform or app module should express a logical interest: a timeline, a profile, a thread, reactions, mentions, a conversation, a wallet history. The framework should compile that interest into a plan:

- what cached data can satisfy it now,
- what derived views need to be materialized,
- what relay metadata must be discovered,
- which relays should receive live requests,
- which relays should receive NIP-77 reconciliation,
- what fallback is allowed,
- when the work should close,
- what diagnostic state should be emitted.

That plan is not just a Nostr filter. It is a policy object owned by the actor. A Nostr filter is one possible wire artifact produced by the plan.

Compilation must be semantics-preserving. Some filters can be safely merged, split, or grouped. Others cannot. Limits, time bounds, close-on-EOSE behavior, relay exclusivity, private delivery, and author-specific routing can all change the meaning of a request if combined carelessly. The compiler should prefer doing less work over issuing an over-broad request that silently changes semantics.

The compiler should also be able to recompile. Relay lists arrive late, cache coverage changes, relays connect or disconnect, NIP-77 capability becomes known, auth state changes, and view claims appear or disappear. Recompilation should update the underlying work without exposing protocol churn to platform code.

NMP should avoid making "grouping delay" or "relay batching" part of the public mental model. Those are execution optimizations. The stable product idea is that identical or compatible logical interests share work, while incompatible interests stay separate even if they look superficially similar.

## 8. Loading And Pagination Lessons

Applesauce's timeline loaders highlight a useful distinction between the desired window and the network request used to fill it. A user scrolling a timeline is not asking the app to send a specific relay filter. They are asking the app to extend a view window.

NMP should model pagination and loading as view actions over bounded windows, not as platform-issued relay fetches. The actor can decide whether the next page comes from cache, NIP-77 coverage, live relay requests, or a fallback path. The platform should receive refined view state, not own cursor policy.

This also keeps performance policy centralized. The same planner that owns outbox routing can enforce limits, deduplicate page loads, avoid duplicate boundary events, and prevent multiple components from opening equivalent pagination work.

## 9. NMP Principles To Preserve

### 9.1 Outbox Is Policy, Not Transport

Outbox support belongs in the actor-owned routing and planning layer. Relay sockets should not decide whether a relay is an inbox, outbox, indexer, fallback, or hint. They should connect, subscribe, publish, report status, and nothing more.

### 9.2 Relay Metadata Is Durable Domain State

Relay lists, relay hints, seen-relay provenance, relay health, and coverage records are part of the app's durable understanding of the Nostr network. They should survive restart and be available to diagnostics. They should not be treated as incidental cache entries.

### 9.3 Developer APIs Should Express Intent

The safe public surface should be phrased in product terms: open a timeline, open a profile, publish a reply, update my relay list, send a message. Relay choice is framework policy. Manual relay selection can exist for tests, diagnostics, migration tools, and advanced overrides, but it should be visibly outside the default path.

### 9.4 Reads Should Render Before Routing Is Perfect

NMP's best-effort rendering doctrine still applies. Missing relay metadata should not block rendering cached data or placeholders. The system can start with known data, report degraded coverage, discover better relay metadata, and refine the view in place.

### 9.5 Privacy-Sensitive Routes Must Fail Closed

Public timeline discovery can tolerate bounded fallback. Private or recipient-sensitive publishing cannot. DMs, gift wraps, and other private flows must not silently fall back to broad public relays because metadata was missing.

### 9.6 Routing Decisions Must Be Observable

Outbox bugs are hard to diagnose from UI symptoms. NMP should expose enough diagnostic state to answer:

- what operation was requested,
- which relay categories were considered,
- which relay set was chosen,
- what fallback was used,
- what coverage is known,
- whether live, sync, cache, or fallback execution is active,
- which relays accepted, rejected, or timed out.

This should be actor-derived diagnostic state, not raw socket callbacks crossing FFI.

### 9.7 Tests Should Target Policy Directly

NMP should have tests that assert routing policy, not just end-to-end happy paths. The important failures are subtle: wrong relay category, over-broad fan-out, stale relay-list replacement, fallback that hides missing coverage, and private events delivered to public relays.

### 9.8 Coverage Is Different From Cache Presence

Having an event in the local store does not prove that a view is complete. NMP should distinguish cached facts from coverage facts. A cache can answer "what do we currently have?" Coverage can answer "what range have we reconciled against which relays?" Both are needed for correct rendering, sync, and diagnostics.

## 10. Product Direction

The synthesis is:

- From NDK, take the ambition that outbox routing is automatic by default.
- From NDK, also take the live-subscription-plus-background-sync shape for NIP-77.
- From Applesauce, take the compositional discipline: metadata, relay selection, transport, store, models, loaders, and actions are separate concerns.
- From Applesauce, also take the idea that a desired view window and the relay requests used to fill it are separate things.
- From NMP's own doctrine, keep Rust as the single owner of policy and keep platform code out of protocol decisions.

NMP should therefore make outbox invisible in ordinary app code but visible in diagnostics and tests. It should use explicit internal routing artifacts, but expose intent-oriented product APIs. It should route efficiently by author and recipient, but still render cached state immediately. It should support fallback for public reads, but fail closed for private delivery. It should make NIP-77 a coverage and backfill policy, not a blocking fetch API. It should compile logical interests into cache, sync, live, and fallback work without exposing that machinery to platform code.

That is the durable lesson: outbox support is not a feature bolted onto subscriptions. It is a framework-level correctness policy that touches storage, planning, publishing, diagnostics, privacy, and the developer API.
