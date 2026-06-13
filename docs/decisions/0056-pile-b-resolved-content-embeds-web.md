# ADR-0056: Pile-B — kernel-emitted resolved content + embeds for web (ContentTreeWire over wasm→JS)

Status: PROPOSED

> Numbering note: `0052` is the highest decision committed to `docs/decisions/`
> on `master`. `0053` is contended by three concurrent open PRs (#1331
> incremental-projection-emission, #1339 host-declared subscriptions, #1341
> web-persistence), so this ADR deliberately skips ahead to the next clearly
> free number, `0056`, to avoid the in-flight `0053` collision. `0054`/`0055`
> are left as a gap reservation for the other two `0053` contenders to
> renumber into. Single-source-of-truth / no-duplicate-id discipline.

## Context

The `nmp-gallery-web` goal needs the web content/embed components to render
**real, kernel-resolved data** — parsed content trees, profile mentions, media,
and rich embed cards (article / note / profile / highlight) — with **zero
placeholders and zero TS-side parsing or resolution** (doctrine: the kernel owns
resolution/parsing; web components are thin renderers, mirroring native).

### Key finding: the content half is already transported, just not decoded

The task brief frames this as "ContentTreeWire is NOT emitted to JS today." The
code says something more precise, and it materially shrinks the work:

- The kernel **already produces** `ContentTreeWire` internally. Every timeline
  card is built by `TimelineEventCard::from_event`, which calls
  `tokenize_with_kind(...).to_wire()`
  (`crates/nmp-nip01/src/timeline_projection.rs:173-199`). The structural parse
  (`crates/nmp-content/src/wire/projection.rs:22` `ContentTree::to_wire`) is pure
  and D1/D6-clean.
- `ContentTreeWire` **already has a typed FlatBuffers wire** — file identifier
  `NFCT`, schema id `nmp.content.tree`
  (`crates/nmp-content/schema/content_tree.fbs`,
  `crates/nmp-content/src/wire/typed_fb.rs`).
- Those `NFCT` bytes **already cross the wasm→JS boundary today**. Each
  `TimelineEventCard` in the home-feed projection carries
  `content_tree_bytes:[ubyte]` (the `NFCT` sub-buffer), populated
  **unconditionally** at encode time
  (`crates/nmp-nip01/src/typed_wire/encode.rs:296-297,309`). That card rides
  inside the `OpFeedSnapshot` (`NOFS`, schema id `nmp.nip01.opfeed`, projection
  key `nmp.feed.home` — `crates/nmp-nip01/schema/op_feed.fbs:64-88`), which the
  web client **already decodes** (`web/chirp/src/nmp/feedProjection.ts:216-240`).
- The generated TypeScript decoders for `ContentTreeWire`/`WireNode` **already
  exist** (`web/chirp/src/nmp/generated/nmp/content/content-tree-wire.ts`,
  `wire-node.ts`, `wire-nostr-uri.ts`, …).

What is missing on web for content is therefore **one decode step plus the
renderers**: `feedProjection.ts:72` reads only the raw `content` string and
explicitly defers the tree (`feedProjection.ts:59-61` "NFCT content tree …
deferred to PR-F4"); `HomePanel.tsx:168` renders `content` as plain text. Native
already does the decode — Android `TypedHomeFeedDecoder.kt:230-254`, iOS
`TypedHomeFeedDecoder.swift:186-199` — proving the bytes are live and correct.

**Conclusion (determines effort): the content half needs no new schema, no new
kernel emission, and no drift-gate work — it is a TypeScript decode + thin
renderer task.** The genuinely new schema + kernel + 4-gate-drift work is
**embeds only.**

### The embed half is genuinely missing a typed projection

An embed is a `nostr:nevent…/naddr…/note…/npub…` reference inside content. The
content tree already surfaces it as a typed `EventRef` / `Mention` node carrying
a `WireNostrUri` (`crates/nmp-content/src/wire/mod.rs:78-82,200-220`). To render
the *card*, the renderer needs the **resolved referenced event/profile** plus the
kind-dispatched card fields.

The kernel already has every piece except the typed projection that joins them:

- **Resolution path works on web today.** An app claims an embed via the
  `nmp.kernel.claim_event` action (FFI `nmp_app_claim_event`,
  `crates/nmp-ffi/src/timeline.rs:199-220`; wasm routing
  `crates/nmp-wasm/src/dispatch_routing.rs:83,218` →
  `reducer.claim_event(uri, consumer_id, …)`). The kernel resolves it (from
  store, or fetches via `OneshotApi`) and surfaces the **raw** event in the
  `claimed_events` projection — typed as `KCEV`
  (`crates/nmp-core/schema/claimed_events.fbs`, `decode_claimed_events` is
  public). Profiles resolve symmetrically into `resolved_profiles` (`KRPR`,
  `crates/nmp-core/schema/resolved_profiles.fbs`).
- **Kind dispatch + card shaping already exists, untyped.**
  `resolve_embed_projection(event, ctx)`
  (`crates/nmp-content/src/embed_projection/mod.rs:40-151`) is the single
  `match event.kind` point (ADR-0034) that turns a `KernelEvent` into a typed
  `EmbedKindProjection` — `ShortNote` / `Article` (title/summary/image/d-tag) /
  `Highlight` (quote + source `e`/`a`/`r`/`context`) / `Profile` / `Unknown` —
  each carrying an embedded `ContentTreeWire` for the body
  (`embed_projection/variants.rs`). But this only crosses the wire as **serde
  JSON** via `EmbeddedEventEnvelope`
  (`crates/nmp-content/src/embed_projection/envelope.rs`); there is **no typed
  FlatBuffers projection** and **no web consumer**.

#### Why not just reuse `claimed_events` (`KCEV`) on web?

This is the central objection, and it is what justifies a new projection. `KCEV`
carries the **raw** event — `content:string`, `tags:[TagRow]`, and `kind` as an
**opaque `uint`** with, by its own design note, "no kind-dependent branching in
the buffer … so it is kernel-owned" (`claimed_events.fbs:31-35`). Building embed
cards directly from `KCEV` on the web would require TS to (a) switch on `kind`,
(b) extract `title`/`summary`/`image`/`d`/`e`/`a`/`r`/`context` from raw tags,
and (c) tokenize the embedded body into a content tree. That is **exactly** the
kernel-owned parsing/resolution the doctrine forbids in TS, and it would
duplicate `resolve_embed_projection` in JavaScript. A typed `resolved_embeds`
projection exists precisely to run that dispatch **once, in Rust**.

#### Lineage: this continues the GH #920 direction

The timeline schema already contains the embed-card *shape* — `id`,
`author_display`, `kind`, `created_at`, `content_tree_bytes` — in the
`ContentEventRenderEntry`/`ContentRenderData` tables
(`timeline_snapshot.fbs:161-182`). GH #920 deliberately **emptied** the per-card
`content_render` (the encoder now writes `ContentRenderData::default()`,
`encode.rs:298`) to move embed/profile resolution **out of per-card
denormalization and into snapshot-level join maps** (`claimed_events`,
`resolved_profiles`) keyed by id/pubkey. A new snapshot-level `resolved_embeds`
map keyed by `primary_id` is the **continuation** of that established direction —
not a reversal — which is why `content_render` stays empty rather than being
repopulated.

### Cross-platform / drift constraints

Any new `.fbs` must regenerate bindings and pass **four** codegen-drift CI gates
(Rust, Swift, Kotlin, TypeScript), and the `flatc` pins differ per target:
Rust/Swift `25.12.19`, TypeScript `25.9.23`, Kotlin `25.2.10`. Schema features
must be chosen for the **intersection** of those versions.

## Decision

Adopt a **single new kernel-emitted typed projection, `resolved_embeds`** (file
identifier `NEMB`, schema id `nmp.content.embeds`, projection key
`resolved_embeds`), mirroring the `KRPR`/`KCEV` shared-key sidecar contract
(ADR-0037). One projection — a map of `primary_id → EmbedCard` — **not** one
projection per embed kind: the per-kind variant is an enum discriminator inside
the row, matching how `WireNode` discriminates node kinds and how `KRPR`/`KCEV`
ship one flattened map.

The **content half ships with no schema and no kernel change** — it is a pure
web task (decode the already-present `NFCT` bytes + thin renderers).

### Schema ownership and emission tier (resolve the ambiguity)

`resolved_embeds` is **not** a kernel Tier-2 built-in. Its row branches per
`kind` (article vs note vs highlight vs profile), and per `claimed_events.fbs`'s
own rationale, kind-dependent shaping is **not** D0 kernel-owned — kind dispatch
belongs to `nmp-content` (ADR-0034). Therefore:

- **Schema lives in `nmp-content`** (`crates/nmp-content/schema/resolved_embeds.fbs`),
  next to `content_tree.fbs`, since it is the embed model.
- **Emission is Tier-1** (host/protocol-crate-registered via
  `SnapshotRegistry::register_typed`, ADR-0037), not Tier-2. The encoder is a
  pure transform `(claimed_events, resolved_profiles) → Vec<EmbedCard>`: for each
  resolved event, run `nmp_content::resolve_embed_projection`, enrich the
  `author_display` from `resolved_profiles`, and embed the body as an opaque
  `NFCT` `content_tree_bytes` sub-buffer.

> **Open mechanical tension (flagged, resolved in Stage 1):** Tier-2 encoders
> read live `&self` kernel state; Tier-1 encoders read state parked behind an
> `Arc<Mutex>` slot and have no `&self`. `resolved_embeds` is a transform of
> *two kernel-owned* projections (`claimed_events` + `resolved_profiles`) emitted
> from a *non-kernel* owner. The Stage-1 wiring decision is whether to (a) park
> the two source maps into the Tier-1 slot each tick, or (b) make this the first
> "Tier-1.5" projection — a kernel-driven encoder that is allowed to read
> kernel-owned resolved state but lives in `nmp-content`/`nmp-nip01` for D0
> reasons. Recommended: (a) — reuse the existing slot mechanism, no new tier.

### Projection contract (FlatBuffers schema sketch)

Schema-design choices below are **required by the pin spread** (Kotlin
`25.2.10` < TS `25.9.23` < Rust/Swift `25.12.19`), not stylistic: the
maximally-compatible subset across that skew is **enum-discriminated optional
fields (no unions)** and **opaque `[ubyte]` sub-buffers**. Embedding the content
tree as opaque `NFCT` bytes (rather than a `flatc include` of `content_tree.fbs`)
also sidesteps include-ordering differences across the version-skewed
toolchains — the identical technique `timeline_snapshot.fbs:175-177` already
uses.

```fbs
// crates/nmp-content/schema/resolved_embeds.fbs  (SKETCH — design gate)
namespace nmp.content;

// One resolved embed card. Discriminator + optional per-kind fields — NOT a
// union (cross-runtime stability under the flatc pin spread), mirroring the
// WireNode optional-fields approach in content_tree.fbs.
enum EmbedKind:ubyte { ShortNote = 0, Article = 1, Highlight = 2, Profile = 3, Unknown = 4 }

// Author display, verbatim from kind:0 (absent until a profile resolves).
// has_* companions distinguish "absent" from "present empty string" — the same
// shape AuthorDisplay uses in timeline_snapshot.fbs.
table EmbedAuthorDisplay {
  pubkey:string;                 // raw hex (always present)
  has_name:bool; name:string;
  has_picture_url:bool; picture_url:string;
}

table EmbedTagRow { values:[string]; }   // for Unknown.tags (Vec<Vec<String>>)

table EmbedCard {
  // --- common ---
  primary_id:string;             // map key echoed in body: hex event id, or
                                 // "kind:pubkey:d" coordinate (naddr)
  kind:EmbedKind;
  raw_kind:uint32;              // opaque protocol kind (drives Unknown dispatch)
  id:string;                     // resolved event id (empty for Profile)
  author:EmbedAuthorDisplay;
  created_at:uint64;
  // Typed nmp-content ContentTreeWire (NFCT) for the embedded body. Empty for
  // Profile/Highlight which carry no rich body. Opaque to THIS schema — decoded
  // with the nmp-content NFCT decoder (same pattern as timeline cards).
  content_tree_bytes:[ubyte];

  // --- Article (kind:30023) ---
  has_title:bool; title:string;
  has_summary:bool; summary:string;
  has_hero_image_url:bool; hero_image_url:string;
  d_tag:string;                  // "" when absent

  // --- ShortNote (kind:1) ---
  media_urls:[string];           // top-level media for preview thumbnails

  // --- Highlight (kind:9802) ---
  highlighted_text:string;       // "" unless Highlight
  has_source_event_id:bool; source_event_id:string;   // inner nevent/note ref
  has_source_event_addr:bool; source_event_addr:string;// inner naddr ref
  has_source_url:bool; source_url:string;              // web URL
  has_context:bool; context:string;

  // --- Profile (kind:0) ---  (display already in `author`; extras here)
  has_about:bool; about:string;
  has_nip05:bool; nip05:string;
  has_lud16:bool; lud16:string;
  has_banner_url:bool; banner_url:string;

  // --- Unknown (extensibility escape hatch) ---
  has_content:bool; content:string;   // raw event content
  tags:[EmbedTagRow];                 // raw tags for custom native/web renderers
  has_alt_text:bool; alt_text:string; // NIP-31 alt

  // --- resolution state (renderer affordances) ---
  collapsed:bool;                // depth limit / cycle / unsupported
  has_collapse_reason:bool; collapse_reason:string;  // "depth_limit"|"cycle"|"unsupported"
}

// Flattened primary_id -> EmbedCard map (FlatBuffers has no map type). Entries
// sorted by key (BTreeMap), matching KRPR/KCEV.
table ResolvedEmbedEntry { key:string; value:EmbedCard; }
table ResolvedEmbedsSnapshot { entries:[ResolvedEmbedEntry]; }

file_identifier "NEMB";          // free (verified against all in-tree ids)
root_type ResolvedEmbedsSnapshot;
```

This is a near-field-for-field mirror of `EmbedKindProjection` +
`EmbeddedEventEnvelope` (`embed_projection/variants.rs`, `envelope.rs`), which is
the single source of truth the encoder maps from.

### Web decode + component contract

TS decode mirrors `KRDG`/`KRPR` exactly
(`web/chirp/src/nmp/relayDiagnosticsProjection.ts`): locate the projection by
key in `snapshot.typedProjections()`, validate the `NEMB` file identifier on the
payload, `ByteBuffer` → `ResolvedEmbedsSnapshot.getRootAs…`, build a
`Map<string /*primary_id*/, EmbedCard>`, return `undefined` on any malformed
input (never throw). The embedded `content_tree_bytes` decode reuses the existing
`ContentTreeWire`/`WireNode` generated TS.

The web components are authored as canonical source under
`web/registry/src/vendor/web/{content-*,embed-*}/` (this ADR **specifies**, does
not create them; it does **not** touch existing `user-*` components or
`web/nmp-gallery`). Each is a **thin renderer** that walks the decoded wire type
— no parsing, no resolution, mirroring the native widgets.

**Content components (`content-*`, depend only on the already-transported
`NFCT` tree):**

| Component | Renders | Input |
| --- | --- | --- |
| `NostrContentView` | Full content tree: walks the `WireNode` arena from `roots`, dispatching per `WireNodeKind` (Text/Paragraph/Heading/List/Link/Image/CodeBlock/Emphasis/Strong/…). | `ContentTreeWire` |
| `NostrMinimalContentView` | Compact single-line/preview variant of the tree (used inside embed cards / quote bodies to bound nesting). | `ContentTreeWire` |
| `NostrMentionChip` | A `Mention` node (`WireNostrUri` kind=Profile): resolves display name via the **existing** `resolved_profiles` (`KRPR`) map keyed by `primary_id` (pubkey); falls back to truncated npub when absent. | `WireNostrUri` + resolved-profiles map |
| `NostrMediaGrid` | A `Media` node's `urls` + `media_kind`. | `WireNode::Media` |
| `NostrQuoteCard` | An `EventRef` node rendered as a quoted card; resolves the referenced event via the `resolved_embeds` map (falls back to a claim-pending affordance). | `WireNostrUri` (Event) + resolved-embeds map |

**Embed components (`embed-*`, depend on the new `resolved_embeds` projection):**

| Component | Renders | `EmbedKind` |
| --- | --- | --- |
| `NoteEmbed` | Author + timestamp + body via `NostrMinimalContentView(content_tree)` + media. | `ShortNote` |
| `ArticleEmbed` | Hero image + title + summary + author. | `Article` |
| `ProfileEmbed` | Avatar + name + nip05 + about (display from `author`/`resolved_profiles`). | `Profile` |
| `HighlightEmbed` | Quoted `highlighted_text` + source affordance (`source_event_id`/`addr`/`url`). | `Highlight` |

Nested embeds (a note that quotes a note) resolve **iteratively**, not in one
shot: the inner `EventRef`/`source_event_id` stays an unresolved node/affordance
until separately claimed, depth-bounded by `RenderContext`
(`embed_projection/envelope.rs:30-37`). This matches the Phase-1 claim model and
prevents unbounded fan-out.

## Sequenced, de-risked implementation plan

Stages are ordered so the highest-value, lowest-risk work (content rendering,
zero schema) ships first and the peer-dependent work is last.

**Stage 0 — Content rendering on web (NO schema, NO Rust, NO drift).**
Decode `card.contentTreeBytes()` into the existing generated `ContentTreeWire` TS
in `web/chirp/src/nmp/feedProjection.ts` (lift the `feedProjection.ts:59-61`
deferral), surface it on the feed item. Author `content-*` components
(`NostrContentView`, `NostrMinimalContentView`, `NostrMediaGrid`,
`NostrMentionChip`) under `web/registry/src/vendor/web/content-*/`. @mention
display names join against the existing `resolved_profiles` map.
*Risk: low. Dependencies: none (bytes are already live; Android/iOS prove
correctness). Ships real content rendering immediately.*
*Caveat: full @mention display depends on `resolved_profiles` being populated,
which on web depends on the peer-owned profile-claim fix; chips render npub
fallback until then (raw-data doctrine).*

**Stage 1 — `resolved_embeds.fbs` + Rust emission + 4-gate drift.**
Add `crates/nmp-content/schema/resolved_embeds.fbs` (the sketch above).
Regenerate Rust/Swift/Kotlin/TS bindings with the **per-target pinned** `flatc`
(Rust/Swift `25.12.19`, TS `25.9.23`, Kotlin `25.2.10`) per the recipe in each
schema header; check them in. Write the Tier-1 encoder (transform
`claimed_events` + `resolved_profiles` → `Vec<EmbedCard>` via
`resolve_embed_projection` + profile enrichment + `NFCT` embed) and register it
(resolve the Tier-1 slot tension noted above — recommended: park the two source
maps into the registered slot each tick). Rust round-trip + parity tests
(encoder output ≡ serde `EmbedKindProjection`).
*Risk: medium (4 drift gates, pin spread, Tier-1 wiring). Dependencies: none on
the peer. Additive — native's existing `EmbeddedEventEnvelope` JSON path is
untouched.*

**Stage 2 — TS decode of `resolved_embeds`.**
Author `web/chirp` (or shared) decoder mirroring `KRPR`; expose
`Map<primary_id, EmbedCard>` on the runtime snapshot. Decode the embedded `NFCT`
body via existing generated TS.
*Risk: low. Dependencies: Stage 1.*

**Stage 3 — Embed components (`embed-*`).**
Author `NoteEmbed`, `ArticleEmbed`, `ProfileEmbed`, `HighlightEmbed`,
`NostrQuoteCard` under `web/registry/src/vendor/web/embed-*/`. `NoteEmbed`
reuses `NostrMinimalContentView` for the body.
*Risk: low. Dependencies: Stages 0 + 2.*

**Stage 4 — chirp-web / gallery migration (claim wiring).**
Walk the content tree's `EventRef`/`Mention` nodes, drive
`nmp.kernel.claim_event` (and `claim_profile`) from web on mount / release on
unmount, render embed cards from `resolved_embeds`. Replace placeholder/plaintext
rendering with the real components.
*Risk: medium. Dependencies: the wasm claim surface
(`crates/nmp-wasm/src/dispatch_routing.rs`, **peer-owned** — exists today for
events) and, for `@mentions`/`ProfileEmbed` display, the peer-owned
profile-claim fix. Event embeds render fully without the peer; profile-dependent
display degrades to raw npub/pubkey until the peer fix lands.*

## Adversarial risk section

- **Schema cross-platform impact.** A new `.fbs` adds generated Swift/Kotlin
  bindings even though native renders embeds via the serde JSON envelope today.
  Mitigation: the projection is purely additive (ADR-0037 shared-key fallback);
  native keeps its JSON path until it chooses to adopt the typed one. No existing
  schema changes, so no wire break.
- **Drift-gate breakage (the highest-probability failure).** Four gates, three
  `flatc` pins. A union or a `flatc include` could regenerate differently across
  the version skew. Mitigation: no unions (enum + optional fields), opaque
  `[ubyte]` `NFCT` sub-buffer (no include), exact per-target regen recipe in the
  schema header — the same constraints `content_tree.fbs`/`timeline_snapshot.fbs`
  already satisfy across these gates.
- **Claim dependency.** Embed resolution requires EVENT claims driven from web.
  Confirmed viable today (`dispatch_routing.rs:83,218`; `claimed_events` fetches
  via `OneshotApi` on claim). But `EmbedClaimRegistry` is a Phase-1
  **dedupe-only** primitive — it does not itself open upstream subscriptions
  (`embed_registry/mod.rs:24-34`); resolution happens only when the kernel
  independently ingests the event, which the `claim_event` fetch path triggers.
  Risk: an embed whose event is unfetchable stays an unresolved affordance
  (acceptable; typed, not a crash).
- **Profile-claim dependency (peer-owned).** `@mentions` and `ProfileEmbed`
  display names need `resolved_profiles` populated by profile claims, currently
  broken on web (peer territory). Mitigation: every card still renders with raw
  pubkey/npub fallback (raw-data doctrine — display absent until kind:0); Stage 0
  and event-embed stages do not block on it.
- **Real-data feasibility per kind.** Note ✓ (event claim + body tree).
  Article ✓ (kind:30023 via naddr → claim → title/summary/image from tags).
  Highlight ✓ (kind:9802 content + source tags; the *nested source* event is a
  separate iterative claim, deferrable). Profile — display needs kind:0 (gated on
  the peer profile-claim fix; card structure renders regardless).
- **File-size gate.** The encoder + bindings must respect the LOC ceiling; follow
  the existing pattern of extracting cluster encoders into sub-modules
  (`typed_projections/builtins_*.rs`). The schema sketch lives in this ADR (not a
  live `.fbs`) at the design gate precisely so no half-wired schema trips the
  drift gates before Stage 1.

## Consequences

- Web renders real, kernel-resolved content immediately (Stage 0) with zero new
  schema and zero TS-side parsing.
- One new typed projection (`NEMB`) gives web (and any future host) rich embed
  cards with kind-dispatch owned in Rust, continuing the GH #920 snapshot-join
  direction.
- The doctrine holds: kernel owns parsing/resolution; web components are thin
  renderers mirroring native; no duplicated kernel logic in JS.
