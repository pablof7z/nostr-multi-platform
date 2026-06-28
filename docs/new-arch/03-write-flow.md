# Write Flow

Writes have separable stages:

```text
record local publish intent -> construct draft -> finalize envelope
  -> sign event -> publish signed event
```

They are separate because apps sometimes need to construct a draft, sign with a
non-primary signer, publish a pre-signed event, or publish through a
protocol-specific route. The stages are still Rust-owned. Native does not route,
retry, infer tags, or maintain publish state.

The public doorway should still be typed actions or generated builders that
become typed actions. The internal publish path should remain one path through
the actor, signer port, publish policy, local store, and publish engine.

Before a write touches a signer, relay socket, or publish planner, it becomes a
local publish intent/status fact owned by Rust. That is the offline-first root:
the app can show pending, failed, cancelled, signed, stored, planned, sent, and
exhausted states from one replayable ledger even when signing or network
delivery is delayed. Route provenance should attach to that intent or its target
resolution record, then survive through finalization, signer parking, retry,
resume, local ingest, and status emission. A fire-and-forget relay publish call
is not the NMP write architecture.

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
  events. Its content write surface is exactly "publish this already-constructed
  event in this group context." It finalizes the event by adding or validating
  NIP-29 group envelope data and host-relay route context.
- A DM helper can build the correct private-event envelope and mark the write as
  private-inbox-routed.
- An app crate can compose protocol builders without making protocol crates
  import each other.
- An app crate can own product-specific builders for custom kinds, queues,
  podcast metadata, capture flows, or curation events.

All event mutations happen before signing. After signing, the event id and
signature are immutable.

The conceptual result is a local publish intent plus unsigned event data and
route/privacy/protocol context:

```text
PublishIntent {
    correlation_id,
    owner,
    status,
}

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
should keep the existing one publish doorway, but widen or augment the existing
intent/target/reason/status carriers so the missing audit facts survive. Reusing
`PublishTarget` as-is is not enough: today's `Explicit { relays }` says only
"send here," not why that exact route is valid. New `PublishIntent`,
`EventDraft`, or `PublishContext` types are justified only if they remove
branching or duplicate route/privacy/protocol state elsewhere.

Current first-slice recommendation: add narrow internal provenance and intent
records, then carry them through existing publish carriers. A broad
`PublishContext` is the fallback, not the target. Current code does not yet
satisfy the offline-first ledger rule for all publish paths: durable records are
too often created after signing or target resolution, and some downstream paths
report queued/dispatch state as if it were publish completion. P6 must prove a
real pre-sign durable intent identity before claiming the write architecture is
solved. The likely shape is:

```text
PublishTarget::Explicit {
    relays,
    provenance: PublishRouteProvenance,
}

PublishIntentRecord {
    durable_intent_id,
    host_correlation_id?,
    owner,
    stage,
    draft_kind?,
    signer_selector?,
    signer_pubkey?,
    event_id?,
    naddr?,
    route_provenance?,
    route_plan?,
    local_store_state,
    signer_state,
    relay_outcomes,
    retry_state,
    cancellation_state,
    error?,
}
```

`durable_intent_id` is not the event id and not just a process correlation id.
It exists before signing, survives remote-signer parking, restart, retry, cancel,
and route replanning, and is the identity status outputs use until an event id or
addressable id exists.

`PublishRouteProvenance` should describe the class, owner, subject, and
guarantee of an explicit route. Automatic public routing stays `Auto` plus
resolver reasons. Explicit route classes are protocol host pin, verified private
inbox, manual override, imported/verbatim, and diagnostic. Product code should
not be able to construct an unclassified explicit relay list.

Hard route rules:

- private kinds and private envelopes require verified private-inbox provenance;
  a non-empty `Explicit` relay list is not enough;
- `h`-tagged/group writes require NIP-29 host-pin provenance or remain
  imported/manual raw writes with reduced guarantees;
- pre-signed/imported events never upgrade themselves into protocol-owned
  provenance after the fact;
- manual explicit routes are audited opt-outs with owner, reason, and status,
  not a silent replacement for failed outbox planning.

The missing invariant is route provenance, not a broad context wrapper. The
publish path must know whether an explicit relay set is a manual override, a
NIP-29 host pin, a verified private inbox, an imported/verbatim external event,
or another protocol-owned route. The exact representation is an ADR decision,
but the distinction must survive signing, remote-signer parking, retry/resume,
local ingest, and status emission.
Publish/action status must expose that provenance class and reason. A status row
that says only `queued`, `signed`, `sent`, or lists relay URLs is insufficient:
the app and audits must be able to tell whether the route was automatic,
host-pinned, verified-private, manual/audited, imported/verbatim, or diagnostic.
That status is structured data. Rust emits provenance class, reason, owner,
terminal state, relay/server facts, and correlation ids; shells render labels,
tone, icons, and copy. Do not reintroduce display strings such as
`relay_reason`/`format_relay_reason()` as the durable route model.

Minimum route-provenance matrix:

| Route provenance | Required proof | Allowed planner behavior | Status/reporting |
|---|---|---|---|
| automatic public route | author/recipient context plus NIP-65/mailbox lookup | planner chooses relays and replans on mailbox change | route planned/rejected with reasons |
| protocol host pin | protocol context such as NIP-29 group id and host relay | publish only to the host/protocol relays | host pin visible in status |
| verified private inbox | recipient inbox proof from protocol-specific lookup | publish only to verified inbox relays | fail closed when unknown |
| manual explicit override | app/protocol owner, purpose, tests, and relay set | send exactly there, with no hidden fallback | marked manual/audited |
| imported/verbatim external event | caller declares reduced guarantees and supplies signed bytes | validate/store/publish only within explicit policy | status says imported/verbatim |
| diagnostic/test route | test, export, or diagnostic owner and non-product scope | may bypass product planner only in that scope | hidden from product APIs |

Minimum implementation shape:

```text
route_provenance {
  class: automatic | host_pin | verified_private_inbox |
         manual_override | imported_verbatim | diagnostic
  owner: protocol | app | substrate | diagnostic/test
  context: typed, redacted facts needed to audit the class
  guarantees: what the route proves and what it explicitly does not prove
}
```

This is not a commitment to a new public `RouteProvenance` type. The cheapest
implementation may be to extend existing `PublishTarget`, `RelaySelectionReason`,
`PublishCommand`, `ParkedOp::Publish`, `PublishRecord`, and publish-status
payloads. That is the preferred path if it deletes branching and keeps one
publish stack. What is not acceptable is preserving today's ambiguity where NIP-29
host pins, NIP-17 inbox pins, manual overrides, cold-start account publishes,
and imported pre-signed events all become an indistinguishable
`Explicit { relays }` fact before status/retry/resume.

The class and reason must survive these edges:

- generated builder or typed action decode;
- finalizer mutation before signing;
- active/local signer and remote-signer parked continuation;
- signed-event validation and local ingest;
- publish-engine route planning, persistence, retry, resume, and cancel;
- publish/action status projection;
- diagnostic/export surfaces.

If provenance can be preserved by adding one field to the existing target/reason
pipeline, do that. A broad `PublishContext` object is justified only if the live
code audit proves the existing pipeline cannot carry the invariant without
duplicating route/privacy/protocol state.

Existing explicit-relay callers need provenance on the surviving publish path:

| Current shape | Required provenance class | Required proof |
|---|---|---|
| `PublishTarget::Explicit` from app/manual caller | manual explicit override | caller owns purpose/reason; no hidden fallback; status marks manual |
| NIP-29 group publish plan | protocol host pin | group id, host relay, `h` tag/previous-group tags where applicable; reject missing group context |
| NIP-17/gift-wrap relay set | verified private inbox | recipient inbox proof; fail closed when unknown |
| `UnsignedEventToRelays` host-pinned unsigned event | protocol host pin or manual override | caller-supplied provenance before signing; host pin keeps typed context such as group id/source |
| account/bootstrap relay-list publish | automatic public route or app-owned bootstrap policy | distinguish NIP-65 relay-list update from app onboarding/bootstrap defaults; status shows planner or policy source |
| pre-signed `SignedEvent` / imported event | imported/verbatim unless a protocol plan proves stronger provenance | imported status, validation result, and reduced guarantees; no silent upgrade to protocol-owned route |
| test/diagnostic explicit relays | diagnostic/test | not reachable from product shell APIs |

Older docs and wiki pages mention `RoutingContext::explicit_targets` as the
place NIP-17 or NIP-29 explicit routes should land. Live GitHub state verifies
#1538 was closed by merged PR #1600, which removed the dead routing-context
explicit-target seam and kept `PublishTarget::Explicit` as the single live
explicit-relay mechanism.
P6 must not reintroduce a broad routing context or second explicit route lane to
solve provenance. The remaining work is to carry provenance class and reason
through the existing publish target/reason/status pipeline.

Any partial change that merely adds a compatibility alias for historical routing
terminology fails the architecture gate. The goal is fewer explicit-route
mechanisms plus richer audit status, not a new wrapper around an already deleted
seam.

## Stage 2: Finalization

Finalization is the last mutation point before signing. It applies route and
protocol envelope rules that may depend on the publish call:

- NIP-29 can add or verify the `h` tag, attach group id, preserve/append
  group-context tags where the protocol requires them, and pin the host relay.
- NIP-17 can construct private envelopes and opt out of public outbox routing.
- NIP-22 reply helpers can choose reply tags based on the target kind.
- NIP-65 relay-list publishing can enforce the correct public route policy.
- Client identity finalization can append the app's declared NIP-89 client tag to
  eligible public-routable content events from one composition-root declaration,
  while User-Agent uses the same identity through the transport path. It must
  not mutate DMs, gift wraps, private envelopes, imported/pre-signed events, or
  reserved/profile/metadata events unless a protocol-specific ADR explicitly
  allows that surface.
- Podcast publishing can construct NIP-F4 show/feed/episode/list events, attach
  Blossom references, preserve Blossom server provenance, select per-podcast or
  active-account signers, and preserve explicit write-relay provenance.
- App builders can attach correlation ids and product publish state.

The finalizer result should be narrow. It should return the few facts the
publish engine needs rather than a broad context object that every protocol can
mutate freely:

```text
FinalizedDraft {
    unsigned_event,
    signer_selector,
    route_provenance,
    privacy_class,
    local_store_policy,
}
```

Finalization must fail before signing if the required context is missing. A
group publish without a group route, an unknown private inbox, or an unsupported
explicit relay policy should not create a signed event.
If a protocol finalizer needs recent context, it reads that context through the
Rust store/query seam. NIP-29 previous-tag anti-spam context, for example, must
come from a kernel/store-backed query over group events, not from a
`RecentGroupEvents`-style side cache owned by the action module or shell.
Client metadata has a hard privacy gate. A NIP-89 client tag, analytics marker,
or any other app identity metadata may be added only at one outbound-finalization
decision site, only before signing, and only for events classified as public
routable. The gate must exclude encrypted/private content, gift wraps, DMs,
profile/metadata writes, reserved builder-only kinds, diagnostic imports, and
pre-signed events. A protocol/app builder may request metadata, but the final
decision belongs to the shared finalizer so explicit-relay, automatic, and
protocol-pinned publish paths cannot diverge.
Generic raw publishing cannot silently bypass protocol invariants. A NIP-29
group write is not `comment_in_group`, `reply_in_group`, or a namespace of
kind-specific actions; it is one kind-agnostic content publish surface over an
already-constructed event plus group context. The base event is constructed by
the appropriate NMP/app builder, then the NIP-29 finalizer adds or verifies the
group envelope and host route. An `h`-tagged write that did not pass through that
group-route proof is imported/verbatim/manual raw publish with reduced
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

Signer storage authority must be explicit. Active-user keys, named product
signers such as per-podcast keys, agent signers, NIP-46 session material,
NIP-55/platform signer references, provider API keys, and imported pre-signed
events are different security classes. Each signer class needs a storage owner,
rehydration behavior, restart/retry/cancel/delete semantics, and a rule for which
publish ledger records keep it alive.

Podcast Player adds a concrete signer proof:

| Podcast signer need | Required model |
|---|---|
| active user publishes a normal social note/comment | active account signer plus automatic or protocol route provenance |
| per-podcast key publishes NIP-F4 show/feed/episode events | named product signer with explicit ownership, permission, status, and key storage capability |
| agent publishes or mutates podcast-owned data | agent signer path with product permission and publish correlation |
| NIP-46/bunker signer | parked continuation with timeout/failure owned by Rust, not Swift `Task.sleep` UI policy |
| NIP-55/platform signer | external signer capability result mapped back to the same signer/publish status stream |

Named product signers stay tied to the publish ledger until every dependent
show, episode, deletion, upload, or retry record reaches a terminal ack, error,
retry-exhausted, or cancelled state. Dispatch acceptance, queued/signed status,
or a timestamp written at enqueue time is not terminal and cannot release the
signer/key ownership obligation.

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

Relay planning is lane-aware. The pool owns connections, not product policy.
Outbox routing, NIP-29 host pins, verified NIP-17 inboxes, indexer/search
relays, fallback/app relays, and manual diagnostic overrides are different route
classes with different audit guarantees. Collapsing them into one explicit relay
list is the architecture problem.

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
  private envelope construction and dynamic recipient inbox lookup. Verified
  inbox provenance is distinct from static relay pins, and unknown inboxes fail
  closed.
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
event JSON, `queued`/`signed`, `relay_pending`, optimistic timestamps, or
`publish_dispatched`. They must build, sign, route, store, publish, and emit
ack/error/retry/exhausted status through the same Rust-owned publish stream.
Capability results that affect signed event bytes are prerequisites, not
post-signing mutations. For NIP-F4 this means media upload, content hash,
Blossom server selection, and the final media reference must complete or fail as
Rust-owned state before the show/episode event is finalized and signed. A
post-hoc Blossom reference rewrite after signing is invalid.
Blossom server selection follows the same rule as relay selection: native may
execute upload/download capabilities, but Rust owns which server list is valid,
why an explicit server is allowed, and how that status is reported.
A NIP-F4 event that contains a Blossom URL without recording which server
contract produced it has the same audit problem as an explicit relay list
without route provenance. Server provenance must survive construction,
publish status, retry/resume, and user diagnostics.
Republish or repair work cannot be driven by snapshot-tick polling. Blossom
upload completion, relay ack/error, user retry, credential changes, and job-state
events may enqueue work; a "check every snapshot" loop is the old architecture.

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
reply_to(event).content("nice")                 # NIP-10/NIP-22/app reply builder
article().title("Hello World").content(body)    # NIP-23/app article builder
nip29.publish_group_event(group_id, article_draft)
nip29.publish_group_event(group_id, reply_draft)
podcast.publish_episode(show_id, episode_id, signer, route_policy)
highlighter.share_artifact_to_room(artifact_id, room_id)
```

These examples are interface sketches. The settled API should prefer generated,
typed, field-complete builders over JSON action strings or raw tag mutation.
The layering is the point: reply, reaction, article, podcast, and app-specific
builders construct drafts; NIP-29 only finalizes an already-constructed draft for
group context and host routing. NIP-29 must not depend on NIP-22 or learn reply
semantics to publish a reply-shaped draft into a group.
Existing or proposed helpers such as `react_in_group`, `share_event_in_group`,
or `repost_in_group` are acceptable only as thin compatibility/generated
wrappers over the one kind-agnostic group finalizer. If they need separate route,
tag, signer, or status logic, they should be removed from the app-facing model
instead of becoming another publish namespace.

The app-facing shape should make the three stages visible without making the
shell own protocol policy:

```text
reply = reply_to(event).content("nice")
app.publish(reply)

group_share = highlighter.share_artifact(artifact_id)
app.publish_to_group(group_share, group_id)

episode = podcast.episode(show_id, episode_id).with_blossom(blob_ref)
app.publish(episode, signer: podcast_key, route: podcast_write_policy)
```

In all three cases the builder constructs an unsigned draft, the publish call or
protocol finalizer supplies the final envelope/route context, Rust selects or
uses the requested signer, and status arrives as typed output. The shell never
adds `h` tags, chooses NIP-65 relays, stores publish success, or decides whether
an explicit relay/server list is valid.

Highlighter's group-share/repost flow is the concrete field-completeness test.
The app draft may need artifact id, selected text, source metadata, article/card
refs, app tags, and discussion context before NIP-29 adds group envelope data and
host route provenance. A generic `comment_in_group` or native-built raw event is
not enough if it cannot carry those product fields through construction,
signing, local ingest, route status, retry, and diagnostics.

Podcast publishing is the concrete non-primary-signer test. NIP-F4 show/feed/
episode/list events, Blossom references, per-podcast keys, agent signers, write
relay policy, and server provenance must all use the same publish status stream.
Returning constructed JSON, `relay_pending`, or `publish_dispatched` without
ack/error/retry/exhausted status does not prove publishing.

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
Highlighter also proves that near-correct writes can still fail the architecture:
share-to-room, discussion, capture, relay app-data, and web publish flows must
carry the same correlation id, draft validation, signer continuation, route
provenance, local ingest, retry, and terminal status. A fire-and-forget raw
publish, `correlation_id: None`, or dispatch-accepted UI success is a retained
old door. A typed namespace does not save the design if the caller gets a
correlation/status handle and then discards it; losing terminal status is still
fire-and-forget publishing under a cleaner name.
Managed NIP-05 is a write workflow, not a generic SSR exception. The contract
must name the owner of the auth event kind/schema, injected clock/expiry,
uniqueness/idempotency, durable name mapping, profile-publish coupling,
compensation on partial failure, and typed status. A web route that validates
with server wall clock, mutates KV/memory, and publishes profile/Blossom/list
events directly is a product architecture choice that needs an explicit owner.
Highlighter Blossom/capture follows the same pre-sign rule as Podcast NIP-F4:
server selection, blob descriptor, hash, size/media metadata, upload result,
retry/failure status, and route/provenance must complete before any profile,
highlight, picture, share, or room event that references the blob is finalized
and signed.

Highlighter write/read rows that must be classified independently:

| Flow | Required owner/proof |
|---|---|
| room chat | NIP-29 group context, route/admission, signer path, local ingest, final publish status |
| room discussion roots/replies | NIP-22/thread parentage in Rust descriptors/builders, not TS/Swift tag assembly |
| public highlight comments | `#E`/root/comment admission and publish builders with typed status |
| reactions/bookmarks | protocol builders plus route provenance and read-your-writes projection |
| highlight publish | kind `9802` draft/build/sign/publish/status owned by Highlighter Rust |
| highlight repost/share | kind `16`/share-to-room composition through app Rust plus NIP-29 finalizer when needed |
| group invites/membership/admin | group feature/app Rust actions with host-route proof and status |
| relay app-data | classified as server/relay product policy, diagnostic, or migrated typed action |
| SSR search/featured/room reads | read-only public-data cache or Rust/app-server-owned product state with durability and injected seams |

Podcast proves the same rule for non-primary signers. Per-podcast keys, agent
signers, Blossom server references, configured write routes, and NIP-F4
show/episode/list events cannot update `last_published_at` or equivalent product
state on construction, signing, or queued dispatch. Product completion follows
from the publish ledger reaching a terminal ack/error/retry-exhausted state.
`publish_dispatched`, `relay_pending`, or "agent request accepted" is never
user-visible or agent-visible publish success for NIP-F4. NIP-F4 proof must cover
show kind `10154`, episode kind `54`, author/feed/list kind `10064` or successor,
deletion kind `5` when used, Blossom upload provenance, selected server, URL,
hash, size, signer identity, event id/naddr, correlation id, retry/cancel, and
restart recovery. Uploads or provider capabilities that affect signed bytes must
complete before signing; capabilities that happen after publish must be modeled as
separate follow-up writes.
The selected NIP-F4 proof must include executable checks for these three
failures: Blossom reference not complete before signing, `last_published_at`
mutated before terminal relay/server status, and dispatch/enqueue reported as
publish success.

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
durable intent id
  -> host correlation id
  -> owner and draft kind
  -> draft accepted/rejected
  -> signer selected/pending/signed/failed/cancelled
  -> locally stored/skipped/rejected
  -> route class, route owner, redacted context, guarantees
  -> route planned/rejected
  -> relay/server ack/error/retry/exhausted
  -> retry/cancel state
  -> product completion state
```

Highlighter action results, podcast publish diagnostics, Blossom upload
continuations, signer handoffs, and retry/cancel controls should converge on
typed publish/action-result outputs. Apps can render product-specific state, but
Rust remains the single writer of publish progress.
