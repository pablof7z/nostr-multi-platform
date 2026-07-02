# ADR-0071: Publish intents, composable drafts, and route provenance

> **Not the same "intent" as `nmp-intent`.** The "operation intent" this ADR
> defines (record → draft → finalize → sign → publish → status, below) is the
> publish/write pipeline. `nmp-intent`'s input-intent resolver (raw user text
> → ref/relay/nip05/search classification,
> [docs/design/intent-routing/types.md §3.5](../design/intent-routing/types.md))
> is an unrelated concept that happens to share the word "intent".

## Decision

Every production write starts as a Rust-owned operation intent before signing,
route resolution, relay sockets, retry, or user-visible completion state.

Event construction, finalization, signing, publishing, local ingest, retry, and
status reporting are separate stages of one actor-owned workflow:

```text
record local intent
  -> construct unsigned draft
  -> finalize protocol or app envelope
  -> sign through the selected signer capability
  -> publish through route policy
  -> report structured status
```

Draft builders are composable. Protocol crates and app crates may provide
builders for replies, reactions, articles, group publishes, direct messages,
relay lists, app shares, and similar flows. Builders produce unsigned drafts or
typed draft commands. They do not sign, publish, choose relays, or imply terminal
success as hidden side effects.

Finalizers may mutate signed-event content only before signing. Group tags,
private envelopes, client identity tags, reply tags, relay-list policy, and
app-specific publish context must be complete before the signature is created.
After signing, event id and signature are immutable.

Route provenance is typed. Product code cannot construct unclassified explicit
relay lists. A route is classified as automatic public route, protocol host pin,
verified private inbox, manual explicit override, imported external event, or
diagnostic route. Private events fail closed without verified private-inbox
provenance.

Intent initiation is not artifact construction. Generic protocol artifact
grammar has one owner. Apps and protocol crates request or wrap artifacts they
do not own; they do not hand-build foreign wire events.

## Context

Loose publish surfaces collapsed different concerns into one relay list:
manual overrides, group host pins, private inboxes, imported events, diagnostic
sends, and ordinary outbox routing. The actor needs to know why a route is
valid, not just where bytes are sent.

NMP also needs app-visible draft ergonomics without giving shells ownership of
signing, relay policy, retry, local ingest, or publish status.

## Consequences

The publish pipeline carries more structured status and provenance. That makes
offline-first pending, signed, planned, sent, failed, cancelled, and exhausted
states replayable and auditable.

Some existing raw or verbatim publish paths are valid only as protocol-owned,
imported, manual, or diagnostic flows. They are not the happy path for app
writes.

## Boundaries

Permitted:

- app/protocol draft builders that produce unsigned drafts;
- finalizers that mutate envelopes before signing;
- typed manual overrides with provenance and reduced guarantees;
- imported/verbatim signed events with imported provenance;
- structured publish status emitted from Rust.

Forbidden:

- draft builders that sign, publish, or choose relays as side effects;
- tag or envelope mutation after signing;
- generic raw publish as app write guidance;
- anonymous explicit relay lists in production product code;
- shells treating dispatch acceptance, queued state, local signing, or local
  ingest as terminal publish success.

## Enforcement

Publish tests check route class, owner/reason, signer provenance, stage,
correlation id, relay facts, terminal status, and fail-closed private routing.

Ownership gates require protocol-owned artifact provenance for generic wire
artifacts such as kind:5 deletion events. Doctrine and clean-room docs gates
reject app-facing raw publish guidance, anonymous manual routes, and terminal
success claims based only on dispatch or queue state.

## Related

- [ADR-0074](0074-nip09-generic-deletion-ownership.md) - NIP-09 deletion
  artifact ownership.
- [ADR-0069](0069-explicit-feature-composition.md) - protocol owner
  composition.
- [ADR-0072](0072-runtime-capability-and-shell-boundary.md) - shell boundary.
- [docs/product-spec/api-surface.md](../product-spec/api-surface.md) - public
  write API examples.
- [docs/product-spec/doctrine.md](../product-spec/doctrine.md) - doctrine gates.
- #2746 - ADR current-only cleanup.
