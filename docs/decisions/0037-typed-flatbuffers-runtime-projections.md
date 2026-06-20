# ADR-0037 — Typed FlatBuffers sidecar for high-volume runtime projections

> Note (ADR-0046, 2026-06-12): the `nmp-app-template` crate named below was
> renamed to `nmp-defaults`. Read `nmp-app-template` / `nmp_app_template` here
> as `nmp-defaults` / `nmp_defaults`.
>
> Current status (2026-06-16): ADR-0044 and the PR-B/F-05/F-10 work superseded
> this ADR's permanence claim for `payload:Value`; the follow-up cleanup removed
> `SnapshotFrame.payload` and the `Value`/`Pair` variant tree from
> `nmp_update.fbs`. Production `UpdateFrame` snapshots carry typed
> `SnapshotEnvelope` fields plus `typed_projections` sidecars only. The text
> below is the historical migration decision record.

- **Status:** Accepted / Implemented — the typed sidecar shipped; the
  `payload:Value` permanence claim is **superseded by ADR-0044** (the generic
  `Value` tree was removed from `nmp_update.fbs`). See the status note above.
- **Relates to:** the FlatBuffers update-transport envelope (commits `021ba295`
  "Replace update transport with FlatBuffers" and `716eac9c` "Address FlatBuffers
  transport review feedback"), ADR-0032 (raw-data projection doctrine), ADR-0033
  (`nmp-feed` viewport FFI — owner of the `nmp.feed.home` projection key), ADR-0018
  (content-tree FFI projection). Cautionary precedent: ADR-0025 (Marmot bespoke
  FFI cluster — the app-coupling-in-the-transport anti-pattern this ADR's
  union-free design avoids recurring).
- **Scope:** the `nmp-core` transport schema
  (`crates/nmp-core/schema/nmp_update.fbs`) and its checked-in bindings; the
  snapshot-projection emission path; typed protocol/feed schemas for
  `nmp.feed.home`; and every platform host that consumes the NMP update stream
  (iOS Chirp, chirp-tui, web, Android gallery).

## Context

The NMP update transport already uses FlatBuffers for the **envelope**:
`UpdateFrame` wraps a `SnapshotFrame` or a `PanicFrame`, with `file_identifier
"NMPU"` (landed via commit `021ba295` "Replace update transport with
FlatBuffers"). At the time of this decision, the **payload content** inside a
`SnapshotFrame` was a generic, self-describing `Value` tree (the JSON-equivalent
variant `Null | Bool | Int | UInt | Float | String | List | Map`). Every
projection — including high-volume ones like the Chirp home feed — was
serialized into that tree and walked field-by-field by each host on every
snapshot tick. (ADR-0044 later removed the `Value` tree from the schema
entirely; see the status note above. This section is the historical motivation.)

That generic tree was the right substrate for the long tail of low-frequency
projections: it is fully app-agnostic, needs no per-projection schema, and a host
can decode an unknown projection by walking the map. It was the wrong substrate for
the **hot path**. The home feed re-serializes a list of event cards (author hex,
content tree, timestamps, reaction/zap counts, embedded-event cards) into a
deeply-nested `Value`/`Pair`/`List`/`Map` structure on every tick, and each host
re-walks that structure by string-keyed map lookups. The cost shows up twice: in
serialization width on the Rust side, and in keyed-lookup / allocation churn on
the host side — exactly where the 4 Hz snapshot model is most load-sensitive (the
single highest-risk performance bet in the architecture).

The natural fix is a typed payload: a flat FlatBuffers table for the feed projection
that a host reads by field offset with zero string-keyed lookups. The design tension
is **where the typed schema lives**. The `nmp.feed.home` projection is fed by
`nmp-nip01::ModularTimelineSnapshot` with viewport/cursor mechanics owned by
`nmp-feed` (ADR-0033). A naïve FlatBuffers approach would add a union over every
app projection type directly into `nmp_update.fbs` — recreating in the transport
schema exactly the app coupling that ADR-0025 named as an anti-pattern and that the
generic `dispatch_action` / generic-projection seams exist to prevent. `nmp-core`
transport must not learn the names `Feed`, `Dm`, `Group`, or `Article`.

## Decision

Add a **typed projection sidecar** to `SnapshotFrame`. (As decided here, the
sidecar was additive alongside the generic `payload:Value`; ADR-0044 later removed
the generic tree, making the typed sidecar the sole host-visible projection
carrier. The historical additive framing is preserved below for context.)

### Commitment 1 — the sidecar is opaque bytes keyed by a schema URI, never a union

`nmp-core` transport gains two new tables and one new field. The discriminator is a
`(schema_id, schema_version, file_identifier)` triple of scalars/strings, **not** a
FlatBuffers union over app types. `nmp-core` sees an opaque `[ubyte]` blob plus that
descriptor; it never declares — and never needs to regenerate bindings for — any
app's table shape.

```fbs
// New in nmp_update.fbs (namespace nmp.transport)

// A single typed projection payload. The payload bytes are an app/protocol-owned
// FlatBuffers buffer; nmp-core transport treats them as opaque. The schema_id +
// schema_version + file_identifier describe the buffer's root schema so a host
// knows which decoder to apply. nmp-core never declares the app table type — that
// is the whole point.
table TypedPayload {
  schema_id:string;          // e.g. "nmp.nip01.timeline"
  schema_version:uint = 1;   // bumped by the schema-owning crate on shape change
  file_identifier:string;    // the app FlatBuffers file_identifier (e.g. "NFHM")
  payload:[ubyte];           // opaque to nmp-core; host-decoded via the descriptor
}

// Binds a typed payload to a projection key (the same key space used by
// nmp_app_register_snapshot_projection — e.g. "nmp.feed.home").
table TypedProjection {
  key:string;                // projection key, e.g. "nmp.feed.home"
  payload:TypedPayload;
}

table SnapshotFrame {
  schema_version:uint = 1;
  typed_projections:[TypedProjection];    // typed sidecar, may be empty/absent
}
```
(The `payload:Value` field shown in earlier revisions of this ADR was removed
from `nmp_update.fbs` by ADR-0044.)

The reason this descriptor beats a FlatBuffers union: a union would force every
app's root table to be declared inside `nmp_update.fbs` and force a `nmp-core`
binding regeneration (and a FlatBuffers runtime-pin bump on every platform) for
every new typed projection any app ever adds. The opaque-bytes-plus-descriptor
shape keeps the transport schema closed against app churn — new typed projections
land entirely in app/protocol crates with **zero** edits to `nmp-core`. The
`file_identifier` lets a host cheaply reject a buffer whose root schema is not the
one it expects before attempting to decode.

### Commitment 2 — app/protocol crates own their typed schemas

No app-specific schema lives in `nmp-core`. The typed FBS schema for a projection
lives in the crate that owns the projection's data shape, with its own checked-in
bindings and its own FlatBuffers runtime pin (subject to the same
`ci/check-flatbuffers-version-pins.sh` discipline as the transport schema).

For the `nmp.feed.home` pilot:

- `nmp-feed` owns cursor/page/window semantics and the typed structural envelope at
  `crates/nmp-feed/schema/feed_home.fbs` (`schema_id "nmp.feed.window"`,
  `file_identifier "NFWM"`). Protocol crates must not duplicate cursor/page tables.
- `nmp-content` owns the typed content-tree buffer (`schema_id
  "nmp.content.tree"`, `file_identifier "NFCT"`).
- `nmp-nip01` owns the timeline/card/content-render schema for the home-feed pilot
  at `crates/nmp-nip01/schema/timeline_snapshot.fbs`.

The pilot descriptor carried in `TypedProjection` is `key "nmp.feed.home"`,
`schema_id "nmp.nip01.timeline"`, `schema_version 1`, `file_identifier "NFTS"`.
Inside the NFTS buffer, nmp-nip01 embeds typed nmp-content (`NFCT`) buffers for
content trees and a typed nmp-feed (`NFWM`) buffer for the feed window. The
`schema_version` is owned by the schema-owning crate and bumped when the typed
table shape changes in a way a host must distinguish.

### Commitment 3 — raw data only, same as ADR-0032

The typed payload changes the **encoding**, never the **content contract**. Every
field in a typed projection follows ADR-0032: pubkeys as 64-char lowercase hex,
timestamps as Unix `u64` seconds, counts as raw integers, display names verbatim
from kind:0 (absent when unseen), picture URLs verbatim. The banned
`nmp_core::display::*` forwarders are no more permitted in a typed projection than
in a `Value`-tree projection. Typing is a transport optimization, not a license to
pre-format.

The pilot is a strict typed slice: no production JSON subpayloads are allowed
inside the typed `nmp.feed.home` payload. Once a field is inside the
NFTS/NFCT/NFWM sidecar path it is represented by typed FlatBuffers tables or
typed FlatBuffers sub-buffers.

> The original ADR carried a "Commitment 4 — host preference and fallback
> contract" plus a "Migration & staged removal window" describing dual emission
> (typed sidecar + generic `Value` subtree) and per-key removal of the generic
> tree. ADR-0044 removed the generic tree wholesale, so there is no fallback
> path and no dual emission: a host-visible projection has a typed sidecar or it
> is not host-visible. The superseded text is in git history.

## Consequences

### What this enables

- Hot-path projections (starting with the home feed) decode by field offset with
  zero string-keyed map lookups on the host, and serialize as a flat buffer on the
  Rust side — directly attacking the 4 Hz snapshot cost on the most load-sensitive
  projection.
- New typed projections are added entirely within app/protocol crates. `nmp-core`
  transport never grows an app noun and never regenerates bindings for an app's
  table — the transport schema is closed against app churn (the structural opposite
  of the ADR-0025 bespoke-cluster anti-pattern).

### Registration seam lives on the `AppHost` trait (not concrete-only)

`register_typed_snapshot_projection` is on the `AppHost` trait
(`crates/nmp-core/src/substrate/app_host.rs`), mirroring the generic
`register_snapshot_projection`, and is implemented by both `AppHost` impls —
`NmpApp` (`crates/nmp-ffi/src/lib.rs`) and `NmpAppBuilder` (delegating into the
same shared registry, `crates/nmp-app-template/src/builder.rs`). It was
originally concrete-only on `NmpApp` (`crates/nmp-ffi/src/snapshot.rs`).
**Rationale:** reusable protocol/feed crates register their substrate pieces
through `register_runtime(app: &impl AppHost)` (e.g. `nmp-wot`,
`nmp-app-template::register_defaults`), so they only ever see the trait. A
concrete-only typed seam meant those crates could register a *generic* `Value`
projection but not a *typed* one — which would block them from completing the
JSON→typed snapshot migration this ADR drives. Promoting the seam to the trait
is a pure mechanism addition (no projection migrated by it); the closure returns
`Option<TypedProjectionData>` and the trait already carries generic closure
methods, so adding one more generic method changes no object-safety property.

### What this does NOT change

- **Historical only:** this ADR originally said `payload:Value` stayed in
  `SnapshotFrame` permanently. ADR-0044 and the 2026-06-16 schema cleanup reverse
  that commitment. The current update transport has no generic payload tree and
  no compatibility slot; host-visible projections need typed sidecars.
- The `UpdateFrame` / `SnapshotFrame` / `PanicFrame` envelope shape, the `"NMPU"`
  file identifier, and `schema_version` semantics — unchanged. `typed_projections`
  is an additive optional vector; an absent or empty vector is a valid frame.
- ADR-0032 — typed projections carry raw protocol data only.
- The generic snapshot-projection key space and the
  `nmp_app_register_snapshot_projection` seam — `TypedProjection.key` reuses the
  same keys.

### Legacy diagnostics path

The former `nmp_app_chirp_snapshot` JSON pull helper has been **deleted** (the
`nmp-app-chirp` crate is gone; see ADR-0039 on the push-projection seam). There
is no JSON snapshot pull path; host-visible state is read from the typed update
stream.

### Risks

- **FlatBuffers runtime-pin asymmetry.** The platforms run different FlatBuffers
  runtime lines (Rust+Swift `25.12.19`, web/TypeScript `25.9.23`, Android/Kotlin
  `25.2.10` per the `nmp_update.fbs` header). Because each app/protocol crate owns
  its typed schema and checked-in bindings, every such schema must observe the same
  per-platform pin discipline enforced by `ci/check-flatbuffers-version-pins.sh`.
  This is the largest ongoing maintenance cost of the design.
- **Schema-version skew.** A host on a newer `schema_version` than the emitter, or
  vice versa, must treat an unrecognized descriptor as "no typed payload
  available" (the projection is simply absent for that host) rather than
  mis-decode. With the generic `Value` tree removed (ADR-0044), there is no
  fallback representation, so the schema-owning crate bumps `schema_version` only
  for shape changes a host must distinguish.

## Pilot

`nmp.feed.home` — the Chirp home feed, currently projected from
`nmp-nip01::ModularTimelineSnapshot` with viewport mechanics in `nmp-feed`
(ADR-0033). The typed projection key is `nmp.feed.home`; the typed payload
descriptor is `schema_id "nmp.nip01.timeline"`, `schema_version 1`,
`file_identifier "NFTS"`. The NFTS payload embeds the nmp-feed `FeedWindow`
typed buffer for page/cursor/window data and nmp-content `ContentTreeWire`
typed buffers for content trees. It is chosen because it is the highest-volume
projection and the one whose host-side `Value`-tree walk is the most expensive
on the 4 Hz tick.

## Rollout order

Typed-read adoption proceeds **iOS → TUI → Android** for the v1 native surface,
then **web/TypeScript** after the post-v1 web/wasm milestone:

1. **iOS** (Chirp) — primary showcase, newest FlatBuffers runtime (`25.12.19`).
2. **TUI** (chirp-tui) — same Rust-side runtime line, no separate codegen toolchain.
3. **Android/Kotlin** — runtime `25.2.10`, the oldest pin, so it ships after
   the typed schema has stabilized on the platforms with newer runtimes.
4. **Web/TypeScript** — post-v1 runtime lane; current pin remains documented in
   `nmp_update.fbs` when the web milestone resumes.
