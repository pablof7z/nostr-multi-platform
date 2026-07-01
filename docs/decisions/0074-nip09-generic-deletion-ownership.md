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
- `a` tag grammar (address-coordinate deletion targets) and the
  `AddressCoordinate` (`kind:pubkey:d`) type that decodes it — see "Amendment"
  below.
- Deletion content/reason rules (content = human-readable reason).
- Deletion identity rules (at least one target event id; id must be hex-64).
- Generic deletion read seam: `DeleteRecord::try_from_kernel_event(event)` —
  the shared decode that projections call instead of hand-parsing `e`/`a`/`k`
  tags.
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
- The shared read seam (`DeleteRecord::try_from_kernel_event`, née
  `deletion_targets`) centralises tag parsing: `nmp-nip25`'s
  `ReactionProjection::ingest_delete` and `ReactionAggregateProjection::ingest_delete`
  both delegate to it, as do `nmp-nip18`, `nmp-content`, `nmp-note-feed`, and
  `nmp-nip68` after the Amendments below.
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

## Amendments

### 2026-07-01 — `a`-tag grammar and `AddressCoordinate` (#2589)

The original decision scoped `nmp-nip09`'s read seam to `deletion_targets(tags)`
— `e`/`k` tags only. That undersold the crate's own charter: `nmp-nip18` had a
richer, unofficial `DeleteRecord` (author, created_at, event targets, AND
address-coordinate targets) predating `nmp-nip09` entirely (#1995, merged before
ADR-0074 shipped). Two crates with nothing to do with reposts —
`nmp-content` (NIP-23 article kind:30023 delete-folding) and `nmp-note-feed`
(kind:1 note delete-folding) — imported `nmp_nip18::DeleteRecord` directly for
generic kind:5 decoding, recreating the exact cross-crate layering leak this ADR
was written to prevent, one hop over. `nmp-nip68` (kind:20 picture feeds) had
done the same independently.

This amendment folds that drift back into the exclusive claim rather than
opening a competing decoder:

- `nmp-nip09::DeleteRecord` (replacing `DeletionTargets`/`deletion_targets`)
  is now the canonical decode: `author`, `created_at`, `event_targets`,
  `address_targets: Vec<AddressCoordinate>`, and `kinds`. It decodes a whole
  `KernelEvent` (not just a tag slice) so the author/timestamp needed for
  same-author-retracts and `created_at <=` version-ordering comparisons ride
  along with the targets — callers no longer separately thread `event.author`/
  `event.created_at` past the decoder.
- `nmp-nip09::AddressCoordinate` (the `kind:pubkey:d` address-coordinate type,
  moved from `nmp-nip18::coordinate`) is now canonical here too. This was the
  one open design question: `AddressCoordinate` was NOT deletion-only in
  `nmp-nip18` — it also identified repost targets (`RepostRecord.target_address`,
  `RepostTarget::Address`) and was reused by `nmp-content`/`nmp-native-runtime`
  to compute unrelated addressable-event identity (article rows, naddr
  `primary_id`). But the type itself is generic NIP-01 `a`-tag grammar, not
  repost- or article-specific logic — and `nmp-nip09` is the crate whose
  exclusive claim already covers `a`-tag *deletion* grammar, so it is the
  natural single owner of the `a`-tag identity primitive full stop. `nmp-nip18`
  keeps its own repost-target derivation logic (embedded-event coordinate
  proof, generic-repost `a`-tag proof) — only the shared identity type moved,
  not nip18's repost-specific reasoning about when a coordinate is proven.
- `nmp-nip18`, `nmp-content`, `nmp-note-feed`, and `nmp-nip68` all repoint
  their delete-folding at `nmp_nip09::DeleteRecord` and their coordinate
  identity at `nmp_nip09::AddressCoordinate`. `nmp-nip18`'s own
  `nostr.kind.5.delete_for_repost_projection` non-exclusive claim is unchanged
  — it still folds deletes into repost-activity rows, just through the
  upgraded shared decoder instead of its own copy.
- No compat alias: `nmp-nip18::coordinate` and `nmp-nip18::delete` are deleted
  outright, not re-exported. Every call site was repointed in the same change.

## Related

- #2511 — tracking issue for this work.
- #2589 — `a`-tag grammar / `AddressCoordinate` amendment tracking issue.
- #2506 — composable ownership doctrine (intent-vs-artifact split).
- ADR-0071 — write intents, composable drafts, and route provenance.
