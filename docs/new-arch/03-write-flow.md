# Write Flow

Writes have separable stages:

```text
construct draft -> finalize envelope -> sign event -> publish signed event
```

They are separate because apps sometimes need to construct a draft, sign with a
non-primary signer, publish a pre-signed event, or publish through a
protocol-specific route. The stages are still Rust-owned. Native does not route,
retry, infer tags, or maintain publish state.

The public doorway should still be typed actions or generated builders that
become typed actions. The internal publish path should remain one path through
the actor, signer port, publish policy, local store, and publish engine.

## Stage 1: Construction

Event construction is composable protocol and product logic.

Examples:

```text
react to event X with "+"
construct a reply to event Y
construct an article draft
construct an event that may later be published into a NIP-29 group
construct a NIP-17 DM envelope
construct a kind:10002 relay list
construct a podcast episode publish
construct a Highlighter share-to-room event
```

Construction may be layered:

- A reaction builder can construct the reaction event for a target event.
- A reply builder can inspect the target and choose the right reply shape, such
  as NIP-22 behavior when replying to a non-kind:1 event.
- NIP-29 does not construct replies, reactions, articles, or app-specific
  events. It can finalize an already-constructed event into a group context by
  adding or validating NIP-29 group envelope data and host-relay route context.
- A DM helper can build the correct private-event envelope and mark the write as
  private-inbox-routed.
- An app crate can compose protocol builders without making protocol crates
  import each other.
- An app crate can own product-specific builders for custom kinds, queues,
  podcast metadata, capture flows, or curation events.

All event mutations happen before signing. After signing, the event id and
signature are immutable.

The conceptual result is unsigned event data plus route/privacy/protocol context:

```text
EventDraft {
    kind,
    tags,
    content,
}

PublishContext {
    route_policy,
    privacy_policy,
    protocol_context,
}
```

Names are illustrative and are not type commitments. The first implementation
should try to collapse existing publish variants behind existing `PublishTarget`,
publish command, policy, signer, and correlation data. New `EventDraft` or
`PublishContext` types are justified only if they remove branching or duplicate
route/privacy/protocol state elsewhere.

The missing invariant is route provenance, not a broad context wrapper. The
publish path must know whether an explicit relay set is a manual override, a
NIP-29 host pin, a verified private inbox, an imported/verbatim external event,
or another protocol-owned route. The exact representation is an ADR decision,
but the distinction must survive signing, remote-signer parking, retry/resume,
local ingest, and status emission.

Minimum route-provenance matrix:

| Route provenance | Required proof | Allowed planner behavior | Status/reporting |
|---|---|---|---|
| automatic public route | author/recipient context plus NIP-65/mailbox lookup | planner chooses relays and replans on mailbox change | route planned/rejected with reasons |
| protocol host pin | protocol context such as NIP-29 group id and host relay | publish only to the host/protocol relays | host pin visible in status |
| verified private inbox | recipient inbox proof from protocol-specific lookup | publish only to verified inbox relays | fail closed when unknown |
| manual explicit override | app/protocol owner, purpose, tests, and relay set | send exactly there, with no hidden fallback | marked manual/audited |
| imported/verbatim external event | caller declares reduced guarantees and supplies signed bytes | validate/store/publish only within explicit policy | status says imported/verbatim |

Existing explicit-relay seams need to be classified before they are collapsed:

| Current shape | Required provenance class | Required proof |
|---|---|---|
| `PublishTarget::Explicit` from app/manual caller | manual explicit override | caller owns purpose/reason; no hidden fallback; status marks manual |
| NIP-29 group publish plan | protocol host pin | group id, host relay, `h` tag/previous-group tags where applicable; reject missing group context |
| NIP-17/gift-wrap relay set | verified private inbox | recipient inbox proof; fail closed when unknown |
| `UnsignedEventToRelays` / pre-signed import | imported/verbatim or protocol host pin | imported status and reduced guarantees unless a protocol plan proves stronger provenance |
| test/diagnostic explicit relays | diagnostic/test | not reachable from product shell APIs |

## Stage 2: Finalization

Finalization is the last mutation point before signing. It applies route and
protocol envelope rules that may depend on the publish call:

- NIP-29 can add or verify the `h` tag, attach group id, preserve/append
  group-context tags where the protocol requires them, and pin the host relay.
- NIP-17 can construct private envelopes and opt out of public outbox routing.
- NIP-22 reply helpers can choose reply tags based on the target kind.
- NIP-65 relay-list publishing can enforce the correct public route policy.
- Podcast publishing can construct NIP-F4 show/feed/episode/list events, attach
  Blossom references, select per-podcast or active-account signers, and preserve
  explicit write-relay provenance.
- App builders can attach correlation ids and product publish state.

Finalization must fail before signing if the required context is missing. A
group publish without a group route, an unknown private inbox, or an unsupported
explicit relay policy should not create a signed event.
Generic raw publishing cannot silently bypass protocol invariants. A NIP-29
group write is not `comment_in_group` or `reply_in_group`; it is "publish this
already-constructed event in this group context." The base event is constructed
by the appropriate NMP/app builder, then the NIP-29 finalizer adds or verifies
the group envelope and host route. An `h`-tagged write that did not pass through
that group-route proof is imported/verbatim/manual raw publish with reduced
guarantees. Likewise, private-event publishes must distinguish verified NIP-17
inbox routing from other explicit private-envelope delivery.

## Stage 3: Signing

Signing uses a selected signer:

```text
default active account
specific registered account
named product signer
agent signer
remote NIP-46 signer
platform signer capability
```

The app may request "publish as this signer," but NMP owns signer lookup,
capability round trips, failure state, and replayability. The action vocabulary
does not encode a native signing backend. Native executes keychain or signer
capabilities and reports raw results; Rust decides the next state.

Signer runtime state is an output contract. Local, NIP-46, NIP-55, agent, and
named signer paths should expose typed pending/ready/failed/remote/local status,
broker initialization or teardown state where applicable, and correlation ids
for parked continuations. Shells should not infer signer completion from missing
errors, URI callbacks, or side effects.

NIP-46-style signer protocols should own only transport-agnostic protocol state:
handshake sequencing, RPC envelope build/parse, NIP-44 wrapping, request ids, and
response correlation. Runtime crates or platform capabilities own sockets,
threads, browser workers, OS callbacks, and process execution. A signer protocol
crate that owns transport/process lifecycle is the wrong boundary; a host that
builds NIP-46 envelopes in native code is also the wrong boundary.

Minimum signer matrix:

| Signer path | Native/web role | Rust-owned state |
|---|---|---|
| local app key | execute secure key access capability | selected account, permission, pending/failed/signed status |
| browser NIP-07 | invoke browser signer capability | action correlation, capability result, route/privacy continuation |
| NIP-46/browser bunker | execute relay/RPC transport capability where hosted outside Rust | broker lifecycle, parked continuation, signer identity, timeout/failure state |
| NIP-46/native broker | execute OS/network capability if delegated | broker lifecycle, reconnect state, parked continuation, status output |
| Android/iOS platform signer (NIP-55-style) | execute external signer IPC/app callback | capability correlation, selected signer, pending/cancel/failure state |
| named product/agent signer | execute configured signer capability | signer registry, permission, product policy, publish continuation |
| imported pre-signed event | no signing, only validation/capability import if needed | validation result, route provenance, status/retry policy |

Podcast Player adds a concrete signer proof:

| Podcast signer need | Required model |
|---|---|
| active user publishes a normal social note/comment | active account signer plus automatic or protocol route provenance |
| per-podcast key publishes NIP-F4 show/feed/episode events | named product signer with explicit ownership, permission, status, and key storage capability |
| agent publishes or mutates podcast-owned data | agent signer path with product permission and publish correlation |
| NIP-46/bunker signer | parked continuation with timeout/failure owned by Rust, not Swift `Task.sleep` UI policy |
| NIP-55/platform signer | external signer capability result mapped back to the same signer/publish status stream |

The output is a signed event plus signer status updates.

Publishing a pre-signed event is an integration/import path. It still enters
Rust with route and privacy context, validation result, correlation id, and route
policy. It must not create a second native publish API or bypass local validation,
store ingest, publish status, retry/cancel, or privacy gates.

## Stage 4: Publishing

Publishing takes a signed event and a route policy.

Default public events use automatic routing:

```text
author write relays via NIP-65
plus protocol-required recipient inboxes where applicable
```

Manual relay selection is an explicit opt-out:

```text
typed_publish_action.with_audited_explicit_route(relays, reason)
```

There should be one canonical explicit-relay representation internally. The API
can expose convenient builders, but it should not grow parallel explicit-route
paths that bypass the publish policy table.

Existing explicit-relay seams must converge before this is considered clean. If
`PublishTarget::Explicit`, protocol publish plans, routing contexts, and
pre-signed publish APIs all represent "send exactly here," the ADR must either
collapse them to one internal representation or document why each remaining seam
protects a different invariant. Dead explicit-target fields should be deleted,
not taught as part of the architecture.

The phase gate is not only D10/D11. It needs focused tests that prove route
provenance survives dispatch-envelope decode, codegen builders, signer
continuation park/drain, retry/resume, local ingest, and publish-status
projection. D10/D11 prevent some bypasses; they do not prove provenance was
preserved.

Protocol crates can define stricter routing:

- NIP-29 group events are host-relay-pinned. The group feature supplies the host
  relay context and rejects an `h`-tagged publish that lacks that context.
- NIP-17 DMs do not use normal public outbox publishing. The DM feature owns
  private envelope construction and recipient inbox routing, and unknown inboxes
  fail closed.
- Cross-protocol flows are composed in the app crate. For example, a highlighter
  app can publish a highlight through the highlight feature, then share it into a
  NIP-29 group through the group feature. Neither protocol crate imports the
  other's domain types.

Before relay delivery, NMP stores the signed event locally when the protocol
allows read-your-writes. Projections update from the store path, not from native
optimism. The publish engine owns relay dispatch, ack classification, retry,
resume, cancellation, and publish status projections.

Podcast NIP-F4 is a required publish proof, not an optional downstream nicety:

```text
NIP-F4 show event       kind:10154
NIP-F4 episode event    kind:54
podcast/follow/feed     kind:10064 or successor if the spec changes
Blossom references      server/blob references with explicit server provenance
```

The final architecture is not proven while these paths return only constructed
event JSON or `relay_pending`. They must build, sign, route, store, publish, and
emit ack/error/retry/exhausted status through the same Rust-owned publish stream.
Blossom server selection follows the same rule as relay selection: native may
execute upload/download capabilities, but Rust owns which server list is valid,
why an explicit server is allowed, and how that status is reported.

## Generated Builders And Actions

The app-facing API can look like builders, but the runtime boundary should be a
typed dispatch envelope:

```text
reply_to(event).content("nice")
  -> typed action payload
  -> DispatchEnvelope
  -> ActionModule
  -> signer/publish continuation
  -> publish status output
```

Generated builders should carry namespace, schema version, host correlation id,
feature route, and validation errors into the same byte doorway used by native,
web, and TUI shells.

Builder examples:

```text
react_to(event, "+")
reply_to(event).content("nice")
article().title("Hello World").content(body)
nip29.publish_group_event(group_id, article().title("Hello World").content(body))
nip29.publish_group_event(group_id, reply_to(event).content("nice"))
podcast.publish_episode(show_id, episode_id, signer, relays)
highlighter.share_artifact_to_room(artifact_id, room_id)
```

These examples are interface sketches. The settled API should prefer generated,
typed, field-complete builders over JSON action strings or raw tag mutation.
App-owned raw event templates are acceptable only inside Rust typed actions or
generated builders. They may construct app-specific kinds such as highlights,
comments, lists, or podcast events, but they still must carry correlation,
validation, route/privacy context, signer selection, and publish-status output
through the same actor-owned flow. Fire-and-forget event writes are not a
separate app runtime.

Highlighter acceptance requires a write inventory. Web NDK paths that build,
sign, and publish onboarding profile events, Blossom lists, interest lists,
group invites/membership/metadata, highlights, comments, capture events, or
artifact shares must either move behind typed Rust actions/builders or be
classified by ADR as SSR-only, diagnostic, or migration-scoped with deletion
criteria. Native/Rust raw publish paths need the same correlation and status
proof; "the event was sent" is not enough.

## Compatibility Boundaries

Event-producing writes go through the typed action/publish doorway:

```text
generated builder or typed action
  -> DispatchEnvelope
  -> ActionModule
  -> actor-owned sign/finalize/publish path
```

Lifecycle and capability control calls may keep dedicated APIs when they do not
construct events, but new event-producing FFI symbols, bespoke `send_cmd` paths,
or native-built `PublishRaw` JSON are out of bounds. Explicit relay publishing
must have one canonical internal representation, so group pins, podcast write
relays, and audited app/protocol relay overrides do not fork into parallel
routing paths.

Public runtime surfaces should be classified explicitly:

- substrate internals;
- protocol-internal actions and sessions;
- generated app-feature APIs;
- capability control and result reporting;
- diagnostic/test tools;
- migration shims with deletion criteria.

Generated app-feature APIs are allowed for non-event product work such as
playback, downloads, provider credentials, STT/TTS, local agents, catalog fetches,
or imports. They become a violation only when they construct Nostr events, choose
Nostr relays, infer protocol tags, or own publish/sign status outside Rust.

The same boundary applies to web/TypeScript. Direct NDK subscriptions, direct
web publish/sign paths, and web-side tag/protocol parsing are product-runtime
violations unless an ADR classifies them as SSR-only, diagnostic, or
migration-scoped with deletion criteria.

## Publish Status

Publish status is a first-class output, not a native side table:

```text
correlation id
  -> draft accepted/rejected
  -> signer pending/signed/failed
  -> locally stored/skipped
  -> route planned/rejected
  -> relay ack/error/retry/exhausted
  -> product completion state
```

Highlighter action results, podcast publish diagnostics, Blossom upload
continuations, signer handoffs, and retry/cancel controls should converge on
typed publish/action-result outputs. Apps can render product-specific state, but
Rust remains the single writer of publish progress.
