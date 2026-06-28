# App Model

An app has a Rust composition root. The composition root installs the mandatory
substrate and then opts into named reusable NMP features and app-owned product
features. The preferred production shape is a builder or explicit registration
expression where substrate, storage, policy, and feature opt-ins are visible:

```rust
let app = NmpApp::builder()
    .storage_path(policy.storage)
    .with_substrate(policy.substrate)
    .with_nip02_follow_list()
    .with_nip51_lists()
    .with_nip50_search(policy.search)
    .with_nip29_groups(policy.groups)
    .with_home_feed(policy.home_feed)
    .with_app_feature(highlighter_app::feature(policy.highlighter))
    .build();
```

The exact names above are illustrative. The rule is not illustrative:
`register_defaults()` is rejected as production app architecture. Product apps
show the substrate and feature methods they install. A typestate or equivalent
builder is attractive because it can require storage and substrate before start,
make the app immutable after build, and remove idempotency bugs from repeated
registration. If a monolithic preset survives temporarily, it is a tutorial or
migration shim with explicit callers and a deletion target, not the architecture
real products should copy.
For a migration shim, the bar is higher than "maybe a consumer exists": name the
live consumers, support window, owner, and deletion/formalization gate. If those
cannot be proven against current call sites, the preset is deleted rather than
documented as a compatibility surface.
Known live surfaces to classify include `nmp-defaults` itself, `nmp-cli`
templates/help, browser builder `start()` composition, `nmp-gallery`'s
composition root, examples, docs, and downstream app roots. The future doc
cannot simply say "rejected" while those surfaces keep teaching it as the normal
production path; each surface must migrate to explicit feature composition or be
labeled tutorial/migration with the support window and deletion gate above.

`nmp init` should teach production architecture by default: explicit feature
composition and policy builders. A separate tutorial preset can exist only when
the generated text labels it as tutorial/sample convenience and points production
apps at explicit feature composition.

The scaffold gates must flip with this design. Production scaffold tests reject
hidden `register_defaults()` and `declare_consumed_projections` teaching paths.
Tutorial scaffold tests may allow a preset only when the generated text labels
the preset as tutorial/sample convenience and the production path is separately
tested.

The substrate is the correctness floor: actor, store, indexes, signer ports,
capabilities, planner, publish engine, and typed update delivery. Feature bundles
sit on top of it.

## Feature Bundles

A feature bundle is not an open screen. It installs reusable capability:

- read/query descriptors;
- typed projection producers;
- actions and reducers;
- event draft builders;
- parsers and protocol validation;
- capability needs;
- protocol-owned state;
- publish route policy where the protocol owns it.

NMP feature bundles provide reusable Nostr mechanisms: NIP-02 follows, NIP-29
groups, NIP-17 DMs, NIP-65 routing, profile refs, event refs, search, generic
thread reduction, generic live counts, and publish policy.

App feature bundles provide product behavior that is not reusable Nostr
infrastructure: Highlighter capture/OCR/share queues/article chrome, podcast
playback/downloads/feed fetching/transcripts/agents, or gallery showcase
catalog state. They use NMP features, but they do not become NMP crates unless
the mechanism is useful to other Nostr apps.

Home feed is a composition proof case, not a privileged framework feature. A
microblog app may install a home-feed bundle that uses NIP-02 follows, NIP-65
outbox routing, profile refs, event refs, ranking, mute policy, and app-owned
fallbacks. Those reusable mechanisms belong in NMP; the product meaning of
"home" belongs in the app or a reusable feed crate only if it is genuinely a
generic Nostr feed mechanism.

The app crate is also the right owner for cross-protocol product composition.
For example, publishing a highlight and then sharing it into a NIP-29 room is
Highlighter behavior. The highlight feature and NIP-29 feature do not need to
import each other's domain types.

Custom app features should not require a framework PR. An app Rust crate can
define a typed session descriptor, output schema, reducer, generated adapter
contract, actions, event builders, and capability needs for its own domain, then
compose those with NMP protocol features. What it must not do is push raw relay
subscriptions, projection declarations, tag mutation, or publish routing into
the native shell just because the feature is app-specific.

This should reduce NMP, not grow it. When a downstream app exposes a missing
piece, the first question is whether NMP lacks a reusable Nostr mechanism or the
app lacks an app-owned Rust feature. If the behavior is podcast playback,
Highlighter capture, gallery showcase state, OCR, catalog search, local agents,
or another product domain, keeping it in the app crate is the simpler
architecture. NMP crates should shrink toward reusable protocol/runtime
mechanisms and delete framework doors that exist only because app crates could
not previously define typed sessions/actions cleanly.

This is the answer to the NDK comparison in #2313. NMP should feel like a
one-call subscribe from Swift, Kotlin, TypeScript, or TUI after the app Rust
crate has defined the session, but the production model is not arbitrary
shell-authored Nostr subscriptions. If a product needs "kind 999999 from these
authors" or "events from this relay-pinned group," the app or protocol Rust
feature owns that descriptor once, and generated host APIs expose the pleasant
open/close/render surface to every shell. Shell-only raw streams stay
diagnostic, test, export, prototype, or migration tools; they are not the
architecture for shipped product behavior.

## Composition Gates

The composition root should make these gates explicit:

- `substrate`: actor, store, planner, signer ports, capability registry, and
  update delivery.
- `protocol features`: reusable Nostr read/write/query/publish behavior.
- `app features`: product state, product actions, app-owned projections, and
  capability needs.
- `output contract`: typed outputs the shell may render or cache for rendering.
- `capability contract`: native/web capabilities Rust may request and the raw
  results the shell reports back.
- `client identity`: one declared app identity such as name/version/handler that
  feeds User-Agent and opt-in NIP-89 client tags through one Rust-owned outbound
  finalization path.

This keeps framework defaults useful without hiding the product architecture.
Feature-install helpers should live in feature composition crates such as
defaults, runtimes, protocol crates, or app crates, usually as builder extension
traits or explicit registration helpers. They should not become a pile of
unrelated methods on `nmp-core::AppHost`.

Composition should be idempotent where practical and explicit across browser,
native, TUI, and test roots. A browser `start()` path should not silently install
a different product architecture from the native root.

Explicit composition still needs observability. Rejecting hidden
`register_defaults()` must not delete the useful part of ADR-0049: an app should
be able to inspect what installers ran, what they registered, what they skipped
or yielded to app policy, and which capability/runtime requirements remain
unsatisfied. The difference is that the ledger explains an explicit composition
root; it is not a substitute for reading a magic preset.

`nmp-defaults` is a reusable composition library, not a leaf app. It may provide
generic routing, mailbox, parser, signer, and publish installers, but it must not
own operator policy such as seed follows, bootstrap relay lists, app relay
brands, signer permission defaults, or product onboarding choices. Leaf app Rust
crates provide that policy explicitly, preferably through typed builders that
make the "with policy" versus "without policy" decision visible at compile time.

Client identity follows the same single-source rule. An app declares client
identity once in its Rust composition root. NMP can derive User-Agent headers and
optional NIP-89 client tags from that one declaration during outbound
finalization. Native shells do not maintain parallel client-label, version, or
tag tables, and protocol crates do not hard-code product identity.

Generated catalogs and manifests follow the same rule. Known signer apps,
signer capabilities, Android package/query declarations, iOS URL-scheme plist
entries, generated TypeScript relay/config tables, release manifest entries, and
similar platform-visible catalogs must have one Rust or manifest system of
record. Native/web files may contain generated artifacts, not independently
maintained policy tables. A parity gate that compares Swift to Kotlin but not
back to the Rust/catalog source is not enough; drift prevention must point at the
single writer.

Generated app-feature APIs mean typed action, output, runtime, and capability
adapters. They do not mean resurrecting generated per-app framework composition
or hiding product policy inside generated native glue. If a generated API
constructs Nostr events, chooses relays, signs, publishes, parses protocol tags,
or owns durable product state, it is the wrong boundary unless the Rust app or
protocol feature is the actual owner and the generated surface is only transport.

FFI binding strategy is an implementation lane, not the architecture itself.
FlatBuffers/update frames can remain the update payload transport while typed
sessions/actions and generated adapters improve the public model. UniFFI,
C-ABI, JNI, and browser-worker bindings are allowed to change only when the
change deletes hand-written drift or narrows a public door. Targeted Android
binding generation may be worth pulling forward if it retires duplicated JNI
work; a full iOS binding migration is a separate decision unless the ADR
explicitly reopens it. Binding churn that preserves the same old read/write
doors is not simplification.

## App Feature Runtime Contract

App-owned Rust crates may need runtime services that are not reusable Nostr
protocol machinery. Podcast playback, queueing, downloads, transcript work,
provider catalogs, STT/TTS, local agents, widgets, AppIntents, CarPlay, remote
commands, Live Activities, Handoff, and import/export flows are valid app
features. They should have generated app-feature APIs or typed capability
requests, not be forced through NMP protocol crates and not be mistaken for
legacy FFI just because they are app-specific.

The contract is still Rust-owned:

- app Rust owns durable state, policy, reducers, shutdown, scheduled work, and
  injected clocks;
- native/web executes platform capabilities and reports raw results;
- generated app-feature APIs are typed and versioned;
- capability result channels re-enter the Rust reducer path;
- event-producing operations still use the typed action/publish doorway.

Native mirrors are allowed only when they are capability or rendering
mechanics, not durable product truth:

| Native/local surface | Allowed role | Forbidden role |
|---|---|---|
| Widget/App Group snapshot | last Rust-emitted widget frame for WidgetKit | source of playback queue, episode state, relay state, or publish status |
| `MPNowPlayingInfoCenter` / remote command state | OS media surface fed from Rust playback state | independent playback state machine or queue owner |
| ActivityKit/Live Activity state | executor-side copy needed by ActivityKit throttling/lifecycle | decision about whether an activity should exist or what episode is current |
| `NSUserActivity`/Handoff payload | OS handoff payload built from Rust semantic state | second navigation/playback source of truth |
| image/profile/render caches | bounded render cache for already-projected data | protocol cache, profile truth, relay policy, or ref lifecycle owner |
| secure storage/keychain | secret-bearing capability store | signer policy, permission model, or publish continuation owner |
| native app database/UserDefaults | migration/import/export staging or render cache with Rust owner | durable product store for Nostr/account/playback/feed facts |

Headless and OS-owned surfaces are not exempt. A widget, AppIntent, CarPlay
scene, remote command, Live Activity, extension, or suspended-process resume may
open app-lifetime/service sessions or submit capability results, but it must not
own a parallel playback queue, signer state, relay policy, or publish result
model.

This distinguishes legitimate app runtime surface from forbidden protocol
escape hatches. A Whisper upload, playback seek, provider-key read, or local
agent tool call may be an app-feature API. A native-built Nostr event,
native-chosen relay route, or native-owned publish status is not.

## Developer Mental Model

An app developer should know:

- which features the app installs;
- which feature/ref session a screen, component, widget, or service opens;
- which typed output that owner renders;
- which draft builder or action builder constructs an event or product action;
- which signer should sign when the active account is not enough;
- whether publishing uses automatic routing, protocol-pinned routing, or an
  explicit relay override;
- when to close query handles.

For custom product behavior, the developer should expect one Rust definition
plus generated shell calls:

```text
define app/protocol Rust session once
  -> generate Swift/Kotlin/TypeScript/TUI open/close/action helpers
  -> shell opens the typed session and renders typed output
```

That is the "door." It is not a framework PR for every feature, and it is not a
native callback that streams arbitrary raw events.

An app developer should not need to know:

- projection tiers;
- `SnapshotRegistry`;
- muted observers;
- replay shapes;
- raw relay fanout;
- NIP-65 mailbox lookup internals;
- cache/store replay mechanics;
- FlatBuffers sidecar registration;
- snapshot tick reconcilers;
- publish retry classification;
- native-side relay routing.

## Shell Responsibilities

Native and web shells have three jobs:

- render the typed state Rust gives them;
- execute capabilities requested by Rust;
- hold ephemeral presentation state such as focus, animation, scroll affordance,
  or transient sheet state.

The discriminating test stays simple: if a second platform would have to
reimplement the behavior to stay correct, the behavior belongs in Rust.

Rust outputs carry semantic facts, not presentation formatting. A projection may
emit raw signer kind/state tokens, pubkeys, timestamps, event refs, route status,
and domain state. Shells may choose labels, icons, colors, typography,
truncation, local date formatting, and animation as long as those choices do not
change behavior, routing, policy, identity, replay, sorting, or protocol meaning.

Projection caches generated for Swift, Kotlin, TypeScript, C, or TUI are not
product state. They are render adapters for typed Rust outputs. A shell may keep
row-delta caches for profiles, event refs, playback rows, or domain slices when
the adapter owns their lifecycle and Rust remains the durable source of truth.

Connectivity follows the same rule. Native may report raw platform facts such
as `NWPath` state, metered network flags, background mode, or reachability. Rust
owns app policy such as Wi-Fi-only publishing, relay-pool pause/resume, retry
eligibility, and whether pending work may continue on the current connection.

## Downstream App Shape

`nmp-gallery` mostly exercises NMP features directly across iOS, Android, TUI,
desktop, and web in this checkout. It needs component-scoped profile and event
refs, generated ref caches, embed resolution, auth/signing component coverage,
and no ref/projection retry timers. Its showcase relays are sample
data/bootstrap policy for the gallery app, not framework defaults for NMP.
Today's gallery bridge still teaches old architecture through
`register_defaults()` / `consume_all_builtin_projections()` and platform-local
URI/ref adapters; the migration proof is not complete until those are replaced
or explicitly scoped as tutorial/showcase compatibility.

Highlighter needs NMP features plus Highlighter-owned bundles for capture,
share, article reading, curation, room chrome, comments, feedback, and podcast
surfaces. Its native shells should execute Keychain, OCR, share extension,
camera, AVPlayer, NIP-55, and connectivity capabilities, then report raw
results to Rust.

Podcast Player needs NMP features plus podcast-owned bundles for playback,
queue, downloads, feed subscription, OPML/import, transcripts, widgets,
Blossom-backed publish flows, and agent behavior. RSS, playback, queue, and
transcript logic must stay in the app Rust crate.

Secondary downstream apps such as 29er and Olas are useful sanity checks but not
permission to add product nouns to NMP. 29er should prove NIP-29/raw-event/group
tree behavior through reusable protocol features and an app Rust core; Olas
should prove picture-event, WoT, and image-feed mechanisms without moving Olas
ranking or onboarding policy into framework crates.
