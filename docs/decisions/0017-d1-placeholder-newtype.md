# ADR-0017 — D1 placeholder contract: `Placeholder<T>` newtype for always-renderable display fields

**Date:** 2026-05-18
**Status:** Historical; superseded by ADR-0032 raw-data projection doctrine.
Do not use this ADR as current guidance for `TimelineItem` or `ProfileCard`
picture fields.
**Doctrines invoked:** D1 (best-effort rendering — render now, refine in place)

## Context

This ADR originally interpreted D1 as requiring every display field to carry a
non-optional value at all times. ADR-0032 later corrected the projection
contract: raw absent facts stay absent (`Option<String>`), and presentation
layers render missing-picture UI without Rust inventing placeholder protocol
values.

`TimelineItem.author_picture_url` was `Option<String>`, which:

1. Allowed `null` to cross the FFI boundary — a D1 violation detectable by
   C13 (`framework_magic_contract/c5_c8_c13.rs`).
2. Required Swift callers to branch on `Optional<String>` instead of always
   rendering something.
3. Forced the UI to special-case "no profile picture yet" instead of letting
   the placeholder value guide rendering.

`ProfileCard.picture_url` had the same violation.

The C13 test was `#[should_panic]`-documented as a substrate gap
(`#57-c13-gap`) because the field type made it impossible to write a passing
assertion that `author_picture_url` is always non-null.

## Historical Decision

### Option chosen at the time: migrate `Option<String>` display fields to `String`

Option (a) — simply re-export `TimelineItem` for integration-test access — was
rejected because it would have unlocked tests against the *current broken
shape*; the type would still allow `None` in the serialised JSON.

Option (b) — migrate display fields from `Option<String>` to `String`, with a
deterministic placeholder at the projection boundary — was chosen.  It makes
the D1 invariant a compile-time guarantee rather than a runtime discipline.

### `Placeholder<T>` newtype (`nmp_core::substrate::placeholder`)

A zero-cost `Placeholder<T>` newtype is introduced in `nmp_core::substrate::placeholder`:

```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Placeholder<T>(pub T);
```

It serialises as the inner `T` (bare string, `#[serde(transparent)]`) so the
JSON wire format seen by Swift/Kotlin is unchanged: `String?` decoders accept a
non-null `String` without modification.

`Placeholder<T>` implements `Display`, `Deref<Target = T>`, and `AsRef<str>`.
A helper `picture_placeholder(pubkey: &str) -> String` produces the canonical
picture-URL placeholder:

```
identicon:<first-16-hex-chars-of-pubkey>
```

The `identicon:` scheme prefix is:

- **Deterministic** — same pubkey → same value → SwiftUI diffing never fires
  spurious updates.
- **Detectable** — the prefix lets the UI decide to show avatar initials +
  color instead of attempting a network fetch.
- **Non-empty** — satisfies D1's "always renderable" invariant.

### Historical fields migrated

This ADR records the placeholder-newtype experiment. Current raw-data
projection doctrine (ADR-0032) keeps absent profile pictures as
`Option<String>` at the projection boundary; do not reintroduce placeholder
strings as a parallel source of truth.

`Profile` (the internal cache struct, never serialised) keeps `picture_url:
Option<String>` — `None` correctly models "kind:0 has not arrived".

### C13 test — resolved

The `#[should_panic]` marker is removed.  C13 now drives the kernel via
`spawn_actor` + `IngestPreVerifiedEvents` and asserts:

The kernel-internal `timeline_item()` helper has been deleted; the remaining
current guard is `ProfileCard::picture_url`, which surfaces `None` when a
profile omits a picture.

## Consequences

### Current contract

- `TimelineItem.author_picture_url` and `ProfileCard::picture_url` are
  `Option<String>` projection fields.
- Missing pictures are not represented as `identicon:` strings in Rust-owned
  projection data.
- The remaining current kernel guard is `ProfileCard::picture_url`, which
  surfaces `None` when a profile omits a picture.

### Remaining utility

- `Placeholder<T>` still exists as a general helper, but it is not the current
  contract for `TimelineItem` or `ProfileCard` picture fields.

## Alternatives rejected

### A — expose `TimelineItem` for integration-test access only

Rejected: would not fix the underlying D1 violation.  The type would still be
`Option<String>` in the wire format, so iOS could still receive `null`.

### C — `Placeholder<T>` as tagged enum (`Pending | Authoritative(T)`)

Rejected at the time: a tagged enum would have duplicated projection-level
provenance. Current projections that need provenance should carry one
projection-owned discriminator rather than duplicating that signal inside this
wrapper.
