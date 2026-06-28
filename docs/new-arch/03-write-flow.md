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
construct a NIP-29 group message
construct a NIP-17 DM envelope
construct a kind:10002 relay list
construct a podcast episode publish
construct a Highlighter share-to-room event
```

Construction may be layered:

- A reaction builder can construct the reaction event for a target event.
- A reply builder can inspect the target and choose the right reply shape, such
  as NIP-22 behavior when replying to a non-kind:1 event.
- A group publish helper can add the NIP-29 `h` tag and attach the host-relay
  route context.
- A DM helper can build the correct private-event envelope and mark the write as
  private-inbox-routed.
- An app crate can compose protocol builders without making protocol crates
  import each other.
- An app crate can own product-specific builders for custom kinds, queues,
  podcast metadata, capture flows, or curation events.

All event mutations happen before signing. After signing, the event id and
signature are immutable.

The conceptual result is an event draft plus publish context:

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

Names are illustrative. The shape matters: event bytes and routing/privacy
context travel together until signing and publishing finalize the operation.

## Stage 2: Finalization

Finalization is the last mutation point before signing. It applies route and
protocol envelope rules that may depend on the publish call:

- NIP-29 can add or verify the `h` tag, attach group id, and pin the host relay.
- NIP-17 can construct private envelopes and opt out of public outbox routing.
- NIP-22 reply helpers can choose reply tags based on the target kind.
- NIP-65 relay-list publishing can enforce the correct public route policy.
- Podcast publishing can attach explicit write relays and Blossom references.
- App builders can attach correlation ids and product publish state.

Finalization must fail before signing if the required context is missing. A
group publish without a group route, an unknown private inbox, or an unsupported
explicit relay policy should not create a signed event.

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

The output is a signed event.

Publishing a pre-signed event is an expert path. It still enters Rust with a
`PublishContext`, validation result, correlation id, and route policy. It must
not create a second native publish API.

## Stage 4: Publishing

Publishing takes a signed event and a route policy.

Default public events use automatic routing:

```text
author write relays via NIP-65
plus protocol-required recipient inboxes where applicable
```

Manual relay selection is an explicit opt-out:

```text
publish(event, relays = [...])
```

There should be one canonical explicit-relay representation internally. The API
can expose convenient builders, but it should not grow parallel explicit-route
paths that bypass the publish policy table.

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
nip29.publish_to_group(article, group_id)
nip29.reply_to_group_event(event).content("nice")
podcast.publish_episode(show_id, episode_id, signer, relays)
highlighter.share_artifact_to_room(artifact_id, room_id)
```

These examples are interface sketches. The settled API should prefer generated,
typed, field-complete builders over JSON action strings or raw tag mutation.

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
