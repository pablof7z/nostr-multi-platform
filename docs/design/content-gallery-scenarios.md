# NMP Content Gallery — Scenario Matrix

> **Purpose.** This document is the canonical specification for the NMP
> content-component showcase ("NmpGallery") — the NMP analogue of NDKSwift's
> content-component demo app. It enumerates **every** Nostr content / embed
> combination that the `nmp-content` tokenizer + `nmp-nip23` article path
> renders, so a human can visually verify each rendered shape.
>
> Every event referenced here is **real**: signed in-process by
> `nmp_signers::LocalKeySigner` with deterministic test keys (via
> `LocalKeySigner::from_secret_hex`), with valid Schnorr signatures and
> correct event IDs. **No event is ever published to a relay.** The
> "fixture store" is a relay-free, pre-resolved bundle.
>
> This doc is consumed by:
> - **STAGE 2** — `crates/nmp-content-fixtures` (offline signed-event +
>   pre-tokenized DTO bundle generator).
> - **STAGE 3** — `ios/NmpGallery/` (SwiftUI showcase rendering the bundle).

---

## 1. Architecture & Doctrine Constraints

### 1.1 The ContentTree FFI projection gap (forcing function)

`nmp_content::Segment` and `nmp_content::ContentTree` **deliberately do not
derive `serde`** (see the doc-comment in `crates/nmp-content/src/segment.rs`).
The cross-platform contract is: *"FFI consumers project to platform-native
types at the bridge; cross-process serialization is out of scope for Layer
A."* The C ABI in `crates/nmp-core/src/ffi/mod.rs` exposes events / timeline
/ identity only — **there is no live `ContentTree` projection across FFI**.
The relevant work item, **T93 ("ContentTree FFI ADR")**, is an architecture
decision record that is still in flight and **not landed**.

We are forbidden from editing `crates/nmp-content/**` ("consume as-is").
Therefore this gallery uses the **pre-tokenized DTO** strategy, which is
*consumption*, not editing:

1. **STAGE 2 (Rust, our crate)** imports `nmp_content::{tokenize_with_kind,
   Segment, ContentTree, MarkdownNode, MarkdownInline, RenderMode}` and
   `nmp_nip23` **as-is**. For each scenario it:
   - builds + signs the event(s) with `LocalKeySigner`,
   - runs the **real** `tokenize_with_kind` (and the real `nmp-nip23`
     decode/view path for kind:30023 / NIP-51),
   - projects the resulting `Segment` / `MarkdownNode` tree to a
     **serde-derivable `SegmentDto` / `ContentTreeDto`** that mirrors the
     variants 1:1,
   - pre-resolves every embed target (mention kind:0, quoted note,
     naddr→30023, naddr→NIP-51 list) into an `embeds` map keyed by the
     NIP-21 URI, applying the real `RenderContext` depth (`max_depth = 4`,
     PD-015) + `visited`-set cycle guard,
   - emits **one JSON bundle**.
2. **STAGE 3 (Swift)** loads the bundle and walks `SegmentDto` to render
   per-variant SwiftUI. Embed resolution is a lookup in the bundle's
   `embeds` map — this **is** the relay-free in-process fixture store that
   `EmbedClaimRegistry` would serve at runtime.

The Rust side performs all real tokenizing / NIP-23 parsing; only the
**cross-language transport** changes. The proposed `ContentTreeDto` schema
(§5) is offered as the candidate shape for T93 to canonicalize.

**Projection-gap note (forcing function).** `nmp_content::RenderContext`
(the PD-015 depth budget + `visited`-set cycle guard) is *also* non-serde
with no FFI projection. A flat URI→entry map cannot pre-bake depth/cycle
collapse: a cyclic URI legitimately appears at multiple depths but the map
has one slot per URI. Therefore the bundle carries only **resolution
facts** (the resolved target + its rendered body), plus the two
*context-independent* collapse facts that are properties of the URI/kind
rather than of any render path: `"dangling"` (URI absent from the
relay-free store) and `"unsupported"` (kind has no NMP view). The PD-015
depth + cycle guard is enforced **at render time in the Swift walker**
(STAGE 3), mirroring `RenderContext::should_collapse` semantics
(`depth >= max_depth (4)` OR `visited.contains(into)`); the bundle proves
the cycle is *renderable* by guaranteeing each cycle body really contains
the back-reference that trips the renderer's `visited` set.

### 1.2 Canonical doctrines exercised

| Doctrine | Where it bites in this matrix |
| --- | --- |
| **D0** | Signer material lives in `nmp-signers`, not the kernel — fixtures sign via `LocalKeySigner`. |
| **D1** | Best-effort rendering: a missing kind:0 → deterministic identicon placeholder; never blank / crash / spinner-gate. |
| **D4** | Single tokenize entry point (`tokenize_with_kind`) with a `mode`/`kind` flag — never three separate functions. |
| **D8** | NIP-23 article body rendering is doctrine-bounded (no GFM tables / strikethrough — PD-012). |
| **PD-012** | CommonMark-core only in article bodies: H1–H6, bold/italic, ordered/unordered/nested lists, fenced + inline code, blockquote, links, images, `nostr:` mentions. **No tables, no strikethrough, no task lists, no footnotes.** |
| **PD-015** | Embed recursion default `max_depth = 4`; beyond it the embed card **collapses** (`RenderContext::should_collapse`). |
| Cycle guard | `RenderContext.visited` set: `A → B → A` collapses the re-entrant card. |

### 1.3 LOC / quality budget

- Files ≤ 300 LOC soft, ≤ 500 LOC hard.
- No `TODO` / `FIXME` / `unimplemented!` in non-test code.
- Swift cell renderers split per category (§6) to stay under budget:
  `text`, `mentions`, `quotes`, `articles`, `lists`, `fallback`.

### 1.4 Deterministic test identities

All identities are derived from fixed 32-byte secret-key hex seeds so event
IDs are stable across runs (screenshots stay diffable).

| Alias | Secret hex seed (32 bytes) | Role |
| --- | --- | --- |
| `ALICE` | `0000…0001` (`00`×31 + `01`) | Primary author |
| `BOB` | `0000…0002` | Quoted / mentioned author |
| `CAROL` | `0000…0003` | Article author, list owner |
| `DAVE` | `0000…0004` | Profile-without-metadata author |
| `EVE` | `0000…0005` | Cycle partner (A↔B↔A) |

`ALICE`'s `nprofile` / `npub`, etc., are computed at fixture-build time from
the derived `PublicKey`; this doc refers to them symbolically.

---

## 2. Scenario Catalog — Plain `kind:1` Text

Each scenario: **id**, **exercises** (nmp-content path + doctrine), **event
to synthesize**, **expected rendered shape**.

### S-T01 · Plain text
- **Exercises**: tokenizer fast-path → single `Segment::Text`. `RenderMode::Auto` sniffs kind:1 → Plain.
- **Event**: kind:1, content `"Just shipped the relay reconnect fix. Feels good."`, no tags.
- **Expected**: one paragraph of body text, no chips, no media.

### S-T02 · Hashtags inline
- **Exercises**: `Segment::Hashtag` extraction; leading `#` stripped, lowercased; surrounding text preserved as `Segment::Text`.
- **Event**: kind:1, content `"Debugging #Nostr relays again #NIP01 #zaps"`, `t` tags `["nostr"],["nip01"],["zaps"]`.
- **Expected**: text interleaved with three tappable hashtag chips reading `#nostr`, `#nip01`, `#zaps`.

### S-T03 · Bare URL (non-media)
- **Exercises**: `Segment::Url` (URL not classified as media by extension).
- **Event**: kind:1, content `"Spec lives at https://github.com/nostr-protocol/nips read it"`.
- **Expected**: inline text with a tappable link chip for the URL; no preview card (link-preview is app-owned, out of substrate scope).

### S-T04 · Image URL → media block
- **Exercises**: grouper post-pass → `Segment::Media { kind: Image }`. URL-extension inference only (no MIME sniff).
- **Event**: kind:1, content `"Sunset from the office https://nmp.test/img/sunset.jpg"`.
- **Expected**: text line, then a single image media tile (placeholder image — relay-free, URL not fetched in fixtures; cell shows the classified `Image` kind + URL).

### S-T05 · Video URL → media block
- **Exercises**: `Segment::Media { kind: Video }` from `.mp4`.
- **Event**: kind:1, content `"Recording of the outage postmortem https://nmp.test/v/postmortem.mp4"`.
- **Expected**: text line, then a video media tile (poster placeholder + play affordance; not actually played in gallery).

### S-T06 · Media gallery (multiple adjacent images)
- **Exercises**: grouper grouping consecutive media URLs into ONE `Segment::Media { urls: [..], kind: Image }`.
- **Event**: kind:1, content `"Trip photos https://nmp.test/img/a.jpg https://nmp.test/img/b.png https://nmp.test/img/c.webp"`.
- **Expected**: text line, then a 3-up image grid rendered from a single Media segment (verifies grouping, not 3 separate tiles).

### S-T07 · Mixed media kinds adjacent
- **Exercises**: grouper boundary behavior when image + video + audio extensions are adjacent (separate Media segments per kind run).
- **Event**: kind:1, content `"https://nmp.test/img/x.png https://nmp.test/v/y.webm https://nmp.test/a/z.mp3"`.
- **Expected**: image tile, then video tile, then audio row — three Media segments, each with its own `MediaKind`.

### S-T08 · NIP-30 custom emoji (resolved)
- **Exercises**: `Segment::Emoji { shortcode, url: Some }` resolved from `emoji` tags.
- **Event**: kind:1, content `"gm :nmp: ship it :rocket:"`, tags `["emoji","nmp","https://nmp.test/e/nmp.png"]`, `["emoji","rocket","https://nmp.test/e/rocket.png"]`.
- **Expected**: text with two inline emoji images (placeholder glyph tile labeled with the shortcode + resolved URL).

### S-T09 · NIP-30 emoji (unresolved)
- **Exercises**: `Segment::Emoji { shortcode, url: None }` — shortcode present, no matching `emoji` tag → graceful literal fallback (D1).
- **Event**: kind:1, content `"missing :ghost: shortcode"`, no `emoji` tags.
- **Expected**: the literal `:ghost:` shown as text/badge (never blank, never crash).

### S-T10 · Lightning / Cashu invoice tokens (reserved)
- **Exercises**: `Segment::Invoice(Bolt11 | Bolt12 | Cashu)` — substrate detects + emits; wallet UX is app-owned (M12 deferred), so the gallery renders a non-actionable "invoice detected" badge.
- **Event**: kind:1, content `"zap me lnbc10u1p3x…  or cashu cashuAeyJ0…"`.
- **Expected**: text with two distinct reserved-invoice badges (Bolt11, Cashu) — confirms detection, no wallet action.

---

## 3. Scenario Catalog — Mentions & Quoted Events

### S-M01 · `nostr:npub…` mention chip (kind:0 resolved)
- **Exercises**: `Segment::Mention(NostrUri::Npub)`; embed store provides the target kind:0 → chip shows display name + avatar.
- **Event**: kind:1 by ALICE, content `"talked with nostr:<BOB_npub> about reconnects"`. Plus BOB kind:0 metadata (`name: "bob", picture: https://nmp.test/img/bob.png`) in the embed store.
- **Expected**: text with an inline mention chip rendering BOB's name + avatar.

### S-M02 · `nostr:nprofile…` mention chip (kind:0 resolved, relay hints)
- **Exercises**: `Segment::Mention(NostrUri::Nprofile)` — nprofile carries relay hints; resolution still by pubkey via embed store.
- **Event**: kind:1 by ALICE, content `"shoutout nostr:<CAROL_nprofile>"`. CAROL kind:0 in store (`name: "carol"`, no picture).
- **Expected**: mention chip with CAROL's name + **D1 identicon placeholder** (no picture in metadata).

### S-M03 · Mention with NO kind:0 (D1 identicon placeholder)
- **Exercises**: D1 best-effort — mention target absent from store → deterministic identicon + truncated npub, never blank / never spinner-gate.
- **Event**: kind:1 by ALICE, content `"ping nostr:<DAVE_npub>"`. DAVE kind:0 **omitted** from store.
- **Expected**: chip with deterministic identicon (seeded by pubkey) + shortened `npub1dave…` label.

### S-M04 · Inline quoted note (`nostr:note1…`)
- **Exercises**: `Segment::EventRef(NostrUri::Note)`; embed store resolves the kind:1 target → inline quoted-note card.
- **Event**: kind:1 by ALICE, content `"this nails it: nostr:<BOB_note>"`. Target = BOB kind:1 `"Relays are a CDN you forgot you were running."` in store.
- **Expected**: text line, then an embedded quoted-note card showing BOB's author chip + the quoted body.

### S-M05 · Inline quoted event (`nostr:nevent1…` with relay hint)
- **Exercises**: `Segment::EventRef(NostrUri::Nevent)` — nevent carries id + relay hint + optional author; resolution by id via store.
- **Event**: kind:1 by ALICE, content `"context: nostr:<BOB_nevent>"`.
- **Expected**: identical quoted-note card to S-M04 (verifies nevent and note resolve to the same shape).

### S-M06 · Quoted note that itself contains a mention
- **Exercises**: one level of embed recursion — quoted card body re-tokenized; inner `Segment::Mention` resolves through the same store. `RenderContext.depth = 1`.
- **Event**: ALICE kind:1 `"see nostr:<CAROL_note>"`; CAROL's note = `"agree with nostr:<BOB_npub> here"` (BOB kind:0 in store).
- **Expected**: quoted card for CAROL whose body itself contains BOB's resolved mention chip.

### S-M07 · Nested quotes (quote → quote → quote, depth 3)
- **Exercises**: multi-level recursion, depth 1→2→3, all under `max_depth = 4` → all expand.
- **Event**: ALICE quotes BOB-note, which quotes CAROL-note, which quotes a plain ALICE-note.
- **Expected**: three nested quoted-note cards, fully expanded, visibly indented per level.

### S-M08 · Recursion depth ≥ 4 → collapse (PD-015)
- **Exercises**: `RenderContext::should_collapse` when `depth >= max_depth (4)` — the 5th level renders a collapsed "quoted event" stub, NOT the body.
- **Event**: a chain of 5 quoted notes (ALICE→N1→N2→N3→N4→N5).
- **Expected**: levels 0–3 expand as cards; the level-4 boundary renders a **collapsed embed card** (id + "tap to open" affordance, no recursive body).

### S-M09 · Cycle A → B → A → visited-set collapse
- **Exercises**: `RenderContext.visited` cycle guard — note A quotes B, B quotes A; the re-entrant A collapses even though depth < max_depth.
- **Event**: ALICE note **A** quotes EVE note **B**; **B** quotes **A** (same id). Self-referential id pair signed offline.
- **Expected**: A expands → shows B expanded → B's quote of A renders as a **collapsed** "already shown" card (cycle broken, no infinite recursion, no crash).

---

## 4. Scenario Catalog — NIP-23 Articles & NIP-51 Lists

### S-A01 · `kind:30023` rich CommonMark article (standalone)
- **Exercises**: `nmp-nip23` decode + view path; `tokenize_with_kind(kind=30023)` → `RenderMode::Markdown`; full CommonMark-core subset (PD-012 / D8).
- **Event**: CAROL kind:30023 with `d`, `title`, `summary`, `published_at` tags and a body exercising **every** allowed block:
  - `# H1`, `## H2`, `### H3`
  - **bold**, *italic*, combined ***bold-italic***
  - ordered list (1. 2. 3.), unordered list (- - -), **nested** list (ordered > unordered child)
  - fenced code block ```rust …``` and inline `code`
  - `> blockquote` (including a nested blockquote)
  - `[link](https://nmp.test/spec)`
  - `![embedded image](https://nmp.test/img/figure.png "Figure 1")`
  - a `nostr:<BOB_npub>` mention **inside the body**
  - a horizontal `---` rule
  - **deliberately included but expected to render as literal text** (PD-012 negative control): a GFM `| table |` row and `~~strikethrough~~`.
- **Expected**: article view with title/summary header; each block rendered to its SwiftUI analogue; the table + strikethrough render as **plain text** (proves PD-012 boundary, no GFM).

### S-A02 · `naddr1` → `kind:30023` inside a `kind:1` → Medium-like preview card
- **Exercises**: `Segment::EventRef(NostrUri::Naddr)` resolving (via store) to a kind:30023 → compact article **preview card** (NOT the full article inline).
- **Event**: ALICE kind:1 `"must-read: nostr:<CAROL_naddr_article>"`; target = the S-A01 article in store.
- **Expected**: text line, then a Medium-style preview card: title + summary + author chip + "Read article" affordance (body NOT expanded inline).

### S-A03 · `naddr1` → NIP-51 follow set (`kind:30000`) inside a `kind:1`
- **Exercises**: `nmp-nip23` (NIP-51 view) addressable-list resolution → inline titled list card. `p` tags enumerated.
- **Event**: ALICE kind:1 `"curated devs: nostr:<CAROL_naddr_followset>"`; target = CAROL kind:30000, `d:"nostr-core"`, `title:"Nostr Core Devs"`, `p` tags for BOB, CAROL, EVE.
- **Expected**: inline titled list card "Nostr Core Devs" with 3 member rows (resolved to mention chips where kind:0 present, D1 identicon otherwise).

### S-A04 · `naddr1` → NIP-51 bookmarks (`kind:30003`) inside a `kind:1`
- **Exercises**: NIP-51 generic addressable list with mixed `e` / `a` / `t` items → inline titled list.
- **Event**: ALICE kind:1 `"my reading list nostr:<CAROL_naddr_bookmarks>"`; target = CAROL kind:30003 `title:"Reading List"` with `e` (a note), `a` (an article coord), `t` (a hashtag).
- **Expected**: titled list card "Reading List" with heterogeneous rows (quoted-note stub, article-coord stub, hashtag chip).

### S-A05 · NIP-51 relay list (`kind:10002`) referenced inline
- **Exercises**: replaceable (non-addressable) NIP-51 list resolution by pubkey+kind; `r` tags with read/write markers → inline titled relay list.
- **Event**: ALICE kind:1 `"caro's relays nostr:<CAROL_nevent_relaylist>"`; target = CAROL kind:10002 with `r` tags (`wss://relay.a` read+write, `wss://relay.b` read-only).
- **Expected**: titled "Relay List" card listing each relay URL with a read/write badge.

---

## 5. Scenario Catalog — Edge / Fallback (D1 best-effort)

> **Invariant for this entire section**: never blank, never crash, never a
> permanent spinner. Every failure degrades to a labeled placeholder.

### S-E01 · Malformed bech32 entity
- **Exercises**: tokenizer rejects an invalid `nostr:` token → it stays as literal `Segment::Text`, not a broken chip.
- **Event**: kind:1 `"broken nostr:npub1thisisnotvalidbech32!!! still readable"`.
- **Expected**: the malformed token rendered as plain text; surrounding sentence intact.

### S-E02 · Unknown / unsupported referenced kind
- **Exercises**: `Segment::EventRef` resolves to an event whose kind has no NMP view → generic "unsupported kind N" card (graceful, not blank).
- **Event**: ALICE kind:1 quoting an `nevent` whose target is a kind:31337 (track) event in store.
- **Expected**: a neutral embed card "Unsupported event (kind 31337)" with the id, no crash.

### S-E03 · Dangling `nevent` (target not in store)
- **Exercises**: `Segment::EventRef` whose id is absent from the relay-free store → D1 unresolved-embed stub, NOT an infinite spinner.
- **Event**: ALICE kind:1 quoting an `nevent` with a random id that was never added to the bundle.
- **Expected**: collapsed "Quoted event unavailable" card with truncated id (deterministic, no spinner-gate).

### S-E04 · Profile mention with no kind:0 metadata at all
- **Exercises**: identical to S-M03 but explicitly the fallback control — D1 identicon + npub label.
- **Event**: ALICE kind:1 mentioning DAVE; **no** DAVE kind:0 anywhere.
- **Expected**: identicon chip + `npub1dave…`, never blank.

### S-E05 · Empty content event
- **Exercises**: `ContentTree::empty` path — tokenizer returns zero segments.
- **Event**: kind:1 with `content: ""`.
- **Expected**: the gallery cell shows an explicit "(empty content)" placeholder, not a zero-height blank cell.

### S-E06 · Article with empty body but valid metadata
- **Exercises**: `nmp-nip23` decode succeeds, body tokenizes to empty → header-only article render (D8 graceful).
- **Event**: CAROL kind:30023 with `title:"Draft"`, `summary:"WIP"`, body `""`.
- **Expected**: article header (title + summary) with an explicit "(no body yet)" note, no crash.

### S-E07 · Naddr → list with zero items
- **Exercises**: NIP-51 list view with no `p`/`e`/`a` entries → titled-but-empty list card.
- **Event**: ALICE kind:1 referencing CAROL kind:30000 `title:"Empty Set"` with no member tags.
- **Expected**: "Empty Set" card with an explicit "(no members)" row.

---

## 6. `ContentTreeDto` — Proposed FFI Projection (input to T93)

Serde-derivable mirror of `nmp_content` IR, 1:1 with the Rust variants.
Lives in **STAGE 2's crate only**; offered as the candidate shape for the
T93 ADR. Field names are JSON-stable.

```jsonc
// ContentTreeDto
{
  "mode": "Plain" | "Markdown",
  "segments": [SegmentDto, ...]
}

// SegmentDto — tagged union, "type" discriminator
{ "type": "text", "text": "…" }
{ "type": "mention", "uri": "nostr:npub1…", "kind": "npub"|"nprofile",
  "pubkey": "<hex>" }
{ "type": "eventRef", "uri": "nostr:nevent1…",
  "kind": "note"|"nevent"|"naddr", "id": "<hex>|<naddr-coord>" }
{ "type": "hashtag", "tag": "nostr" }
{ "type": "url", "url": "https://…" }
{ "type": "media", "mediaKind": "Image"|"Video"|"Audio",
  "urls": ["https://…", …] }
{ "type": "emoji", "shortcode": "rocket", "url": "https://…"|null }
{ "type": "invoice", "invoiceKind": "Bolt11"|"Bolt12"|"Cashu",
  "value": "lnbc…" }
{ "type": "markdownBlock", "node": MarkdownNodeDto }

// MarkdownNodeDto (CommonMark-core only, PD-012)
{ "type": "heading", "level": 1, "inlines": [MarkdownInlineDto, …] }
{ "type": "paragraph", "inlines": [MarkdownInlineDto, …] }
{ "type": "blockQuote", "blocks": [MarkdownNodeDto, …] }
{ "type": "codeBlock", "info": "rust"|null, "body": "…" }
{ "type": "list", "orderedStart": 1|null, "items": [[MarkdownNodeDto,…], …] }
{ "type": "rule" }

// MarkdownInlineDto
{ "type": "inline", "segment": SegmentDto }
{ "type": "emphasis", "children": [MarkdownInlineDto, …] }
{ "type": "strong",   "children": [MarkdownInlineDto, …] }
{ "type": "code", "text": "…" }
{ "type": "link",  "label": [MarkdownInlineDto, …], "href": "https://…"|null }
{ "type": "image", "alt": "…", "title": "…"|null, "src": "https://…"|null }
{ "type": "softBreak" }
{ "type": "hardBreak" }
```

### 6.1 Bundle envelope

```jsonc
{
  "version": 1,
  "scenarios": [
    {
      "id": "S-T01",
      "category": "text" | "mentions" | "quotes" | "articles" | "lists"
                  | "fallback",
      "title": "Plain text",
      "exercises": "tokenizer fast-path → single Segment::Text",
      "events": [ SignedEventJson, … ],   // real sigs, valid ids
      "rendered": ContentTreeDto,          // from real tokenize_with_kind
      "embeds": {                          // relay-free pre-resolved store
        "nostr:npub1bob…": {
          "resolvedKind": 0,
          "profile": { "name": "bob", "picture": "https://…"|null },
          "rendered": ContentTreeDto|null
        },
        "nostr:nevent1…": {
          "resolvedKind": 1,
          "event": SignedEventJson,
          "rendered": ContentTreeDto,
          "collapsed": false,              // PD-015 / cycle outcome
          "collapseReason": null|"depth"|"cycle"|"unsupported"|"dangling"
        }
      }
    }
  ]
}
```

`SignedEventJson` is the standard Nostr event object (`id`, `pubkey`,
`created_at`, `kind`, `tags`, `content`, `sig`) — exactly what
`nmp_core::store::RawEvent`/`VerifiedEvent` round-trips, so the bundle is
also injectable via `nmp_app_inject_signed_events` if a later iteration
wants live-kernel rendering instead of pre-tokenized DTOs.

---

## 7. Swift Cell-Renderer Split (STAGE 3, LOC-budgeted)

| File | Renders categories | Scenarios |
| --- | --- | --- |
| `GalleryTextCell.swift` | `text` | S-T01…S-T10, S-E01, S-E05 |
| `GalleryMentionCell.swift` | `mentions` | S-M01…S-M03, S-E04 |
| `GalleryQuoteCell.swift` | `quotes` | S-M04…S-M09, S-E02, S-E03 |
| `GalleryArticleCell.swift` | `articles` | S-A01, S-A02, S-E06 |
| `GalleryListCell.swift` | `lists` | S-A03…S-A05, S-E07 |
| `SegmentDtoView.swift` | shared `SegmentDto` walker | all |

Each cell = title + collapsible raw event JSON disclosure + the
NMP-rendered output (via `SegmentDtoView`, the shared `SegmentDto` walker)
+ embed resolution against the bundle's `embeds` map.

---

## 8. Scenario Count

**31 scenarios**: 10 text (S-T01–S-T10) + 9 mentions/quotes (S-M01–S-M09)
+ 5 articles/lists (S-A01–S-A05) + 7 edge/fallback (S-E01–S-E07).

Screenshot categories (one per category → `docs/perf/content-gallery/`):
`text`, `mentions`, `quotes`, `articles`, `lists`, `fallback` — 6 shots.
