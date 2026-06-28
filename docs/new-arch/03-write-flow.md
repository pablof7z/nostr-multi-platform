# Write Flow

Writes have three separate concerns:

```text
construct unsigned event data
  -> finalize the protocol envelope and route context
  -> sign
  -> publish
```

They are separate so event construction remains composable while NMP still owns
signing, routing, retry, local status, and protocol correctness.

## Construction

Feature code can construct drafts:

```text
react to event X with "+"
reply to event Y
build an article draft
build a NIP-17 private message envelope
build a kind:10002 relay list
build an app-specific product event
```

Construction may inspect protocol facts. For example, a reply builder may choose
NIP-22 semantics when replying to a non-kind:1 event.

Construction does not publish. It produces unsigned event data and intent facts
that later stages can finalize, sign, and route.

## Envelope Finalization

Some protocols need to mutate or validate the unsigned envelope before signing:

- NIP-29 group publishing adds or validates group context such as the `h` tag
  and host relay route.
- NIP-17 private messaging requires private-inbox routing and fail-closed
  recipient relay proof.
- Manual relay override must be marked as an audited explicit route, not hidden
  as normal outbox planning.

All such mutation happens before signing. After signing, event id and signature
are immutable.

## Signing

NMP chooses the signer from Rust-owned policy or an explicit signer selection.
The signer may be primary, delegated, remote, or app-provided through a capability
port. Native executes capability calls and returns raw results; Rust owns the
continuation, status, retry, and failure state.

## Publishing

Publishing uses Rust-owned routing:

- automatic public routing goes through the planner;
- protocol host pins publish to protocol-owned relays;
- verified private routes fail closed when relay proof is missing;
- manual routes are explicit audited opt-outs;
- imported or pre-signed events carry reduced guarantees.

Apps may override relays only through a route class that survives signing,
retry, local ingest, and status reporting.

## Status

Before a write touches signer or network, Rust should record a local publish
intent identity. That identity is not the event id; it exists before signing and
survives remote-signer parking, restart, retry, cancellation, and eventual
publish outcome.

Publish status should expose structured facts:

- durable intent id;
- signer stage;
- route provenance class;
- planned relays or rejected route reasons;
- local store state;
- relay outcomes;
- cancellation or terminal failure.

Shells render labels, tone, icons, and copy. They do not infer publish truth from
relay URLs or fire-and-forget calls.
