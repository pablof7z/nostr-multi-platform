# ADR-0074: `nmp-nip09` as the exclusive owner of NIP-09 deletion artifacts

## Status

Current.

## Context

Kind:5 (NIP-09 deletion) events were previously assembled by the crates that
needed to delete something. The immediate case was `nmp-nip25`, which
hand-built a kind:5 event inside `reaction_delete_draft` and minted its own
artifact provenance token (`REACTION_DELETE_EVENT_PROVENANCE`). This violated
the composable-ownership doctrine established in #2506:

> Intent-initiation ≠ artifact-construction. The crate that *wants* a
> deletion is not the crate that *builds* the kind:5 wire event.

Kind:5 is generic NIP-09 deletion. It is not owned by NIP-25, NIP-29, an app
crate, or the publish substrate. Those crates may *request* deletion of
artifacts they own or wrap deletion drafts in an envelope, but they do not own
the generic deletion wire grammar.

## Decision

Create `nmp-nip09` as the **exclusive positive owner** of the NIP-09 kind:5
deletion artifact surface. Split the intent-vs-artifact responsibility:

```
nmp-nip25 owns: reaction retraction intent, deleted reaction id validation
nmp-nip09 owns: kind:5 deletion construction, e/k tag grammar, reason rules
publish gate: kind:5 requires nmp.nip09 artifact provenance
```

### nmp-nip09 owns

- `kind:5` deletion construction (`build_deletion_draft` / `build_deletion_event`).
- `e` tag grammar (one `["e", id]` per target event id).
- `k` tag grammar (one `["k", kind]` per kind tag, optional per NIP-09 §3).
- Deletion content/reason rules (content = human-readable reason).
- Deletion identity rules (at least one target event id; id must be hex-64).
- Generic deletion read seam: `deletion_targets(tags)` — the shared parse that
  projections call instead of hand-parsing `e` tags.
- A generic `nmp.nip09.delete` action module (`DeleteModule` / `Nip09Descriptor`)
  so apps can delete their own events without assembling kind:5 wire code.

### nmp-nip25 retains (non-exclusive intent claim)

`nmp-nip25` keeps its existing ownership claim
`nostr.kind.5.delete_kind_7_reaction` with `exclusive: false` (scoped to
context `deletes-kind-7-reaction`). This claim covers:

- Reaction retraction intent: the `UnreactModule` identifies which reaction to
  delete and validates the `reaction_event_id` is 64-hex.
- Deleted reaction id validation: the pre-validation before calling nip09.

`nmp-nip25` does NOT build the kind:5 wire event. It calls
`nmp_nip09::build_deletion_draft` and receives an `OwnedDeletionDraft` carrying
`nmp-nip09` artifact provenance.

### Publish gate

`crates/nmp-ownership/src/lib.rs::validate_publish_ownership` now enforces a
generic rule:

```
if kind == 5 { require artifact("nmp.nip09", "nostr.kind.5.deletion") }
```

The previous narrow gate (`kind == 5 && deletes_kind_7_reaction(tags)`) is
replaced by this generic check. The `deletes_kind_7_reaction` helper is deleted;
the publish gate does not inspect k-tags to decide which owner to require.

### Composition

`nmp-nip09` exposes `Nip09Descriptor` (a `ProtocolDescriptor` over the generic
`nmp.nip09.delete` action) for explicit app/runtime composition roots to register
alongside other protocol descriptors (ADR-0069 explicit composition; the
`nmp-defaults` bundle was removed in #2546). It is registered as a yielding
default, so an app that pre-registers a custom deletion handler overrides it.
The descriptor is opt-in: reaction retraction (the immediate driver) flows
through `nmp-nip25`'s delegation to the `nmp-nip09` builder and does not depend
on `Nip09Descriptor` being registered.

## Consequences

Positive:

- Single point of kind:5 construction. No crate hand-builds `e`/`k` tags for
  deletion events except through the `nmp-nip09` builder.
- Generic publish gate: any future crate that needs to delete something (NIP-29
  group management, NIP-57 zap cancellation, etc.) calls the same builder and
  passes the same gate without a new tag-sniffing predicate.
- `deletion_targets` centralises `e`-tag parsing: `nmp-nip25`'s
  `ReactionProjection::ingest_delete` and `ReactionAggregateProjection::ingest_delete`
  both delegate to it.
- Reaction retraction behaviour is **unchanged**: the composed path produces the
  same kind:5 wire event; only the provenance token changes from the nip25 claim
  to the nip09 claim.
- Separates intent (`nmp-nip25` knows which reaction to delete) from
  construction (`nmp-nip09` knows how to build the deletion event), matching
  ADR-0071's intent-vs-publish-stage separation.

Negative / trade-offs:

- `nmp-nip25` gains a new direct dependency on `nmp-nip09`, adding one edge to
  the dependency graph. This is unavoidable given the delegation requirement and
  is consistent with how nip-crates depend on each other.

## Related

- #2511 — tracking issue for this work.
- #2506 — composable ownership doctrine (intent-vs-artifact split).
- ADR-0071 — write intents, composable drafts, and route provenance.
