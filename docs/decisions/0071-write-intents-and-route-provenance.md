# ADR-0071: Publish intents, composable event drafts, and route provenance

## Status

Accepted for the architecture redesign direction.

## Context

ADR-0064 established the one typed write/command doorway and signer capability
round-trip. The redesign keeps that doorway, but issue #2316 and downstream
audits exposed a separate write-path ambiguity: event construction, signing, and
publishing are separable stages, while current publish variants can collapse
manual relay overrides, NIP-29 host pins, NIP-17 private inboxes, imported
pre-signed events, and diagnostic sends into an indistinguishable explicit relay
list.

The missing invariant is not another broad routing context. Live GitHub state
closed the old `RoutingContext::explicit_targets` direction in favor of the
surviving `PublishTarget::Explicit` path. The remaining job is to carry intent
identity and route provenance through the existing one publish stack.

## Decision

Every production write starts as a Rust-owned publish intent before signing,
route resolution, relay sockets, or retry.

Event construction, signing, and publishing are separate stages, but they remain
one actor-owned workflow:

```text
record local publish intent
  -> construct event draft
  -> finalize protocol/app envelope
  -> sign through selected signer capability
  -> publish signed event through route policy
  -> report structured status
```

Event construction is composable. Protocol crates and app crates may provide
builders such as reply, reaction, article, group publish, direct message, relay
list, podcast episode publish, or Highlighter share. Finalizers may apply
protocol-specific envelope mutation before signing: NIP-29 group `h` tags and
host pins, NIP-17 private envelopes and inbox routes, NIP-22 reply shape, NIP-65
relay-list policy, client identity tags, or app-specific publish context.

After signing, event id and signature are immutable. Any required envelope
mutation must happen before signing.

From the app developer perspective, the write surface should feel like:

```text
draft = reply_to(event)
draft.content = "nice!"
publish(draft)

reaction = react_to(event, "+")
publish(reaction)

article = new_article(title: "Hello World", content: "this is my article")
publish_to_group(article, group_id)
```

Those examples are not API commitments. They describe the ownership boundary:
construction helpers build unsigned drafts; publishing selects or receives a
signer, applies any final envelope mutations, plans route policy, signs, sends,
and reports status. A caller may choose a non-primary signer or audited manual
relay override, but those choices become typed provenance, not anonymous relay
lists. Protocol publish helpers may opt out of default outbox planning only by
declaring a route class such as verified private inbox, group host pin, manual
override, imported event, or diagnostic route.

Publish routing status carries route provenance, not just relay URLs. The route
class must remain distinguishable through dispatch, finalization, signing,
remote-signer parking, retry/resume, local ingest, and status output:

- automatic public route;
- protocol host pin;
- verified private inbox;
- manual explicit override;
- imported/verbatim external event;
- diagnostic/test route.

Product code cannot construct an unclassified explicit relay list. Private
events require verified private-inbox provenance. Group writes require host-pin
provenance or remain imported/manual with reduced guarantees. Pre-signed events
do not silently upgrade into protocol-owned provenance after the fact.

Signing is a capability stage, not a construction stage. A draft can be built,
modified, finalized for a protocol envelope, and only then signed. Once signed,
the only valid publish mutations are transport/status metadata outside the
signed event.

The preferred implementation path is to widen or pair existing carriers such as
`PublishTarget`, relay selection reasons, publish commands, parked signer
continuations, publish records, and status payloads. A broad new
`PublishContext` type is justified only if the live code cannot carry the
invariant without duplicating route/privacy/protocol state.

Native/web shells dispatch typed actions or generated builders and render
structured status. They do not mutate tags, choose Nostr relays, retry publish,
or infer success from dispatch acceptance.

## Consequences

Positive:

- Offline-first pending, signed, planned, sent, failed, cancelled, and exhausted
  states are replayable from one Rust-owned status stream.
- NIP-17, NIP-29, manual, imported, and diagnostic routes stay auditable.
- Event construction remains composable without making protocol crates import
  each other's product types.
- ADR-0064's one write doorway survives.

Negative/tradeoffs:

- Existing explicit-relay callers need provenance classification.
- Some downstream paths that report queued/dispatch state as completion must
  change before they count as publish proof.
- Status payloads need structured route/stage data; display strings are not a
  durable route model.

## Alternatives considered

| Option | Why rejected |
|---|---|
| Reintroduce a broad routing context | The dead explicit-target seam was already removed; adding a second route lane increases concepts. |
| Treat `Explicit { relays }` as sufficient | It says where to send, not why that route is valid or what guarantees apply. |
| Let publish helpers sign immediately | It prevents later protocol finalizers from mutating envelopes safely. |
| Let native choose relays for expert cases | It violates Rust ownership of route, privacy, retry, and publish status policy. |

## Fitness functions / enforcement

- Every production publish path creates a durable or replayable intent identity
  before signing.
- Publish status includes provenance class, owner/reason, stage, correlation id,
  terminal state, and relay/server facts.
- Private routes fail closed without verified inbox provenance.
- NIP-29/group writes prove group context, host relay, and envelope mutation
  before signing.
- No product shell treats dispatch acceptance, queued state, or local signing as
  terminal publish success.
- Explicit-route callers are classified as automatic, host-pinned,
  verified-private, manual, imported, or diagnostic.

## Linked work

- ADR-0064: one typed write/command boundary.
- #1538 and PR #1600: deleted the dead explicit-target routing-context seam.
- #2316: foundational architecture decomposition.
- #2320: stale ADR/doc cleanup.
