# ADR-0074: NIP-09 deletion artifact ownership

## Decision

`nmp-nip09` is the exclusive owner of NIP-09 kind:5 deletion artifact
construction and canonical deletion decode.

`nmp-nip09` owns:

- kind:5 deletion draft and event construction;
- `e`, `k`, and `a` tag grammar for deletion artifacts;
- deletion reason/content rules;
- target validation for deletion wire artifacts;
- canonical decode through `DeleteRecord::try_from_kernel_event`;
- `AddressCoordinate` for generic `a`-tag identity;
- artifact provenance required by the publish ownership gate for kind:5 events.

Other crates may request deletion of artifacts they own, or fold deletion facts
into their own projections. They do not build kind:5 wire events, duplicate
deletion tag grammar, or mint deletion artifact provenance.

Intent ownership and artifact ownership are separate. For example, a reaction
crate owns reaction retraction intent, while `nmp-nip09` owns the kind:5
deletion artifact that carries the retraction on the wire.

## Context

Kind:5 deletion is generic NIP-09 wire grammar. Letting each crate that wants to
delete something construct its own kind:5 event creates duplicate grammar,
duplicate provenance, and cross-crate layering leaks.

The same rule applies to decoding. Projections that need deletion facts should
share one canonical decoder instead of importing another protocol crate's
private projection helper.

## Consequences

Future crates that need deletion call the same builder and pass the same publish
ownership gate. Delete-folding projections use the same decoder and coordinate
type.

Crates that previously owned local deletion helpers depend on `nmp-nip09` for
wire artifacts. That dependency is correct because the helper is generic NIP-09
mechanism, not product intent.

## Boundaries

Permitted:

- protocol or app crates identifying which owned artifact should be deleted;
- protocol or app crates wrapping deletion in their own higher-level workflow;
- projections folding `nmp-nip09::DeleteRecord` into their rows;
- publish code requiring `nmp-nip09` artifact provenance for kind:5 events.

Forbidden:

- building kind:5 wire events outside `nmp-nip09`;
- duplicating deletion `e`, `k`, or `a` tag grammar;
- hand-parsing generic deletion facts in unrelated protocol crates;
- treating deletion intent as permission to own deletion wire construction.

## Enforcement

Ownership gates require `nmp-nip09` artifact provenance for kind:5 publish
paths. Crate ownership audits check that generic deletion grammar and
`AddressCoordinate` stay in `nmp-nip09`.

Reviewers grep for kind:5 builders, deletion tag parsing, and address-coordinate
types outside `nmp-nip09`; those sites must be intent wrappers, projection
folding through `DeleteRecord`, or tests.

## Related

- [ADR-0071](0071-write-intents-and-route-provenance.md) - intent,
  finalization, signing, publishing, and route provenance.
- [docs/product-spec/doctrine.md](../product-spec/doctrine.md) - ownership
  doctrine.
- #2506 - composable ownership doctrine.
- #2511 - deletion artifact owner.
- #2589 - `a`-tag grammar and `AddressCoordinate`.
- #2746 - ADR current-only cleanup.
