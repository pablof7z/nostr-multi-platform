# Applesauce — Features NMP Must Add (or That Applesauce Lacks)

> Source: `/private/tmp/nostr-research/applesauce` @ `da5ec22b`.
> Cross-referenced with `docs/aim.md`, `docs/plan.md`, `docs/product-spec.md` in the NMP worktree.

## A. Features present in Applesauce that NMP must reproduce in `nmp-core`

These are not "missing" — they're the deliverables. Listed here because they constrain the architecture.

1. **Model-cache mixin pattern.** `event-models.ts:40-150`. NMP must expose an extension point so downstream crates (or LLM-generated views per `plan.md`) can register new typed views (`MutesView`, `ZapsView`, `WalletView`) without forking `nmp-core`. Applesauce does this via TypeScript prototype augmentation; NMP's equivalent in Rust is a `View` trait + registry.
2. **`share()` with keepWarm + ReplaySubject(1) semantics.** `event-models.ts:73-78`. Every typed view subscription must dedupe across consumers, replay last value to new subscribers, and stay warm for ~60s after the last unsubscribe.
3. **Claim refcount + LRU prune.** `event-memory.ts:176-242`, `observable/claim-*.ts`. Separate from RxJS refcounting. NMP's planner must claim every event surfaced through a view and release on view drop.
4. **Single-instance invariant via `mapToMemory`.** `event-store.ts:136-142`. Every read of the same event id returns the same `&Event` so identity-equality is meaningful. In Rust this is `Arc<Event>` interning.
5. **NIP-01 tie-break in both insertion directions.** `event-store.ts:235-301`. Pre-check and post-cleanup. Both async and sync paths.
6. **Per-account signer queue + `SignerMismatchError` post-conditions.** `accounts/src/account.ts:115-187, 33, 109, 123-124`.

## B. Features NMP has in spec that Applesauce does NOT have (and must build)

### B1. Outbox-by-default on publish

**Applesauce:** caller-responsibility. `ActionContext.publish(event, relays?)` lets the action resolve outbox via `user.outboxes$.$first(...)` and pass relays explicitly (`actions/src/actions/contacts.ts:6-21`). If a caller forgets, the publish goes nowhere useful.

**NMP spec (`aim.md:123-125`):** "Publishes for an event automatically go to the author's write relays plus inbox relays of any p-tagged recipients. The developer does not pick relays per operation; the framework does. They can override, but the override is the exception."

**Gap:** NMP must implement the default-on path. This means the planner inspects every publish, resolves author outboxes + recipient inboxes from the `OutboxState`, and routes automatically. Explicit-override is the opt-out.

**Implementation hint:** `OutboxModel` + `MailboxesModel` already exist as pure functions of the EventStore in Applesauce. NMP's `Publisher` is `OutboxResolver(event.author) ∪ ⋃_p InboxResolver(p)` invoked at publish time. The Applesauce TypeScript shows the resolution logic; NMP just needs to bake the invocation in.

### B2. Persistent sync watermarks + NIP-77 negentropy as a first-class concern

**Applesauce:** `applesauce-relay/src/negentropy.ts` exists and `RelayPool.negentropy(relays, store, filter, reconcile, opts)` is a method. It is **not integrated** with the loaders. There is no "before issuing this REQ, check the watermark" path. Timeline loaders use stateful backward/forward cursors in-process (`timeline-loader.ts:53-186`) that reset on app restart.

**NMP spec (`plan.md`, Phase 2 / M4):** Negentropy is the default sync mechanism with durable per-(filter, relay) watermarks consulted by the planner before issuing historical REQs.

**Gap:** Durable watermarks + planner integration. Applesauce gives you the building block; NMP must wire it into the planner's "should I REQ?" decision.

### B3. Subscription coalescing across views

**Applesauce:** Has dedup via the model cache (one observable per `hash_sum(args)`) but **does not coalesce filters at the relay level**. If view A wants `{kinds:[1], authors:[X]}` and view B wants `{kinds:[1], authors:[Y]}`, two REQs go out.

**NMP spec (`plan.md`, M2):** Subscription planner with coalescing.

**Gap:** Filter-level merge planner. Should be implementable as a layer between the EventStore models and the RelayPool. Applesauce contributors have hinted at this (the `loadBlocksFromFilterMap` per-relay caching at `timeline-loader.ts:269-281` is a poor man's version) but it's not architecturally present.

### B4. Per-relay capability negotiation

**Applesauce:** `RelayPool.group()` treats all relays uniformly. No NIP-11 introspection, no NIP-77 support probe, no per-relay limits awareness. The user must know which relays support what.

**NMP spec (M4):** "Per-relay capability negotiation (probe for NIP-77 support; cache result)."

**Gap:** A `RelayCapabilities` cache layer keyed by URL, populated lazily on first connection, persisted across runs.

### B5. Web-of-Trust scoring as a signal everywhere

**Applesauce:** `selectOptimalRelays` (`helpers/relay-selection.ts:7-11`) accepts a `score(relay, coverage, popularity)` callback. That's the only WoT hook, and it's purely caller-driven. No WoT computation in-framework, no WoT-aware filter/feed/loader.

**NMP spec:** WoT is a first-class layer per `aim.md` and `product-spec.md` (M13).

**Gap:** WoT graph builder + scoring + integration into outbox selection, feed ranking, and relay choice. Applesauce gives you the injection point on outbox; everything else NMP must build.

### B6. (DEFERRED to post-v1) Wallet / NWC / NIP-60 integration

Applesauce has `applesauce-wallet` and `applesauce-wallet-connect` as separate packages. NMP defers wallet entirely to post-v1 per `docs/plan/scope-adjustments-2026-05-18.md`. Worth noting they share the EventStore + factories + signer abstractions — when NMP comes back to wallet, the integration patterns will be there to reference.

### B7. Native platform signers (iOS Secure Enclave, etc.)

**Applesauce:** Has `AndroidNativeSigner` (Capacitor) and `AmberClipboardSigner` (intent URI). No iOS Secure Enclave signer, no macOS Keychain signer, no Windows Credential Vault signer.

**NMP spec:** Multi-platform native by definition.

**Gap:** NMP's signer set must include platform-native paths. The contract (`ISigner`) is right; the implementations don't exist in Applesauce.

### B8. Provenance set semantics on stored events

**Applesauce:** `helpers/relays.ts` provides `addSeenRelay`/`getSeenRelays` (cited at `event-store.ts:23, 192-196`). Events accumulate a "seen on these relays" set as they arrive from multiple relays. But this is informational — there's no API for "give me events that arrived from at least 2 relays."

**NMP spec (`plan.md`):** "an event arriving from 3 relays appears once in the store with all 3 relays in its provenance set." Implied: query by provenance is possible.

**Gap:** Querying by provenance count is not in Applesauce. NMP can build this on top of the seen-relay set; index it in the persistent store.

### B9. Bug-extinction tests as a first-class artifact

**Applesauce:** Tests exist per-package in `__tests__/`. Some have regression-test-explicit names (e.g., the `loadBackwardBlocks` test added with `b03c0d96`). But there's no global "bug-extinction" test suite that asserts known-historic bugs cannot recur.

**NMP spec:** Named bug-extinction tests are a phase gate.

**Gap:** NMP must maintain a `tests/bug_extinction/` directory with one test per historical bug, named after the bug (e.g., `bug_01_stale_replaceable.rs`). The 22 commits in `gotchas.md` are starter material.

### B10. LLM-friendly contract docs per view kind

**Applesauce:** `AGENTS.md` exists with documentation conventions. The packages are well-documented, but there's no machine-checkable contract per view kind that an LLM (or human) could read and implement a new one against.

**NMP spec:** "a developer or LLM given only docs implements a new 'hashtag screen' view kind in ≤ 1 hour, with no edits to `nmp-core`."

**Gap:** NMP needs a contract spec per view kind (inputs, outputs, lifecycle, claim semantics, error states) that is the authoritative reference. Applesauce's docs are descriptive, not prescriptive.

## C. Implementation tasks worth queueing separately

These are large enough to be their own tickets, not subtasks of M2:

### C1. NIP-77 negentropy + durable watermark planner integration (B2)

M4 in the ladder. Applesauce's negentropy lives in `applesauce-relay/src/negentropy.ts`; the **integration** with timeline loaders and the EventStore is the work. Estimated scope: medium-large.

### C2. Filter-level subscription coalescing (B3)

A new layer between typed views and the RelayPool. Inputs: N typed view subscriptions. Outputs: minimal set of REQ filters covering all views, fan-out to consumers. Tricky correctness: filter merge has to preserve per-consumer semantics. Estimated scope: medium.

### C3. iOS/macOS Secure Enclave + Keychain signer (B7)

Mirror `AndroidNativeSigner` for the Apple stack. Specific to NMP. Includes biometric unlock UX. Estimated scope: medium per platform.

## D. Top 3 missing features ranked for impact

If only three of these get separate tickets in M2's vicinity, pick these:

1. **B1 (outbox-by-default on publish)** — it's the spec's strongest differentiator from Applesauce and not free to bolt on later because every publish call site would need rewriting.
2. **B3 (filter-level coalescing)** — without it, M2's "tens of views on screen" target will pummel relays. Better to build the planner with coalescing from the start than to retrofit.
3. **B9 (bug-extinction test suite)** — the cheapest of the three and the biggest leverage. Every bug in `gotchas.md` gets a Rust test by name. Each test prevents one class of recurrence forever. Start the file the same week you start `nmp-core`.
