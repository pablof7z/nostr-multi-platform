# High-Level NMP App Architecture

NMP is a Rust-owned application framework for Nostr apps. A native or web app
should feel like it is asking for product-shaped things:

- "show an article feed";
- "show this group";
- "reply to this event";
- "publish this article to this group";
- "render the current publish status".

It should not feel like it is assembling relay filters, replay order, projection
sinks, signer parking, source replacement, route planning, local ingest, retry,
and teardown by hand.

## Authority And Document Roles

This is the canonical developer-facing overview for NMP's clean-break
architecture. Start here when you need the current builder model: app-owned
composition, typed read sessions, typed write workflows, runtime/capability
boundaries, and thin platform shells.

Document roles:

- ADR-0069 through ADR-0073 are the decision spine: they record the architecture
  decisions and ratchets behind this overview.
- `docs/new-arch/` is retired; do not use it as current guidance.
- `docs/aim.md` is the north star and foundation, not the detailed current
  architecture guide.
- Doctrine, crate-boundary docs, product specs, and the builder guide are
  subordinate role-specific docs. When they conflict with this overview and the
  ADR-0069 through ADR-0073 spine, correct the owning doc in place.

## Developer Model

An NMP app has one Rust composition root. That root declares:

- which reusable Nostr protocol features are installed;
- which app-owned product features are installed;
- which typed read sessions or generated helpers the app exposes;
- which typed write builders/actions the app exposes;
- which raw capabilities the shell may execute;
- which app policy belongs to this app and not to NMP.

The shell starts the Rust runtime, opens typed read sessions, dispatches typed
write actions, renders typed output, and answers capability requests. It does not
choose Nostr relays, parse protocol truth, mutate tags, infer publish success,
own retry policy, or keep durable product state.

## Reading Data

A screen opens a typed read session for the thing it wants to show.

Example shape:

```text
open article feed:
  primary kind: 30023
  source:
    direct authors are people the active user follows
    OR target article was reacted to/commented on by a followed user
  ranking: chronological
  projection key: app.feed.articles
```

That one app-facing demand owns the whole lifecycle:

- resolve the active account's follow set;
- find the relevant author and pointer sources;
- route reads through NIP-65/outbox-aware relay policy;
- replay cached events before live activation;
- subscribe to live relay events;
- admit only matching target events;
- emit typed output and status;
- update when follows or relay lists change;
- clear withdrawn output and close child demand on teardown.

Internally, this may use `open_interest`, observed projection delivery,
`ReducedSource`-style dynamic source reconciliation, replay cursors, and snapshot
emission. Those are implementation machinery, not the developer API.

The rule is not "delete every complex mechanism". The rule is "one product read
has one app-facing contract and one owner." Complexity is justified only when it
protects real Nostr correctness behind that contract.

## Writing Data

Writes separate event construction, signing, and publishing.

Construction is composable. A protocol or app crate can expose helpers such as:

```text
draft = reply_to(event)
draft.content = "nice!"
publish(draft)

reaction = react_to(event, "+")
publish(reaction)

article = new_article(title: "Hello World", content: "this is my article")
publish_to_group(article, group_id)
```

The exact API is not settled by this document. The invariant is:

- builders create unsigned drafts or typed action payloads;
- finalizers mutate the envelope before signing when a protocol requires it;
- signing is a capability stage selected by Rust-owned policy or explicit typed
  caller choice;
- publishing owns route planning, relay attempts, local ingest, retry, and
  status.

Protocol publish helpers can override default NIP-65/outbox planning only with
typed route provenance. Examples:

- NIP-17 DMs use verified private inbox routes, not public outbox planning;
- NIP-29 group events add the group tag before signing and publish to the group
  host relay;
- an explicit relay override is allowed only as a manual route with provenance;
- imported/pre-signed events stay imported/manual and do not acquire protocol
  guarantees after the fact.

Once an event is signed, the signed content is immutable. Only transport and
status metadata may change after signing.

## What NMP Does Internally

NMP is organized so app code sees a small number of product doors, while the
framework absorbs the Nostr-specific lifecycle work.

### Composition

The app Rust crate installs explicit features. Hidden production presets are not
the architecture. `nmp-defaults` can provide reusable installers, but it must not
own leaf-app policy.

### Runtime

Runtime crates own platform lifecycle:

- `nmp-native-runtime` owns native actor lifecycle and native builder state;
- `nmp-browser-runtime` owns browser worker and wasm runtime constraints;
- `nmp-ffi` owns C/JNI glue only.

Runtime crates do not own app product policy.

### Store And Ingest

The actor owns the event store. Relay results, locally signed events, imports,
and capability outputs enter through typed actor paths. The store enforces
replaceable/delete/expiration semantics and records provenance.

The shell never receives "relay fetch result as product truth." It receives
bounded typed output from Rust.

### Read Sessions

A typed read session compiles product demand into internal acquisition, replay,
source reconciliation, projection output, and teardown.

Internal pieces such as interests, observed projections, reduced sources,
snapshot ticks, and pull cursors survive only if they are the best machinery for
that compiler/runtime. They should not leak as concepts every app author must
learn.

### Write Workflows

Typed actions/builders enter through one dispatch doorway. Protocol and app
crates own their payloads and validation. The actor records operation identity,
builds or finalizes unsigned events, requests signing, publishes through typed
route policy, ingests local events, and emits structured status.

The shell can render pending, waiting for signature, rejected, publishing,
published, failed, or cancelled. It cannot turn dispatch acceptance into publish
success.

### Capabilities

Capabilities are raw OS/browser work:

- keychain or keyring access;
- external signer calls;
- file/blob selection;
- browser APIs;
- platform networking facts.

The shell executes the raw operation and reports raw result data. Rust decides
policy and state transitions.

## Public Surface Disposition

The migration should remove public concepts when they are only artifacts of the
old internal shape. Some mechanisms can remain inside NMP when they are the
right implementation machinery, but they should stop being builder-facing doors.

| Surface | Current disposition |
|---|---|
| production `register_defaults()` as the normal app root | Retire as production architecture. Production apps should expose explicit app-owned composition; defaults can survive only as tutorial, test, or migration helpers with an owner and deletion trigger. |
| app-facing raw `open_interest` | Retire as a product read door. It can remain low-level acquisition machinery behind typed sessions/helpers. |
| `open_feed` | Retire duplicate generic feed doors as product architecture. App crates should expose product-shaped feed/session helpers rather than shell-owned feed assembly. |
| app-facing `ObservedProjection` | Retire as a builder recipe. Observed projections can remain scoped internal delivery/replay machinery behind typed read sessions. |
| public `ReducedSource` vocabulary | Retire from app-facing setup. Dynamic source reconciliation belongs behind the session compiler/runtime. |
| special `nmp.feed.home` singleton wiring | Retire as a special app architecture. A home feed may be a projection key or compatibility example, not the public model for new product reads. |
| `PublishRaw` | Keep only as a low-level write/action payload where a protocol or app-owned typed builder lowers into it; shells should not hand-author publish policy. |
| pre-signed publish | Keep as imported/manual publish with explicit provenance. Pre-signed events do not acquire protocol guarantees after signing. |
| anonymous explicit relay lists as product publish state | Retire. Manual relay routing must carry typed route provenance. |

Some of those mechanisms may remain internally. The success metric is fewer
public doors, fewer lifecycle recipes, fewer shell policy sites, and fewer
permanent concepts required to build an app.

## Red-Team Test

A proposed abstraction should fail review if it:

- adds a generic noun without deleting an older public noun;
- creates a second owner for route, source, signer, or output truth;
- makes the shell reimplement correctness on another platform;
- preserves a compatibility path with no owner and deletion trigger;
- teaches app authors an internal lifecycle recipe as normal product code;
- hides product policy in a default bundle.

Complexity is justified when it prevents real Nostr bugs: stale replaceable
events, wrong relay routing, leaked private routes, missing cache replay,
source-set races, signer/backend mismatch, or false publish success. Complexity
is not justified when it only protects an old API shape.
