# ADR-0038 — Typed FlatBuffers sidecar for the OP-centric home feed

- **Status:** Proposed (2026-05-29)
- **Relates to:** ADR-0037 (typed FlatBuffers runtime projections — the
  sidecar transport, the `(schema_id, schema_version, file_identifier)`
  descriptor, the host preference/fallback contract this ADR instantiates a
  second time); ADR-0035 (generic `RootIndexedFeed<R, A, C>` engine — owner of
  `RootFeedSnapshot` / `RootCard`); ADR-0036 (composition-root follow-set
  expansion — owner of the registration site that will emit the typed sidecar);
  ADR-0033 (`nmp-feed` viewport FFI — owner of the `nmp.feed.home` key and the
  `NFWM` feed-window typed buffer); ADR-0032 (raw-data projection doctrine);
  ADR-0018 (content-tree FFI projection — owner of the `NFCT` content-tree
  typed buffer). Cautionary precedent: ADR-0025 (Marmot bespoke FFI cluster —
  the app-coupling-in-transport anti-pattern the descriptor design keeps out of
  `nmp-core`).
- **Scope:** the typed FlatBuffers payload for `nmp.feed.home` *after* the V-80
  OP-centric migration (rungs 1–7 + V-82). Concretely: a new `nmp-nip01`-owned
  `.fbs` schema and checked-in bindings; the typed-projection registration call
  in `nmp-app-template::register_op_feed_defaults`; the three platform decoders
  that read `nmp.feed.home` (chirp-tui `snapshot.rs`, iOS `TypedHomeFeedDecoder`,
  Android `TypedHomeFeedDecoder`); the NFTS-for-feed test surface that this ADR
  retires; and the rung-7 generic-`Value` emission (PR #747), which this ADR
  keeps as the permanent fallback layer.
- **Does NOT change:** the `nmp_update.fbs` transport schema, the `"NMPU"`
  envelope, the `TypedProjection` / `TypedPayload` sidecar tables, the
  `TypedProjectionFn` registry, or any `nmp-core` binding. This ADR adds a
  second typed *descriptor* inside the seam ADR-0037 already built; it does not
  touch the seam itself.

---

## Context

### Two workstreams collided on one projection key

ADR-0037 (PR #739, merged as `372a6ddb`) made `nmp.feed.home` the pilot for a
typed-FlatBuffers hot-path sidecar. It shipped the codec
(`nmp_nip01::typed_wire::encode_modular_timeline_snapshot`, schema
`crates/nmp-nip01/schema/timeline_snapshot.fbs`, descriptor `schema_id
"nmp.nip01.timeline"` / `file_identifier "NFTS"` / version 1), the three
platform decoders, and golden-wire tests — all keyed to the **old** feed shape,
`nmp_nip01::ModularTimelineSnapshot` (blocks + cards).

In parallel, the V-80 OP-centric migration (design at
`docs/perf/op-centric-feed-architecture.md`, rungs 1–7 + V-82 merged) replaced
the *shape* of `nmp.feed.home`. The feed is no longer "blocks + cards"; it is
`nmp_feed::RootFeedSnapshot<C, A>` — a list of thread-root cards, each carrying
the raw attribution list of follows who replied in its thread
(`crates/nmp-feed/src/root_indexed/card.rs`). The NIP-10 instantiation is
`RootFeedSnapshot<TimelineEventCard, Nip10ReplyAttribution>`
(`crates/nmp-nip01/src/op_feed/`). Rung 7 (PR #747) swapped the *producer*
(`register_op_feed_defaults`) and the chirp-tui *renderer* onto this new shape,
but emitted it **only on the generic `Value` path**.

### The collision is more precise than "typed NFTS silently wins"

A code-grounded audit of current master (`38bd0e96`) found that **no production
code path emits the typed NFTS sidecar for `nmp.feed.home`**. The only callers
of `encode_modular_timeline_snapshot` and `register_typed_snapshot_projection`
are tests and golden-wire fixtures:

- `crates/nmp-nip01/tests/golden_wire_fixtures.rs`
- `apps/chirp/nmp-app-chirp/tests/typed_feed_parity.rs`
- `crates/nmp-core/src/kernel/snapshot_registry_tests.rs`

ADR-0037 shipped the *codec, schema, decoders, and registration seam*, but the
live emission wiring into Chirp's producer never landed — and PR #747
explicitly declined to re-wire it (see the comment at
`apps/chirp/nmp-app-chirp/src/ffi/register.rs:155-159`: "ADR-0037 typed sidecar
for nmp.feed.home is intentionally NOT re-wired here … A follow-up PR will add
the `RootFeedSnapshot` typed-FB schema").

The actual runtime behavior on master is therefore:

| Host | Decode path | Result |
|---|---|---|
| chirp-tui | `snapshot.rs` prefers typed NFTS → none emitted → falls back to generic `Value` | Reads the new `RootFeedSnapshot` JSON; `timeline.rs` was rewritten by #747 to parse it. **Works.** |
| iOS | `TypedHomeFeedDecoder` prefers typed NFTS → none emitted → falls back to generic `Value` | Falls into the generic-`Value` render path (`ModularTimelineBridge.swift`, `HomeFeedView.swift`), which still expects the old `{blocks, cards}` shape. **Renders the new `{cards:[{card,attribution}],page,metrics}` shape incorrectly / emptily.** This is the actual user-visible breakage. |
| Android (gallery) | `TypedHomeFeedDecoder` prefers typed NFTS → none emitted → falls back to generic `Value` | Same fallback class as iOS. |

The graceful-fallback contract (ADR-0037 Commitment 4) did its job — nothing
crashes, no buffer mis-decodes — but the iOS/Android *generic-`Value`
renderers* were written for the pre-V-80 shape and were never updated, because
the typed pilot was supposed to carry the feed. The honest diagnosis is: **the
typed pilot for `nmp.feed.home` was never completed for the new shape, and the
generic-`Value` renderers on iOS/Android were left behind by the V-80 producer
swap.**

### The doctrinally-right resolution

The feed IS the typed pilot (ADR-0037 §Pilot). The correct path is not to
abandon typing for the feed, nor to keep the dead NFTS codec on life support,
but to **rebuild the typed path for the new `RootFeedSnapshot` shape** as a new
typed descriptor, restoring the hot-path optimization that motivated ADR-0037
on the highest-volume projection. This ADR specifies that descriptor, schema,
and rollout.

---

## Decision

Introduce a **new typed descriptor** for the OP-centric home feed, owned by
`nmp-nip01`, carried in the same ADR-0037 `TypedProjection` sidecar under the
existing key `nmp.feed.home`. The descriptor is

- `schema_id "nmp.nip01.opfeed"`
- `file_identifier "NOFS"`
- `schema_version 1`

and the payload is a new FlatBuffers root table `OpFeedSnapshot` that **reuses**
the existing typed `TimelineEventCard` table (via FlatBuffers `include`), the
existing `NFWM` feed-window sub-buffer, and the existing `NFCT` content-tree
sub-buffers. The only genuinely new tables are `RootCard` (card + attribution
vector) and `ReplyAttribution`.

### Commitment 1 — a NEW descriptor, not an NFTS version bump

`nmp.feed.home`'s typed payload changes *schema identity*, not *schema
version*. NFTS (`schema_id "nmp.nip01.timeline"`) encodes
`ModularTimelineSnapshot` = blocks + cards. The new payload encodes
`RootFeedSnapshot` = root cards + attribution. These are different schemas, not
two versions of one schema. `schema_id` is the schema's *identity*;
`schema_version` exists for shape-evolution *within* an identity. Bumping NFTS
to v2 to carry a structurally different root would make `schema_version` lie
about what `schema_id` names. A new `schema_id` is **required, not merely
preferred**.

This is reinforced — not justified — by the graceful-fallback benefit: under
ADR-0037 Commitment 4, a host that has not yet shipped the `NOFS` decoder sees
an **unrecognized descriptor** at `nmp.feed.home` and cleanly falls back to the
generic `Value` `RootFeedSnapshot` (correct behavior, just unoptimized). An
in-place NFTS bump would instead present an `NFTS`-tagged buffer of an
incompatible shape; even with `file_identifier` and `schema_version` guards,
the failure mode is "old host attempts a decode it must reject" rather than
"old host never recognized the descriptor in the first place." The new
descriptor makes the staged rollout (below) a clean per-host opt-in.

### Commitment 2 — reuse, do not duplicate

The `OpFeedSnapshot` buffer embeds and references, never re-declares:

- **`TimelineEventCard`** — the same typed table NFTS already defines in
  `crates/nmp-nip01/schema/timeline_snapshot.fbs`. `RootCard.card` is a
  `nmp.nip01.TimelineEventCard`. Because the OP-feed schema and the timeline
  schema both live in `nmp-nip01` and both compile into the crate's
  `src/wire/generated/` module, the OP-feed schema uses a FlatBuffers
  `include "timeline_snapshot.fbs";` and references `nmp.nip01.TimelineEventCard`
  by its fully-qualified name. The card's already-typed
  `content_tree_bytes` (embedded `NFCT`) and `content_render` tables come along
  for free — no re-encoding, identical bytes.
- **`NFWM` feed window** — `page` / `metrics` / cursor data travel as the
  existing `nmp-feed` typed `FeedWindow` buffer (`schema_id "nmp.feed.window"`,
  `file_identifier "NFWM"`), encoded via `nmp_feed::encode_feed_window`. The
  `OpFeedSnapshot` carries it as an opaque `[ubyte]` sub-buffer
  (`feed_window_bytes`), exactly as NFTS does today. `nmp-nip01` does not
  re-declare cursor/page/metrics tables (ADR-0033 ownership boundary).
- **`NFCT` content trees** — already embedded *inside* the reused
  `TimelineEventCard.content_tree_bytes`. No new handling.

The new tables are minimal:

- `ReplyAttribution` — the typed form of `nmp_nip01::Nip10ReplyAttribution`.
- `RootCard` — `{ card: TimelineEventCard, attribution: [ReplyAttribution] }`.
- `OpFeedSnapshot` — `{ schema_version, cards: [RootCard], feed_window_bytes,
  has_page, has_metrics }`.

### Commitment 3 — raw data only (ADR-0032, unchanged)

The typed payload changes encoding, never content. Every field follows
ADR-0032: pubkeys as 64-char lowercase hex, event ids as hex, timestamps as
Unix `u64` seconds, counts as raw integers, display name / picture URL verbatim
from kind:0 (absent when unseen, distinguished by a `has_*` companion bool).
The `ReplyAttribution` table mirrors `Nip10ReplyAttribution` field-for-field:
raw `author_pubkey`, the raw `AuthorDisplay` mirror (already a typed table in
the NFTS schema, reused), the optional flat `author_display_name` /
`author_picture_url` mirrors, the raw `reply_event_id`, the raw
`reply_created_at`. No `nmp_core::display::*` forwarder is invoked anywhere on
the encode path. Typing is a transport optimization, not a license to
pre-format. The `Vec<A>` attribution length IS the count (V-80 Q1 decision); the
schema carries no `attribution_total`.

### Commitment 4 — host preference and fallback (ADR-0037 Commitment 4, instantiated)

Per snapshot, for key `nmp.feed.home`, a host applies the ADR-0037 rule:

1. If `typed_projections` contains a `nmp.feed.home` entry whose descriptor is
   `schema_id "nmp.nip01.opfeed"` + `schema_version 1` + `file_identifier
   "NOFS"` **and** the host has a `NOFS` decoder, the host **MUST** prefer the
   typed payload and **MUST ignore** the generic `Value` subtree under
   `projections["nmp.feed.home"]`.
2. Otherwise (no typed entry, or an unrecognized descriptor — e.g. a host that
   only knows `NFTS`, or a host with no typed decoder at all), the host falls
   back to the generic `Value` `RootFeedSnapshot`.

During the rollout the emitter produces **both** the `NOFS` typed sidecar and
the generic `Value` `RootFeedSnapshot` for `nmp.feed.home` (PR #747's emission
is the fallback layer; this ADR adds the typed layer beside it). The generic
subtree is dropped only when the per-key staged-removal window closes (all
in-scope hosts ship the `NOFS` decoder — see §Migration).

### Commitment 5 — the registration site is `nmp-app-template`, not `nmp-nip01`

`nmp_nip01::register_op_feed` (`crates/nmp-nip01/src/op_feed/wiring.rs`)
deliberately takes **no `&NmpApp`** (it would invert the `nmp-nip01 → nmp-ffi`
dependency graph; documented at `wiring.rs:13-38`). It returns an
`Arc<OpFeedEngine>`; the composition root performs `NmpApp`-level registration.
Therefore the typed-sidecar registration lands in
`nmp_app_template::register_op_feed_defaults`
(`crates/nmp-app-template/src/op_feed_defaults.rs`), beside the generic
`FeedController` registration that already exists there.

`nmp-nip01` owns a free encoder helper (no `NmpApp`):

```rust
// crates/nmp-nip01/src/op_feed/typed_wire.rs (new)
pub fn encode_op_feed_snapshot(
    snapshot: &RootFeedSnapshot<TimelineEventCard, Nip10ReplyAttribution>,
) -> Vec<u8>;
pub fn decode_op_feed_snapshot(
    bytes: &[u8],
) -> Result<RootFeedSnapshot<TimelineEventCard, Nip10ReplyAttribution>, String>;
pub const OP_FEED_SCHEMA_ID: &str = "nmp.nip01.opfeed";
pub const OP_FEED_FILE_IDENTIFIER: &[u8; 4] = b"NOFS";
pub const OP_FEED_SCHEMA_VERSION: u32 = 1;
```

`nmp-app-template` wraps it in a `register_typed_snapshot_projection` closure
that snapshots the engine and encodes the result:

```rust
// in register_op_feed_defaults, after the generic FeedController registration
let engine_for_typed = Arc::clone(&engine);
app.register_typed_snapshot_projection("nmp.feed.home", move || {
    let snap = engine_for_typed.snapshot(&FeedRequest::default());
    Some(TypedProjectionData {
        key: "nmp.feed.home".into(),
        schema_id: nmp_nip01::op_feed::OP_FEED_SCHEMA_ID.into(),
        schema_version: nmp_nip01::op_feed::OP_FEED_SCHEMA_VERSION,
        file_identifier: "NOFS".into(),
        payload: nmp_nip01::op_feed::encode_op_feed_snapshot(&snap),
    })
});
```

**Known waste, deferred:** the typed closure calls `engine.snapshot(...)` and
the generic `FeedController` path calls it again on the same tick — two window
materializations per 4 Hz tick. This is acknowledged and deferred to a
follow-up (a shared per-tick snapshot cache, or moving the generic emission to
re-serialize the typed buffer the way `snapshot.rs:74-79` does today). It is not
load-bearing for correctness and the 4 Hz tick absorbs it during the rollout.

---

## The schema

New file `crates/nmp-nip01/schema/op_feed.fbs`, `namespace nmp.nip01`,
`include "timeline_snapshot.fbs"` so `TimelineEventCard` and `AuthorDisplay`
are referenced, never re-declared.

```fbs
// Typed FlatBuffers wire for the OP-centric home feed
// (`nmp_feed::RootFeedSnapshot<nmp_nip01::TimelineEventCard,
// nmp_nip01::Nip10ReplyAttribution>`).
//
// Relates to ADR-0038 (this schema), ADR-0037 (typed sidecar transport),
// ADR-0035 (RootFeedSnapshot owner), ADR-0032 (raw-data doctrine).
//
// Identity encoding: strings, not raw bytes — every event id / pubkey is a hex
// `String` in the source serde types, so `string` is byte-for-byte parity, the
// same deliberate per-schema choice documented in timeline_snapshot.fbs.
//
// Reuse, not duplication:
//   * `nmp.nip01.TimelineEventCard` + `nmp.nip01.AuthorDisplay` are referenced
//     from the included timeline_snapshot.fbs — RootCard.card IS that table,
//     including its embedded typed NFCT content tree and content_render.
//   * Feed page/cursor/metrics travel as the typed nmp-feed `FeedWindow` buffer
//     (`schema_id "nmp.feed.window"`, file_identifier "NFWM") embedded as
//     opaque bytes (`feed_window_bytes`) — nmp-nip01 never re-declares cursor
//     tables (ADR-0033 ownership).
//
// Regenerate the checked-in Rust bindings only with the workspace FlatBuffers
// pin (25.12.19), enforced by ci/check-flatbuffers-version-pins.sh. Because the
// schema uses an `include`, generate WITHOUT --gen-all so flatc emits a
// `use crate::wire::generated::timeline_snapshot_generated::*;` reference to the
// sibling module rather than re-inlining TimelineEventCard:
//   flatc --rust -o crates/nmp-nip01/src/wire/generated \
//         crates/nmp-nip01/schema/op_feed.fbs

include "timeline_snapshot.fbs";

namespace nmp.nip01;

// -----------------------------------------------------------------------------
// Reply attribution (nmp_nip01::Nip10ReplyAttribution)
// -----------------------------------------------------------------------------

// One follow's NIP-10 reply attributed back to a thread root. Raw data only
// (ADR-0032): raw hex pubkey, raw reply event id, raw signed created_at, and
// the kind:0 display mirrors carried verbatim (absent until a kind:0 lands,
// distinguished by the has_* companion bools). AuthorDisplay is the same typed
// mirror table the sibling TimelineEventCard reuses (from the include).
table ReplyAttribution {
  author_pubkey:string;          // raw hex pubkey of the replying follow
  author_display:AuthorDisplay;  // reused from timeline_snapshot.fbs
  // Flat mirrors of author_display.{name,picture_url}; has_* distinguishes
  // "absent (no kind:0 yet)" from "present empty string".
  has_author_display_name:bool;
  author_display_name:string;
  has_author_picture_url:bool;
  author_picture_url:string;
  reply_event_id:string;         // raw hex event id of the reply
  reply_created_at:uint64;       // raw signed created_at, Unix seconds
}

// -----------------------------------------------------------------------------
// Root card (nmp_feed::RootCard<TimelineEventCard, Nip10ReplyAttribution>)
// -----------------------------------------------------------------------------

// One feed row: a thread root's render card plus the raw attribution list of
// follows who replied in its thread. The attribution vector length IS the
// count (V-80 Q1) — there is no attribution_total field. The vector is bounded
// at encode time by the engine's per-root D5 cap (MAX_ATTRIBUTION_PER_ROOT).
table RootCard {
  card:TimelineEventCard;        // reused from timeline_snapshot.fbs
  attribution:[ReplyAttribution];
}

// -----------------------------------------------------------------------------
// Root: the full RootFeedSnapshot
// -----------------------------------------------------------------------------

table OpFeedSnapshot {
  schema_version:uint32 = 1;     // mirrors OP_FEED_SCHEMA_VERSION
  // Visible-window root cards, newest-first (the engine emits only the current
  // window — D5).
  cards:[RootCard];
  // Typed nmp-feed `FeedWindow` buffer (NFWM). Empty/absent when page+metrics
  // are both absent (RootFeedSnapshot.page = None && .metrics = None). The
  // has_* flags below distinguish "window present" from "window absent" so a
  // host need not parse the sub-buffer to learn there is no paging envelope.
  feed_window_bytes:[ubyte];
  has_page:bool;
  has_metrics:bool;
}

root_type OpFeedSnapshot;
file_identifier "NOFS";
```

### Encoding notes for the implementer

- `RootFeedSnapshot.page: Option<FeedPage>` and `.metrics:
  Option<FeedWindowMetrics>` map onto the embedded `FeedWindow` buffer via
  `nmp_feed::encode_feed_window` (whose `FeedWindowWire` already carries both as
  optionals) plus the two `has_*` flags. When both are `None`, emit empty
  `feed_window_bytes` and both flags `false` — the `snapshot.rs` /
  `typed_feed_parity.rs` precedent for the NFTS empty case. **T1 author's call:**
  the two `has_*` flags duplicate presence info already inside the embedded
  `NFWM` table (a host could read presence from the sub-buffer alone, the way
  NFTS does). They are kept here so a host can answer "no paging envelope"
  without decoding the sub-buffer; if that convenience isn't wanted, drop them
  for strict NFTS parity. Not load-bearing either way.
- `RootCard.card` is built by `TimelineEventCard`'s existing typed encoder
  (`crates/nmp-nip01/src/typed_wire/encode.rs`, the per-card builder around line
  268 — currently private; T1 promotes it to `pub(crate)` so the sibling
  `op_feed/typed_wire.rs` can call it) so the embedded `NFCT` / `content_render`
  bytes are produced by exactly the code NFTS uses today. This is the single
  biggest reuse win and the reason the schema lives in `nmp-nip01`.
- **Card / attribution types are aliases, not newtypes.** Chirp's
  `ChirpTimelineSnapshot` is `pub type ChirpTimelineSnapshot =
  RootFeedSnapshot<ChirpEventCard, ChirpReplyAttribution>` where
  `ChirpEventCard` / `ChirpReplyAttribution` are `pub use ... as` aliases of
  `nmp_nip01::TimelineEventCard` / `Nip10ReplyAttribution`
  (`apps/chirp/nmp-app-chirp/src/lib.rs:53,62`). So the engine snapshot the
  typed closure encodes IS `RootFeedSnapshot<TimelineEventCard,
  Nip10ReplyAttribution>` — the `encode_op_feed_snapshot` signature in
  Commitment 5 is exact, no wrapper conversion needed.

---

## Consequences

### What this enables

- The hot-path feed projection decodes by field offset with zero string-keyed
  map lookups on iOS/Android/TUI — the optimization ADR-0037 promised, now
  delivered on the new shape (the original pilot's whole motivation).
- iOS and Android stop rendering the new shape through a stale generic-`Value`
  decoder — T3/T4 add typed decoders that match the producer.
- A clean, opt-in, per-host rollout: an un-updated host sees an unrecognized
  descriptor and falls back, never mis-decodes.

### What this retires (NFTS-for-feed)

Because **no production code emits the NFTS sidecar for `nmp.feed.home`** (grep
of master: `encode_modular_timeline_snapshot` and
`register_typed_snapshot_projection` have zero non-test callers), there is no
live emission to stop. "Retiring NFTS-for-feed" collapses to:

1. **Decoders — rebind from NFTS to NOFS** (T2/T3/T4):
   - chirp-tui `apps/chirp/chirp-tui/src/snapshot.rs:74-93`
     (`typed_home_feed_from_projections`, `merge_home_feed_projection`).
   - iOS `ios/Chirp/Chirp/Bridge/TypedHomeFeedDecoder.swift` (descriptor
     constants + `nmp_nip01_ModularTimelineSnapshot` → `nmp_nip01_OpFeedSnapshot`).
   - Android `android/app/src/main/java/org/nmp/android/TypedHomeFeedDecoder.kt`
     and the gallery decoder under
     `apps/nmp-gallery/android/app/src/main/kotlin/nmp/transport/` (the live
     consumer the pin script guards).
2. **Feed-keyed parity test — reshape, do not keep**:
   - `apps/chirp/nmp-app-chirp/tests/typed_feed_parity.rs` asserts the NFTS
     descriptor at `nmp.feed.home`. Replace its body with the `NOFS` round-trip
     through `encode_snapshot_with_typed` / `decode_snapshot_with_typed`.
3. **NFTS codec itself — KEEP as available-but-unused infrastructure**:
   - `crates/nmp-nip01/schema/timeline_snapshot.fbs`,
     `crates/nmp-nip01/src/typed_wire.rs` + `typed_wire/encode.rs` +
     `decode.rs`, and `crates/nmp-nip01/tests/golden_wire_fixtures.rs` stay. The
     OP-feed schema *includes* `timeline_snapshot.fbs` and reuses its
     `TimelineEventCard` / `AuthorDisplay` tables, so the NFTS schema is a live
     dependency of NOFS even though the NFTS *root* (`ModularTimelineSnapshot`)
     is no longer emitted. The codec also remains the natural typed shape for a
     future thread-detail projection (a screen that genuinely wants blocks +
     cards). Deleting it would force re-deriving `TimelineEventCard`'s typed
     table elsewhere. Verified feed-only: `encode_modular_timeline_snapshot` has
     no non-test caller, so retiring the feed binding strands no other surface.

**Verification that NFTS is feed-only (done, not deferred):** `grep -rn
encode_modular_timeline_snapshot` across `apps/` and `crates/` returns only
`typed_wire.rs` (definition), `golden_wire_fixtures.rs` (test),
`typed_feed_parity.rs` (test). No thread-detail or other surface encodes it.
The retirement is safe.

### What this does NOT change

- The generic `Value` `RootFeedSnapshot` emission from PR #747 is **permanent**
  fallback (ADR-0037 §"What this does NOT change": `payload:Value` stays
  forever). #747 is **folded in, not superseded** (see §Interaction with rung 7).
- The ADR-0037 sidecar transport, `TypedProjection`/`TypedPayload` tables,
  `TypedProjectionFn` registry, and every `nmp-core` binding — untouched.
  `nmp-core` never learns the noun `OpFeed`; the descriptor is opaque strings.
- ADR-0032 raw-data doctrine.

### Risks

- **FlatBuffers `include` is new to this codebase.** No existing `.fbs` uses an
  `include` directive. Verified with the pinned `flatc 25.12.19` that
  `--rust` / `--swift` / `--kotlin` / `--ts` all generate compiling bindings
  for an include-based schema, and that `flatc --rust` (without `--gen-all`)
  emits a `use crate::<sibling>_generated::*;` reference to the included
  module rather than re-inlining the table — which is exactly the layout
  `nmp-nip01` wants (both `timeline_snapshot_generated.rs` and the new
  `op_feed_generated.rs` live in `src/wire/generated/`). Residual risk is
  per-platform path/module wiring of the generated reference; the fallback
  (duplicate the `TimelineEventCard` + `AuthorDisplay` table declarations in
  `op_feed.fbs`, no include) is mechanical and grows T1 by ~80 LOC. The
  `include` path is preferred; the fallback is documented so T1 can pivot
  without re-opening this ADR.
- **FlatBuffers runtime-pin asymmetry** (ADR-0037's standing risk). The new
  bindings observe the same per-platform pins: Rust 25.12.19 (single workspace
  `Cargo.toml flatbuffers = "25.12.19"` — shared, no new Rust pin), Swift
  25.12.19, web/TS 25.9.23, Android/Kotlin 25.2.10, all guarded by
  `ci/check-flatbuffers-version-pins.sh`. The script keys off `fun
  validateVersion` files under `apps/nmp-gallery/android/.../nmp/transport`; a
  new Kotlin `NOFS` decoder that bakes a `FLATBUFFERS_25_2_10()` guard is
  covered by the existing glob — no script change needed unless the decoder
  lands outside that tree (then add a `require_line` for it).
- **Dual emission widens the wire** for `nmp.feed.home` during the rollout
  (typed `NOFS` + generic `Value`). Bounded: ends when the staged-removal
  window closes. Plus the known duplicate `engine.snapshot(...)` per tick
  (Commitment 5), deferred.

### Migration & staged-removal window

Per ADR-0037, removal of the generic `Value` subtree for `nmp.feed.home` is
**per key, never global**. The emitter (`register_op_feed_defaults`) emits both
the `NOFS` sidecar and the generic `Value` subtree until **every in-scope host**
ships a `NOFS` decoder. In-scope hosts for this key are **iOS, chirp-tui, and
Android** (web is out of scope — see §Open question 3). When all three ship the
`NOFS` v1 decoder, the emitter may stop emitting the generic subtree for
`nmp.feed.home`. Until then, both are emitted. No flag day.

---

## Staged implementation ladder

Each stage is independently mergeable and leaves master green. Stages are named
`T1…T4` to disambiguate from the V-80 `rung 1…7` ladder.

### Stage T1 — Rust typed schema + encoder + emission wiring + retire NFTS-for-feed tests

| File | Change |
|---|---|
| `crates/nmp-nip01/schema/op_feed.fbs` | **NEW** — the schema above (`include "timeline_snapshot.fbs"`, `ReplyAttribution`, `RootCard`, `OpFeedSnapshot`, root `OpFeedSnapshot`, `file_identifier "NOFS"`). |
| `crates/nmp-nip01/src/wire/generated/op_feed_generated.rs` | **NEW** — checked-in `flatc --rust` output (no `--gen-all`; references the sibling `timeline_snapshot_generated` module). |
| `crates/nmp-nip01/src/op_feed/typed_wire.rs` | **NEW** — `encode_op_feed_snapshot` / `decode_op_feed_snapshot`; `OP_FEED_SCHEMA_ID` / `OP_FEED_FILE_IDENTIFIER` / `OP_FEED_SCHEMA_VERSION`. Encodes `RootCard.card` by delegating to the existing `TimelineEventCard` typed encoder; embeds `nmp_feed::encode_feed_window` bytes for the window. |
| `crates/nmp-nip01/src/op_feed/mod.rs` | Wire the `typed_wire` submodule; re-export the public encode/decode + descriptor consts. |
| `crates/nmp-nip01/src/lib.rs` | Export `op_feed::{encode_op_feed_snapshot, decode_op_feed_snapshot, OP_FEED_*}`. |
| `crates/nmp-app-template/src/op_feed_defaults.rs` | Add the `register_typed_snapshot_projection("nmp.feed.home", …)` closure beside the existing generic `FeedController` registration (Commitment 5). |
| `crates/nmp-nip01/tests/op_feed_golden_wire.rs` | **NEW** — golden-wire fixtures for `OpFeedSnapshot` (empty + a populated root-with-attribution + a repost-keyed root). Mirrors `golden_wire_fixtures.rs`: pin the binary bytes + assert `FILE_IDENTIFIER`/`SCHEMA_ID`/`SCHEMA_VERSION` + ADR-0037 parity (typed decode ≡ serde `RootFeedSnapshot`). |
| `apps/chirp/nmp-app-chirp/tests/typed_feed_parity.rs` | **RESHAPE** — replace the NFTS-at-`nmp.feed.home` assertions with the `NOFS` round-trip through `encode_snapshot_with_typed` / `decode_snapshot_with_typed`. |
| `ci/check-flatbuffers-version-pins.sh` | No change (Rust pin is shared; the new schema regenerates against the same `Cargo.toml` pin). Add a `require_line` only if a platform decoder lands outside the existing globbed trees (T3/T4). |

Master after T1: the kernel emits **both** the `NOFS` typed sidecar and the
generic `Value` `RootFeedSnapshot` for `nmp.feed.home`. No host reads `NOFS`
yet (all three still fall back to generic `Value`), so behavior is unchanged
from post-#747 master — except chirp-tui, which is already correct on the
generic path, stays correct.

### Stage T2 — chirp-tui typed decoder

| File | Change |
|---|---|
| `apps/chirp/chirp-tui/src/snapshot.rs` | Rebind `typed_home_feed_from_projections` from NFTS (`nmp_nip01::typed_wire::SCHEMA_ID`, `decode_modular_timeline_snapshot`) to NOFS (`nmp_nip01::op_feed::OP_FEED_SCHEMA_ID`, `decode_op_feed_snapshot`). `merge_home_feed_projection` re-serializes the decoded `RootFeedSnapshot` into `projections["nmp.feed.home"]` — same parity-by-construction round-trip the NFTS path used (the generic projection is itself `serde_json::to_value(RootFeedSnapshot)`, so the typed-derived value is byte-identical). The rung-7 render in `timeline.rs` consumes it unchanged. |
| `apps/chirp/chirp-tui/src/snapshot/tests.rs` | Update fixtures: typed `NOFS` sidecar present → preferred; absent → generic fallback. |

The rung-7 render changes (`timeline.rs`, `ui/post_list.rs`) are **already on
master** (PR #747). T2 only changes which descriptor the typed-read prefers; it
does not re-do the render. Master after T2: chirp-tui reads the typed `NOFS`
path when present, identical output to the generic path it already renders.

### Stage T3 — iOS decoder + bindings + render

| File | Change |
|---|---|
| `ios/Chirp/Chirp/Bridge/Generated/OpFeedSnapshot.generated.swift` (+ regenerated `TimelineSnapshot.generated.swift` if the include changes its module) | **NEW/regenerated** — `flatc --swift` output for `op_feed.fbs`. |
| `ios/Chirp/Chirp/Bridge/TypedHomeFeedDecoder.swift` | Repoint `schemaId` → `"nmp.nip01.opfeed"`, `fileIdentifier` → `"NOFS"`, root → `nmp_nip01_OpFeedSnapshot`; map `RootCard{card,attribution}` into the Swift feed model. Keep the graceful-`nil`-on-mismatch contract. |
| `ios/Chirp/Chirp/Bridge/ModularTimelineBridge.swift`, `HomeFeedView.swift` (+ `KernelBridge.swift`/`KernelModel.swift` as needed) | Update **both** read paths for `nmp.feed.home` to the OP-centric `{cards:[{card,attribution}],page,metrics}` shape: (a) the new typed `NOFS` decode, and (b) the generic-`Value` decode (currently stale — see §Context). Both must render the new shape correctly. The generic path stays load-bearing throughout the rollout: until all three hosts ship `NOFS`, the emitter sends both representations, and the fallback must render correctly if the typed decode ever returns `nil` (ADR-0037 Commitment 4 — the per-key staged-removal window is meaningful only while the fallback is correct). This dual update IS the iOS bug fix from §Context. |
| `ios/Chirp/ChirpTests/**` | Add an `OpFeedSnapshot` Decodable / typed-decode test + a `RootFeedSnapshot` JSON fixture. |
| `ci/check-flatbuffers-version-pins.sh` | No change — the Swift `25.12.19` pin (`ios/Chirp/project.yml`, `Package.resolved`) is already required; the new generated file uses the same runtime. |

Master after T3: iOS reads the typed `NOFS` feed and renders the OP-centric
shape correctly — the user-visible breakage from §Context is fixed.

### Stage T4 — Android decoder + bindings (follow-up; NOT a blocker)

| File | Change |
|---|---|
| `apps/nmp-gallery/android/app/src/main/kotlin/nmp/.../OpFeedSnapshot*.kt` (+ generated) | **NEW** — `flatc --kotlin` output (runtime `25.2.10`) + the typed model. |
| `android/app/src/main/java/org/nmp/android/TypedHomeFeedDecoder.kt` (+ gallery decoder) | Repoint to the `NOFS` descriptor + `OpFeedSnapshot`; map to the Android feed model. |
| `ci/check-flatbuffers-version-pins.sh` | Add a `require_line` only if the new decoder lands outside the existing `fun validateVersion` glob. |

Because of graceful fallback (Commitment 4), Android is **not a blocker**: until
its `NOFS` decoder ships it sees an unrecognized descriptor and falls back to
the generic `Value` `RootFeedSnapshot`. Android must update its generic-`Value`
renderer to the new shape independently (same stale-renderer class as iOS), or
accept degraded feed rendering until T4. **Confirmed: Android last, per
ADR-0037 rollout order.**

Once T2 + T3 + T4 have all shipped the `NOFS` decoder, the staged-removal window
closes and `register_op_feed_defaults` may stop emitting the generic `Value`
subtree for `nmp.feed.home` (a one-line follow-up; tracked, not in this ladder).

---

## Interaction with V-80 rung 7 / PR #747

**Folded in, not superseded.** PR #747 did two things this ADR depends on:

1. **Producer swap** — `register_op_feed_defaults` replaced
   `ModularTimelineProjection` at `nmp.feed.home`. This is the source of the
   `RootFeedSnapshot` that T1's typed encoder serializes. Stays.
2. **Generic `Value` emission** — the engine is registered as a
   `FeedController`, emitting `RootFeedSnapshot` JSON under
   `projections["nmp.feed.home"]`. Per ADR-0037 Commitment 4 this is the
   **permanent fallback layer**. Stays.

#747's chirp-tui render rewrite (`timeline.rs`, `ui/post_list.rs`) is also kept
verbatim — T2 changes only the *typed-read descriptor*, feeding the same render.

This ADR **adds** the typed `NOFS` layer beside #747's generic layer. It removes
nothing #747 added. The only thing it retires is the **NFTS-for-feed
decoder/test wiring from ADR-0037 (PR #739)** — work that predates #747 and was
already dead for the feed (no live emission). The #747 author's deferral comment
(`register.rs:155-159`, "a follow-up PR will add the `RootFeedSnapshot` typed-FB
schema") is precisely the follow-up this ADR specifies.

---

## Doctrine check

| Doctrine / ADR | Compliance |
|---|---|
| **ADR-0032 raw-data** | Typed payload carries raw hex pubkeys/ids, Unix-second timestamps, raw counts, verbatim kind:0 display mirrors with `has_*` absence flags. No `display::` forwarder on the encode path. ✅ |
| **ADR-0037 Commitment 1** (descriptor, not union) | `NOFS` is opaque bytes + a `(schema_id, schema_version, file_identifier)` descriptor carried in the existing `TypedProjection`. `nmp-core` gains no `OpFeed` noun, regenerates no binding. ✅ |
| **ADR-0037 Commitment 2** (app/protocol crate owns its schema) | Schema + bindings live in `nmp-nip01` (owner of `TimelineEventCard` + `Nip10ReplyAttribution`); cursor/window stay in `nmp-feed` (`NFWM`), content trees in `nmp-content` (`NFCT`). ✅ |
| **ADR-0037 Commitment 3** (raw data) | Same as ADR-0032 row. ✅ |
| **ADR-0037 Commitment 4** (preference/fallback) | Instantiated for `NOFS`; un-updated hosts see an unrecognized descriptor and fall back to generic `Value`. Dual emission during rollout; per-key staged removal. ✅ |
| **FB pin discipline** | Rust shares the workspace `25.12.19` pin (no new pin). Swift `25.12.19`, web `25.9.23`, Android `25.2.10` unchanged; new decoders observe them; `ci/check-flatbuffers-version-pins.sh` covers Android via its existing glob. New `include` directive verified to generate against the pinned `flatc`. ✅ |
| **D0** (`nmp-core` learns no NIP/app noun) | The descriptor strings are opaque to `nmp-core`; the encoder lives in `nmp-nip01`; the registration lives in `nmp-app-template`. `nmp-core` is untouched. ✅ |
| **D5** (bounded projections) | `OpFeedSnapshot.cards` is the engine's visible window (bounded); `RootCard.attribution` is bounded at encode time by `MAX_ATTRIBUTION_PER_ROOT`. ✅ |
| **D8** (non-blocking) | The typed projection closure runs inside the snapshot tick: one engine snapshot + one FlatBuffers encode, no I/O. Same contract as the generic projection. ✅ |
| **D11** (no new bespoke C-ABI symbol) | Uses the existing `register_typed_snapshot_projection` Rust seam (ADR-0037). No new `extern "C"`. ✅ |
| **Planning discipline** | This ADR is the single source for the typed-OP-feed decision; the V-80 architecture doc tracks the product-model work; `docs/BACKLOG.md` gets one V-entry pointing here. ✅ |

---

## Open questions needing user input

1. **Window-request parameter.** T1's typed closure snapshots
   `engine.snapshot(&FeedRequest::default())` — the default window. The generic
   `FeedController` path is viewport-aware (it advances cursors via
   `load_older_feed`). Should the typed sidecar mirror the *current* viewport
   request rather than the default window? For T1 the default is acceptable
   (matches the diagnostics-handle path), but a viewport-aware typed emit is a
   real follow-up if the typed path becomes the *sole* read (post staged
   removal). **Recommendation:** ship T1 with default window; track
   viewport-aware typed emit as a follow-up tied to the staged-removal close.
2. **NFTS codec disposition.** This ADR keeps the NFTS codec as
   available-but-unused infrastructure (NOFS includes its `TimelineEventCard`
   table; it is the natural thread-detail typed shape). **Confirm keep**, or
   request deletion (which would force re-deriving `TimelineEventCard`'s typed
   table inside `op_feed.fbs`).
3. **Web scope.** ADR-0037's rollout is iOS → TUI → web → Android. The task
   scopes this work to iOS → TUI → Android, omitting web. Confirmed
   code-grounded: `web/chirp/src/nmp/snapshot.ts` consumes the feed via the
   **generic `Value`** path (`ChirpTimelineSnapshot = {blocks, cards}`) and has
   **no typed decoder at all** — it never read NFTS. So web is unaffected by the
   typed collision; it only needs its generic-`Value` reader updated to the new
   `RootFeedSnapshot` shape, which is a V-80 product-model task, not a
   typed-sidecar task. **Recommendation:** web is OUT of scope for ADR-0038
   (no typed decoder to migrate); its generic-`Value` shape update tracks under
   V-80. Confirm.
