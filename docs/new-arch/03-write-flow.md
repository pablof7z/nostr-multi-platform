# Write Flow

Writes have three separable stages:

```text
construct event draft -> sign event -> publish signed event
```

They are separate because apps sometimes need to construct a draft, sign with a
non-primary signer, publish a pre-signed event, or publish through a
protocol-specific route. The stages are still Rust-owned. Native does not route,
retry, infer tags, or maintain publish state.

## Stage 1: Event Construction

Event construction is composable protocol and product logic.

Examples:

```text
react to event X with "+"
construct a reply to event Y
construct an article draft
construct a NIP-29 group message
construct a NIP-17 DM envelope
construct a kind:10002 relay list
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

## Stage 2: Signing

Signing uses a selected signer:

```text
default active account
specific registered account
agent signer
remote NIP-46 signer
platform signer capability
```

The app may request "publish as this signer," but NMP owns signer lookup,
capability round trips, failure state, and replayability. The action vocabulary
does not encode a native signing backend. Native executes keychain or signer
capabilities and reports raw results; Rust decides the next state.

The output is a signed event.

## Stage 3: Publishing

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
