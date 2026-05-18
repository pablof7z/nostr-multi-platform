# ADR-0017 — D1 placeholder contract: `Placeholder<T>` newtype for always-renderable display fields

**Date:** 2026-05-18
**Status:** Accepted (T64 — substrate gap: TimelineItem D1 placeholder contract)
**Doctrines invoked:** D1 (best-effort rendering — render now, refine in place)

## Context

D1 mandates that every view payload field carries a value at all times.  The
Rust type system is the enforcement layer: display fields must be non-`Option`
so the wrong thing (returning `null` to Swift/Kotlin) is a compile error.

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

## Decision

### Option chosen: migrate `Option<String>` display fields to `String` (D1-aligned)

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

### Fields migrated

| Type | Field | Before | After |
|------|-------|--------|-------|
| `TimelineItem` | `author_picture_url` | `Option<String>` | `String` |
| `ProfileCard` | `picture_url` | `Option<String>` | `String` |

`Profile` (the internal cache struct, never serialised) keeps `picture_url:
Option<String>` — `None` correctly models "kind:0 has not arrived".  The
`Option` is collapsed to a non-Option `String` only at the projection boundary
in `timeline_item()` and `profile_card_for()`.

### C13 test — resolved

The `#[should_panic]` marker is removed.  C13 now drives the kernel via
`spawn_actor` + `IngestPreVerifiedEvents` and asserts:

1. `author_picture_url` in the emitted JSON is a non-null, non-empty string.
2. It starts with `"identicon:"` when no kind:0 has arrived.
3. `author_avatar_source` is `"placeholder"`.

A companion kernel-internal test (`c13_kernel_*` in `kernel/tests.rs`) verifies
the in-place refinement: once a `Profile` with a real `picture_url` is inserted,
`timeline_item()` returns the real URL for the same event.

## Consequences

### Positive

- D1 is enforced at compile time for the two highest-traffic display fields.
- The FFI boundary never emits `null` for picture URLs — Swift callers can
  unconditionally render an identicon or image.
- C13 is an active, passing test (not a `#[should_panic]` document).
- `Placeholder<T>` is available for future display fields that need the same
  always-renderable guarantee.

### Negative / constraints

- Swift structs still declare `pictureUrl: String?` and `authorPictureUrl: String?`.
  Changing them to `String` is a follow-up task (T65 or later): the existing
  `String?` decoders accept non-null strings without breaking, so no FFI breakage
  occurs from this change alone.
- `picture_placeholder` is not a true identicon URI — it does not encode image
  data.  Rendering falls back to the avatar initials + color fields that already
  exist on every item.  A richer identicon-as-data-URI could replace it later
  without breaking the field's non-Option contract.

## Alternatives rejected

### A — expose `TimelineItem` for integration-test access only

Rejected: would not fix the underlying D1 violation.  The type would still be
`Option<String>` in the wire format, so iOS could still receive `null`.

### C — `Placeholder<T>` as tagged enum (`Pending | Authoritative(T)`)

Rejected: the `author_avatar_source` field (`"placeholder"` | `"kind0"`) already
serves as the discriminator on the wire.  Adding a tagged enum here would
duplicate that signal, change the serialised shape (JSON object instead of bare
string), and break existing Swift decoders.
