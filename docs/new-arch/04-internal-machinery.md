# Internal Machinery

The developer-facing shape is small, but NMP still owns the machinery needed to
make it correct across platforms.

## Read Pipeline

For reads, NMP internally:

```text
opens a live query
  -> compiles source expressions into interests/dependent interests
  -> plans relays with outbox routing or explicit relay pins
  -> records reverse admission/wake indexes for the session
  -> replays cached/store data
  -> admits matching live events into Rust-owned read state
  -> emits bounded typed projections
  -> tears everything down when the handle closes
```

Protocol crates own the meaning of protocol queries. The live query machinery
owns lifecycle, replay ordering, dependency tracking, and teardown.

The current `ObservedProjection` path is the closest internal primitive and
should be reused. The goal is not a parallel read path or a broad new engine. It
is to prove that a small typed descriptor can compile into the safe recipe
feature authors need.

Dynamic source implementations should be consolidated only as far as their
semantics truly match. Start with one private shape reconciler around
observed-projection open/close, keep source-specific reducers local, and promote
a general source-reduction core only after multiple source families prove they
share the same diff, fail-closed, teardown, and dependent-interest rules.

Event-to-session admission is a protected invariant. Live sessions need
Rust-owned reverse indexes or wake queues that map an ingested event to the
sessions and outputs it can affect. The destination is not a flat filter-poll
loop over every active interest and not a native-owned refresh trigger. Store
ingest, relay delivery, source changes, and mailbox changes should enqueue
bounded, coalesced work for the owning session/output.

## Write Pipeline

For writes, NMP internally:

```text
receives a typed write intent
  -> constructs or finalizes unsigned event data through the owning feature
  -> signs with the selected signer through the capability/signer port
  -> stores the signed event when appropriate
  -> plans publish relays
  -> dispatches to relays
  -> records publish status and errors as state
  -> updates projections through normal ingest/store paths
```

Protocol crates own protocol-specific draft and route policy. App crates compose
protocol flows when product behavior spans protocols.

Existing one-door pieces should remain the substrate: dispatch envelope, action
modules, signer port, publish policy, publish command, publish engine, local
store, retry/cancel controls, and typed status outputs. `EventDraft` and
`PublishContext` name invariants across those pieces, not automatic new types or
permission to add a second publish stack.

## Actor And Store Rules

The actor remains the single writer. New nondeterministic inputs enter as typed
actions, capability results, or injected seams. Reducers remain replayable from
message history.

The event store and indexes stay inside Rust. Shells receive typed projection
state, not raw store handles. Raw signed events may be exposed through explicit
inspection/export features, but not as the default app data path.

Projection delivery must preserve hard invariants: typed schema identity,
stale-frame drops, clear/tombstone semantics, full-pull cold start paths,
bounded replay, deterministic row ownership, and one merge contract per output.
Hiding projection tiers from app developers cannot mean deleting these executor
guarantees.

Wake/admission indexes are in the same category. They are internal machinery,
but they are not optional if the simpler descriptor model would otherwise
degrade to polling, broad scans, or missed hydration. The ADR must name which
event/store/source changes wake each session family and how stale wakes are
deduped or dropped.

The mechanisms are not automatically sacred. FlatBuffers, sidecar registration,
projection manifests, output namespaces, incremental apply, and generated host
adapters each need an invariant and a rejected simpler alternative. Keep them
where they are the cheapest way to preserve cross-platform decode, stale-frame
protection, render-cache correctness, or wire/CPU bounds. Collapse or delete
them where session-scoped output demand gives the same guarantee with less
surface area.

## Existing Primitive Mapping

The new model should reuse or retire current primitives deliberately:

- `LogicalInterest` and the interest registry remain acquisition internals.
  Product apps should not compose them directly.
- cache-serve and store replay remain hydration internals behind observed
  sessions.
- `ObservedProjectionRegistrar` is the replay-before-live primitive to prove
  descriptor sessions against first.
- dependent interests are the current dynamic-source substrate; they either stay
  internal or collapse into a smaller reconciler/source core.
- `SnapshotRegistry`, `DeclaredProjections`, typed sidecars, incremental apply,
  and `UpdateFrame` remain executor machinery until session-scoped demand proves
  which pieces can be deleted.
- `ActionModule`, `DispatchEnvelope`, `PublishTarget`, publish policy, signer
  ports, and the publish engine remain the one write doorway.
- explicit relay seams remain audited route policy, not native relay choice.

## Complexity Justification Gate

No internal mechanism in this proposal is accepted just because it already
exists. Before implementation, each mechanism must be red/blue-team reviewed:

```text
mechanism
  -> simplest plausible alternative
  -> invariant defended by the complex design
  -> evidence that the simpler alternative fails
  -> consolidation/deletion opportunity
  -> public/private/compatibility classification
```

The default outcome should be deletion or consolidation unless the mechanism
protects one of these invariants:

- replay correctness: cached data is seen before future live data;
- privacy: private events cannot leak to public relays;
- routing correctness: outbox, inbox, host relay, and explicit routes are not
  guessed by shells;
- single-writer state: product facts have one Rust owner;
- cross-platform parity: Swift, Kotlin, TypeScript, TUI, and browser shells do
  not reimplement protocol behavior;
- bounded reactivity: hot paths do not poll, wake unnecessarily, or ship
  unbounded snapshots;
- wake correctness: event ingest and dependency changes reach affected sessions
  without broad polling or native refresh code;
- downstream necessity: `nmp-gallery`, Highlighter, or Podcast Player cannot
  express a real flow with the simpler design.

Examples of complexity that must be justified, not inherited:

- whether `ObservedProjection` needs its current shape or can be a smaller
  replay-before-live helper under sessions;
- whether `ReducedSource`, dependent interests, pointer sources, and active
  observed projection controllers can collapse into one source-reduction core;
- whether projection tiers need to remain as executor internals, and whether the
  app-facing output manifest can hide them completely;
- whether opening a session can be the scoped output declaration, leaving global
  declared projections only for always-on app chrome or compatibility;
- whether generated host adapters are enough, or whether hand-authored row
  caches can conform to one shared merge contract without becoming product
  state;
- whether `PublishContext` is a real missing type or only a naming layer over
  existing publish target, behavior, command, and policy data;
- whether off-actor feed mutation and feed-render memo/provider machinery can be
  deleted by routing viewport/load-older through actor/session actions;
- whether live counts can remain typed projections instead of a dedicated
  primitive;
- whether compatibility APIs such as raw `open_interest` need to remain public,
  and if so which scopes may call them.

The ADR should record rejected simpler alternatives. If the only defense is
"this is how the current code works," the mechanism is not justified.

## Stress Tests

The design must survive these cases before implementation starts:

- **NDK-style subscribe:** a developer asks for kind:1 notes by a dynamic author
  set and receives one typed output while NMP splits the work across author
  relays, cache replay, live relays, and later mailbox changes.
- **NIP-29 group:** a group view is host-relay-pinned, admits only events with
  valid relay provenance/context, fails closed without group context, and tears
  down with the handle.
- **Component embed storm:** many visible event bodies create profile/event/embed
  claims; repeated refs share demand; closing components releases demand and
  generated/host caches clear without stale rows.
- **Account switch:** active-account sessions replan sources and routes without
  native timers, wildcard fallthrough, or stale projections from the old account.
- **Malformed or stale frame:** generated/host merge code applies updates
  transactionally; decode poison cannot corrupt the baseline; clear/tombstone
  frames remove rows.
- **Read-your-writes:** a locally signed event enters the store/ingest path before
  relay delivery, so projections update through the same reducer path as relay
  events, not native optimism.
- **Private routing:** NIP-17 and similar private writes never fall back to public
  outbox routes when recipient inboxes are unknown.
- **Downstream proof:** Highlighter web, Highlighter iOS, Podcast Player, and
  gallery can all express their flows without native-owned protocol policy or
  app-domain nouns in NMP crates.

## Timer Boundary

The no-polling rule applies to product state reconciliation, session routing,
source tracking, relay planning, projection refresh, and reducer correctness.
Those must be event-driven.

Capability executors and presentation affordances may still use timers when the
platform API is inherently sampled or throttled: media playback position,
download progress throttling, speech/audio progress, animation affordances, or a
"copied" label clearing after a delay. Those timers report raw capability or
presentation facts; they do not decide durable state, open relay demand, refresh
projections, or repair missed reducer work.

Every remaining tick observer or timer that touches reducer/session state needs
an explicit invariant, owner, and deletion or formalization decision.

## Non-Goals

- Do not expose a generic raw event callback as the main app API.
- Do not make `open_interest` the app read model.
- Do not let native compute dynamic source sets, route relays, or mutate event
  tags for protocol correctness.
- Do not collapse every protocol-specific publishing rule into `nmp-core`.
- Do not turn `LiveQuery` into an object that owns protocol meaning. Protocol
  and app crates own meaning; live query machinery owns lifecycle.
- Do not present this document as shipped API before ADR and migration work land.
- Do not move product domains into NMP crates because one downstream app needs
  them.
- Do not let compatibility paths remain public teaching examples once the typed
  lifecycle exists.

## Implementation Plan

Each phase must leave the repo shippable and reduce at least one public concept,
duplicate lifecycle recipe, or hidden desync state.

## New-Code Rules

Once the ADR accepts this direction, new code must obey these rules even before
the full migration is done:

- Product screens and shells must not call raw `open_interest` or equivalent
  relay-subscription doors.
- Product reads must enter through a typed feature/ref/session descriptor or a
  named substrate/protocol-internal/diagnostic/test/migration scope.
- New app-facing examples and templates must not teach `register_defaults()` as
  the production mental model.
- New output keys must declare one owner, schema version, merge contract, and
  collision behavior.
- New dynamic sources must specify empty-source behavior and must fail closed
  unless the feature declares an explicit fallback.
- New timer/tick logic must prove it is capability sampling or presentation
  affordance, not reducer/session/projection reconciliation.
- New session families must declare their event/store/source wake conditions and
  reverse-index or queue strategy.
- New event-producing writes must enter through typed actions/builders and
  publish status, never native JSON publish paths.
- New explicit relay writes must preserve route provenance.
- New app-feature APIs are allowed for app runtime capabilities, but they must
  be typed/versioned and may not own Nostr protocol policy.

**P0: Baseline and freeze old patterns.**
Inventory public read/write doors and duplicate lifecycle recipes in
`nmp-core`, `nmp-defaults`, `nmp-native-runtime`, `nmp-browser-runtime`,
`nmp-ffi`, `nmp-codegen`, `nmp-gallery`, Highlighter, and Podcast Player. Add or
extend grep gates so new product code cannot add raw `open_interest` app reads,
new tick reconcilers, native relay timers, or native publish/tag construction.
Classify every existing raw read/write door as substrate, protocol-internal,
diagnostic/test, migration shim, or product API to remove/formalize.
Record baseline counts for: raw `open_interest` public callers, `nmp init`
teaching `register_defaults()` as production architecture, tick observers,
`declare_consumed_projections` app-facing docs, duplicate explicit-relay publish
representations, native-owned relay/policy/tag construction, hand-authored
projection merge caches, downstream direct NDK/FFI publish paths,
`@nostr-dev-kit/ndk` product fetch/sign/publish usage, native-owned network
policy such as `hl.network.wifi_only`, shell-side `tagsJson`/`p:`/`e:`/`a:`
protocol parsing, fire-and-forget event writes, and public `nmp_app_open_feed`
or `nmp_app_open_interest` app doors. The first PR should add ratchets so these
counts cannot grow.
Gates: `cargo test -p nmp-testing --test doctrine_lint_smoke` and
`cargo test -p nmp-testing --test feed_public_surface_retired`.

**P1: Prove a descriptor over existing safe machinery.**
Add the smallest private descriptor facade that compiles into
`ObservedProjection::from_shape`, `OpenObservedInterest`, replay limits,
consumer ids, relay pins, and close. Do not add a new lifecycle engine. Use one
real session as proof. Acceptance: replay-before-live and close-both invariants
still pass in observer replay, descriptor idempotence, reducer parity, and
`nmp-defaults` feed open/close tests; no new public API is taught. The proof
must cover relay pins, cache replay, source changes, open/close teardown, and at
least one component-ref/gallery embed path under the same session lifecycle.

**P2: Extract shape reconciliation and delete tick use where events exist.**
Consolidate the duplicated open/close-on-shape-change controllers behind one
private reconciler. Migrate active-account, browser feed, native feed, and
pointer-source controllers only where their semantics match. Delete
`register_snapshot_tick_observer` usage for identity/source/mailbox/refcount
changes that already have event hooks. For each remaining tick observer, either
add the missing explicit event source or document a bounded actor-scheduled
invariant with a staged deletion gate. "Compatibility" alone is not a reason to
keep it. Use the existing cache-serve wakeup pattern as the reference: live/store
events enqueue coalesced work, and actor ticks only drain already-declared work.

**P3: Make scoped session demand own scoped output demand.**
Prove that opening a session can declare its typed output. Keep
`DeclaredProjections` only for always-on app chrome, compatibility, or measured
wire/CPU wins. Acceptance: `public_typed_projection_decode` still proves
external decode; generated adapters still handle full, delta, clear, stale-frame,
baseline, transactional merge, and D6 poison semantics. `declare_consumed_projections`
must stop being taught as the app manifest for screen/session outputs.

**P4: Migrate component refs and gallery embeds first.**
Move `ProfileRef`, `EventEmbed`, URI decoding, relay hints, embed envelopes, and
row-delta caches behind typed sessions and generated adapters. Delete shell
retry timers and duplicated URI decoding where the Rust path owns it. This is
the first cross-shell proof because gallery exercises Swift, Kotlin, TUI, and
desktop rendering without app-domain policy. Include gallery auth/signing
component coverage; the live checkout does not contain a gallery web root, so
web proof must come from another app/runtime. Fix gallery's timer-based copy
affordance before treating the registry as a copyable downstream template.

**P5: Migrate dynamic and composite reads only after P1-P4 hold.**
Feed, group, search, pointer-source, thread refs, and live-count outputs move to
the same descriptor model. Source-specific reducers stay local unless a shared
core deletes duplication. Acceptance: feed reduced-source tests, real-relay
reduced-source tests, group/search tests, and empty-source fail-closed tests
cover account switch, source change, relay pin, cache replay, and teardown.

**P6: Collapse write variants by invariant, not by new names.**
Generated builders keep using `DispatchEnvelope` and `ActionModule`. First try
to unify `UnsignedEvent`, `UnsignedEventToRelays`, pre-signed publish, signer
selection, `PublishTarget`, correlation id, and policy validation without adding
new public types. Add a named draft/context type only if it deletes branching or
duplicate route/privacy/protocol state. Gates: publish policy, D10 private
routing, signer continuation, generated builder round-trip, and action-result
tests. Explicit relay cleanup is part of this phase: delete dead explicit-target
fields or route every explicit publish through one canonical internal seam with
one attribution/status model. Route provenance is the critical missing invariant:
manual explicit relay, NIP-29 host pin, verified NIP-17 inbox, and
external/verbatim publish must not collapse into an indistinguishable
`Explicit` bucket. Tests must prove generic raw publish cannot accidentally
bypass NIP-29 `h`-tag/group-route proof or NIP-17 verified-inbox policy, and
that remote signer continuations preserve route provenance plus correlation id.

**P7: Prove downstream apps before declaring the architecture final.**
Highlighter must express home feed, room chat, search, comments, share-to-room,
capture, feedback, signer flows, artifact share, article lookup, and room
discussion through app-owned Rust bundles and NMP runtime dispatch. Direct web
NDK relay/filter/tag/sign/publish paths, Swift rich-text protocol projection,
Swift-owned sync policy, Swift `tagsJson` parsing, and fire-and-forget writes
must be removed or formally rejected by the ADR. SSR-only fetches and migration
shims must be labeled as such; they cannot be the product runtime model. The ADR
must decide whether Highlighter web is in the NMP target runtime, a labeled
SSR/migration exception, or deliberately out of scope.

Podcast Player must express playback, queue, feed subscription, NIP-F4, Blossom
publish, explicit write relays, widgets, settings actions, signer runtime, and
feedback without moving podcast nouns into NMP. Bespoke durable FFI, silent
compatibility paths, stale Swift-store docs, and `nmp-signer-broker` pinning
must converge on generic typed dispatch/projection/capability seams and the
current NIP-46 runtime direction. Widget extensions, AppIntents/Siri, CarPlay,
remote commands, Live Activities/Handoff, and cold/suspended process behavior
must prove app-runtime/service sessions rather than native-owned state.

`nmp-gallery` must express component refs, embeds, auth/signing components, and
renderer caches without shell protocol state or timer-based state clearing. It
becomes the conformance fixture for refs/profile, refs/event envelopes,
copied/native components, typed dispatch, and renderer caches only after those
constraints hold.

Any downstream flow that requires native-owned policy or a bespoke framework
door is a design failure, not downstream migration debt.

**P8: Correct durable docs and delete compatibility teaching paths.**
Update architecture API-surface docs, overview/DX docs, builder-guide pages for
subscription planning, publish and ledger, walkthroughs, action-triggered
subscriptions, ADRs, wiki pages, and any episode/transcript-derived teaching
material that currently presents projection tiers or defaults as app-facing
concepts. The `nmp init` template must be corrected according to the ADR
decision: either production scaffold with explicit feature composition and
policy builders, or clearly labeled tutorial preset. Compatibility APIs may
remain only with scope labels, doctrine gates, and deletion criteria.
At minimum, audit and rewrite stale guidance in `docs/product-spec/api-surface.md`,
builder-guide mental-model/codegen/walkthrough pages, subscription planning and
publish guides, wiki app-composition pages, and any generated template that
teaches `register_defaults`, `open_interest`, projection tiers, or
`declare_consumed_projections` as the normal product architecture.
Also audit wiki pages that still teach `nmp.feed.home`, generic defaults,
sidecar projection rituals, raw/pre-signed publish branches, or direct taps as
current architecture. They should either be corrected in place or explicitly
retired before this packet becomes durable documentation.

## Fitness Functions And Ratchets

P0 should convert the new-code rules into repeatable checks. The exact scripts
can change, but each check needs an owner, baseline, target, and enforcement
mode before implementation starts.

| ID | Rule | Baseline Source | Target | Enforcement |
|---|---|---|---|---|
| FF-001 | Product code does not add raw `open_interest` app reads. | grep public FFI/runtime/shell callers and builder-guide examples | count never increases; product callers trend to zero | doctrine lint or `nmp-testing` grep gate |
| FF-002 | `register_defaults()` is not the production app mental model. | `nmp-cli` templates, examples, builder guide, gallery/podcast composition roots | templates teach explicit feature composition or labeled tutorial preset | template test plus doc grep gate |
| FF-003 | App-facing docs do not expose projection tiers or `declare_consumed_projections` for screen/session output. | docs/product-spec, builder guide, wiki, templates | public docs teach typed outputs and session-scoped demand | docs lint grep gate |
| FF-004 | Product state reconciliation does not use snapshot tick polling. | `register_snapshot_tick_observer` call sites and downstream timers | reducer/session/projection tick users trend to zero or have explicit invariant | grep gate plus owner list |
| FF-005 | Dynamic sources fail closed. | feed/source/dependent-interest tests | every migrated source has empty-source and fallback tests | crate tests for source families |
| FF-006 | Output keys have one owner and collision behavior. | projection contract table, host-registered projections, built-ins | composition fails on unowned/colliding keys unless alias/replace is declared | registry test/codegen check |
| FF-007 | Generated/host caches share merge semantics. | Swift/Kotlin/TS/TUI ref caches and `projection_merge_cache` | full/delta/clear/stale-frame behavior covered for generated adapters | cross-language decode/merge tests |
| FF-008 | Explicit relay publishes preserve route provenance. | `PublishTarget::Explicit`, protocol plans, pre-signed publish APIs | manual, NIP-29, verified inbox, and imported/verbatim routes are distinguishable | publish policy and retry/resume tests |
| FF-009 | Private routes fail closed. | D10 tests, NIP-17 inbox tests, Marmot/private publish paths | no unknown-inbox fallback to public/outbox | `nmp-core`, `nmp-nip17`, doctrine tests |
| FF-010 | Downstream shell protocol policy decreases. | Highlighter NDK usage, Swift `tagsJson`, native Wi-Fi policy, Podcast signer inference, gallery URI parsing | counts do not increase; release gates drive them down | downstream grep gates or migration checklists |
| FF-011 | App-feature APIs stay typed and non-protocol unless event-producing. | Podcast STT/TTS/agent/provider APIs and generated FFI | app runtime APIs are classified; event-producing ones use typed publish | API-surface classification test |
| FF-012 | Clean-room app docs work without issue/wiki spelunking. | generated app plus builder guide | new app can open/read/write one feature with typed sessions/actions | walkthrough test or manual UAT checklist |
| FF-013 | Session wakes are indexed and event-driven. | cache-serve wakeups, logical-interest indexes, tick observers, downstream refresh pulls | no session family depends on broad polling or native refresh triggers | session wake/admission tests |
| FF-014 | Rust outputs semantic facts; shells format presentation. | signer labels, SF Symbols, short npubs, relative time, display strings in Rust projections | semantic tokens only in Rust outputs; presentation helpers stay in shells/TUI/test fixtures | grep gate plus projection review |

Useful baseline commands:

```bash
rg -n "nmp_app_open_interest|open_interest" crates apps docs --glob '!target/**'
rg -n "register_defaults|declare_consumed_projections" crates docs apps --glob '!target/**'
rg -n "register_snapshot_tick_observer|sleep|Timer|setInterval" crates apps /path/to/downstream --glob '!target/**'
rg -n "PublishTarget::Explicit|PublishRaw|UnsignedEventToRelays" crates --glob '!target/**'
rg -n "short_npub|format_ago|SF Symbol|status_label|display_label|avatar_initials" crates docs --glob '!target/**'
rg -n "@nostr-dev-kit|tagsJson|hl.network.wifi_only" /Users/pablofernandez/Work/hl --glob '!**/.git/**'
rg -n "Nip46|signer|dispatchSilent|snapshot\\(" /Users/pablofernandez/Work/podcast-player --glob '!**/.git/**'
```

Counts are not success by themselves. They are ratchets: they prevent new old
patterns while the milestone ladder deletes or privatizes the existing ones.

Baseline every architecture milestone with the existing gates. The file-size
hook applies to code and other enforced repo surfaces, not as a constraint on
the length of this design packet:

```bash
git status -sb
git diff --check
cargo test -p nmp-testing --test doctrine_lint_smoke
cargo run -p nmp-testing --bin doctrine-lint -- --workspace-d8
cargo run -p nmp-testing --bin doctrine-lint -- --workspace-native
cargo test -p nmp-testing --test doctrine_native_smoke
cargo test -p nmp-testing --bin doctrine-lint
bash .githooks/check-file-size.sh --from-ref <base-ref> --to-ref HEAD --baseline-ref <base-ref>
```

When touching actions, publish, FFI, or codegen:

```bash
bash ci/check-native-action-boundary.sh --self-test && bash ci/check-native-action-boundary.sh
bash ci/check-dispatch-envelope-gates.sh --self-test && bash ci/check-dispatch-envelope-gates.sh
cargo run --quiet -p nmp-codegen -- gen action-builders --platform ts --check --out web/packages/runtime-web/src/actionBuilders.generated.ts
```

Existing NMP tests that should become phase gates where relevant:

```bash
cargo test -p nmp-testing --test feed_public_surface_retired
cargo test -p nmp-testing --test public_typed_projection_decode
cargo test -p nmp-testing --test cache_serve_replay_fixtures
cargo test -p nmp-testing --test reduced_source_relay_e2e
cargo test -p nmp-testing --test m2_subscription_compilation_audit
cargo test -p nmp-testing --test m8_subscription_lifecycle
cargo test -p nmp-testing --test framework_magic_contract
cargo test -p nmp-testing --test nip17_dm_inbox_routing
cargo test -p nmp-testing --test dx_scaffold_gate
cargo test -p nmp-testing --test dx_login_timeline_gate
cargo test -p nmp-testing --test conformance_catalog_complete
```

Missing gates to create before declaring the design implementable:

```bash
cargo test -p nmp-testing --test architecture_surface_ratchet
cargo test -p nmp-testing --test live_query_descriptor_contract
cargo test -p nmp-testing --test projection_merge_contract
cargo test -p nmp-testing --test publish_route_provenance_contract
cargo test -p nmp-testing --test docs_architecture_teaching_ratchet
```

## Kill Criteria

Stop, redesign, or ask for a human decision if any of these become true:

- The first descriptor proof cannot sit on existing `ObservedProjection` /
  dependent-interest machinery without creating a second read lifecycle.
- Typed sessions reduce names but not the number of public concepts a product
  author must understand.
- Route provenance requires broad publish-context plumbing that adds more
  permanent concepts than it removes.
- A downstream app can express its real flows only by keeping native-owned Nostr
  policy, direct NDK subscriptions, native publish JSON, or app-domain nouns in
  NMP crates.
- Generated adapter/schema work cannot prevent cross-platform payload drift.
- Ratchets cannot be automated or reviewed cheaply enough to stop new
  old-pattern usage.
- Two milestones pass without reducing at least one measured old-pattern count
  or deleting one duplicated lifecycle recipe.
- The team cannot decide whether Highlighter web is target-runtime, SSR/
  migration exception, or out of scope.
- The team cannot decide whether `nmp init` generates production architecture or
  a labeled tutorial preset.
- The team cannot decide which downstream migrations are release gates versus
  follow-up issues.
- Podcast headless/runtime surfaces require native-owned state to work.
- The signer support matrix cannot converge on one Rust-owned status and
  continuation model.
- Explicit relay selection is meant to be a user-visible product affordance, but
  the product cannot specify its owner, audit text, and route guarantees.

## Fitness Checks

The destination is not reached until these are true:

- No public builder guide asks product apps to manually pair raw interest open,
  observer registration, replay, projection sidecar, and teardown.
- No production state reconciliation depends on snapshot tick polling.
- New app reads enter through typed session descriptors or named
  substrate/protocol-internal/diagnostic/test/migration acquisition scopes.
- Every dynamic source has deterministic diffing, explicit fallback policy, and
  no wildcard result from empty demand.
- Projection tiers are absent from app-facing docs and starter apps.
- Rendering row/delta caches implement one shared/generated merge contract.
  Hand-authored caches may exist only as thin, proven adapters; they cannot own
  product facts or independent merge semantics.
- Publish builders route through the existing typed action doorway and publish
  engine.
- Publish/action status is visible as typed Rust-owned output.
- `nmp-gallery`, Highlighter, and Podcast Player can express their current core
  flows without native-owned protocol state or app-domain logic inside NMP.
- Clean-room app docs can build a simple app without reading ADRs, issues, wiki
  pages, or old design chats.
- Every retained internal mechanism has a written invariant, rejected simpler
  alternative, and deletion/consolidation decision.
- Baseline counts for old patterns move down or stay flat behind ratchets; no
  milestone is allowed to add a second route, projection, or publish lifecycle
  while claiming progress toward simplification.

## Follow-Up ADR Decisions

The ADR for #2316 must settle:

- final names for `LiveQuery`, `LiveView`, `ProjectionSession`, or another term;
- whether the public app door is one generic `open_query` or typed per-feature
  open helpers backed by one descriptor model;
- which simpler solution was rejected, and what evidence proves the selected
  shape is not another convenience layer over the old lifecycle split;
- whether default public reads are always planned/outbox-routed unless a
  descriptor explicitly declares relay-pinned, private, or audited explicit
  routing;
- how app Rust crates define custom sessions, outputs, reducers, builders, and
  capability needs without changing NMP crates or moving protocol work to native;
- how existing feed, group, search, ref, and pointer-source sessions migrate;
- how projection producer ownership replaces public Tier-1/Tier-2 language, and
  how schema keys, owner/version metadata, collision failures, and alias/replace
  rules work;
- how event draft construction, signer selection, and publish routing are
  represented in generated builders;
- whether route provenance can be represented with existing publish fields or
  needs one narrow internal type;
- the supported signer matrix across local keys, NIP-07, NIP-46 browser/native,
  NIP-55-style platform signers, named product signers, agent signers, and
  imported pre-signed events;
- what doctrine lint blocks reintroduction of raw observer, tick, or polling
  recipes;
- how `register_defaults()` is positioned after explicit feature composition is
  the taught model;
- which compatibility APIs remain available and what scopes may call them;
- whether current `ReducedSource`/`open_feed` machinery is amended, renamed, or
  replaced by a smaller descriptor/reconciler model;
- whether `nmp init` generates a production scaffold or a tutorial preset, and
  how docs label that choice;
- whether Highlighter web is an NMP target-runtime migration gate, an SSR/
  migration exception, or out of scope for this architecture;
- how app-runtime/service sessions cover widgets, AppIntents, CarPlay, remote
  commands, Live Activities/Handoff, and suspended-process resume;
- which downstream app migrations are release gates versus follow-up issues.
