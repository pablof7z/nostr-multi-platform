# ADR-0037 — Typed FlatBuffers sidecar for high-volume runtime projections

- **Status:** Proposed (2026-05-28)
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
FlatBuffers"). But the **payload content** inside a `SnapshotFrame` is still a
generic, self-describing `Value` tree:

```fbs
table SnapshotFrame {
  schema_version:uint = 1;
  payload:Value;            // generic Value/Pair/List/Map tree
}
```

`Value` is the JSON-equivalent variant tree (`Null | Bool | Int | UInt | Float |
String | List | Map`). Every projection — including high-volume ones like the
Chirp home feed — is currently serialized into this tree and walked field-by-field
by each host on every snapshot tick.

This generic tree is the right substrate for the long tail of low-frequency
projections: it is fully app-agnostic, needs no per-projection schema, and a host
can decode an unknown projection by walking the map. It is the wrong substrate for
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

Add a **typed projection sidecar** to `SnapshotFrame`, alongside — not replacing —
the existing generic `payload:Value`. The sidecar is **additive and
backward-compatible**: a host that does not understand the typed payload continues
to read `payload`.

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
  payload:Value;                          // compatibility during migration — permanent
  typed_projections:[TypedProjection];    // new: typed sidecar, may be empty/absent
}
```

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
inside the typed `nmp.feed.home` payload. Low-frequency, not-yet-typed
projections may continue to use the generic `payload:Value` tree, but once a
field is inside the NFTS/NFCT/NFWM sidecar path it is represented by typed
FlatBuffers tables or typed FlatBuffers sub-buffers.

### Commitment 4 — the host preference and fallback contract

For a given projection key, a host applies this rule per snapshot:

1. If `typed_projections` contains an entry whose `key` matches the projection the
   host wants **and** whose `payload` descriptor (`schema_id` + `schema_version` +
   `file_identifier`) names a schema the host can decode, the host **MUST** prefer
   the typed payload and **MUST ignore** the corresponding subtree under
   `payload:Value`.
2. Otherwise (no typed entry, or an unrecognized descriptor), the host falls back
   to walking `payload:Value`.

During migration, the emitter produces **both** representations for a piloted key:
the typed sidecar entry **and** the generic `Value` subtree. This is what makes the
change backward-compatible — an un-migrated host on an older binary keeps working
unchanged off `payload`, while a migrated host transparently upgrades to the typed
read. The generic subtree for a piloted key is only dropped once the per-key
**staged removal window** closes (see Consequences).

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

### What this does NOT change

- `payload:Value` stays in `SnapshotFrame` **permanently**. It remains the sole
  representation for every projection that is not (yet) typed — the long tail of
  low-frequency projections never needs a typed schema. This ADR does **not**
  schedule removal of the generic tree.
- The `UpdateFrame` / `SnapshotFrame` / `PanicFrame` envelope shape, the `"NMPU"`
  file identifier, and `schema_version` semantics — unchanged. `typed_projections`
  is an additive optional vector; an absent or empty vector is a valid frame.
- ADR-0032 — typed projections carry raw protocol data only.
- The generic snapshot-projection key space and the
  `nmp_app_register_snapshot_projection` seam — `TypedProjection.key` reuses the
  same keys.

### Migration & staged removal window

Removal of a piloted key's generic `Value` subtree is **per key**, never global.
For each piloted projection key, the emitter emits both the typed sidecar and the
generic subtree until **every** platform host ships a decoder for that key's
current descriptor. When all four hosts (iOS, TUI, web, Android) have shipped the
decoder, the emitter may stop emitting the generic subtree for that key. Until then,
both are emitted. No global flag day; each key migrates on its own schedule.

### Legacy diagnostics path

`nmp_app_chirp_snapshot` (returns a JSON C string, `*mut c_char`, defined at
`apps/chirp/nmp-app-chirp/src/ffi/snapshot.rs:14`) is **quarantined as
diagnostics-only and is NOT removed at runtime**. It still has live callers in the
REPL (`apps/chirp/chirp-repl/src/app.rs:237`) and the Android FFI smoke shim
(`crates/nmp-android-ffi/src/lib.rs:222`). It is not part of the typed-projection
path and is not a showcase render path (the showcase home-feed path already reads
the `"nmp.feed.home"` projection from the update stream per ADR-0033). It remains a
legacy pull helper for REPL/tests/diagnostics.

### Risks

- **FlatBuffers runtime-pin asymmetry.** The platforms run different FlatBuffers
  runtime lines (Rust+Swift `25.12.19`, web/TypeScript `25.9.23`, Android/Kotlin
  `25.2.10` per the `nmp_update.fbs` header). Because each app/protocol crate owns
  its typed schema and checked-in bindings, every such schema must observe the same
  per-platform pin discipline enforced by `ci/check-flatbuffers-version-pins.sh`.
  This is the largest ongoing maintenance cost of the design.
- **Dual emission during migration** temporarily widens the wire (typed sidecar +
  generic subtree) for piloted keys. Bounded: it ends per key when the staged
  removal window closes, and it only applies to keys that have opted into typing.
- **Schema-version skew.** A host on a newer `schema_version` than the emitter, or
  vice versa, must fall back to `payload:Value` rather than mis-decode. The
  preference/fallback contract (Commitment 4) makes this safe — an unrecognized
  descriptor is treated as "no typed payload available."

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

Typed-read adoption proceeds **iOS → TUI → web → Android**:

1. **iOS** (Chirp) — primary showcase, newest FlatBuffers runtime (`25.12.19`).
2. **TUI** (chirp-tui) — same Rust-side runtime line, no separate codegen toolchain.
3. **Web/TypeScript** — runtime `25.9.23`.
4. **Android/Kotlin** — runtime `25.2.10`, the oldest pin, so it ships last after
   the typed schema has stabilized on the platforms with newer runtimes.

The generic `Value` subtree for `nmp.feed.home` is only retired once **all four**
have shipped the `nmp.feed.home` v1 decoder.
