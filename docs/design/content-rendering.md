# Design: Should NMP Provide a UI Content-Rendering Layer?

> **Status:** Draft — design recommendation (no impl). T72.
> **Date:** 2026-05-18
> **Scope:** Should NMP ship "just-works" rendering of Nostr content (`nostr:` embeds, mentions, hashtags, markdown, media)? If yes, in what shape?
> **Inputs (load-bearing):** [`docs/research/content-rendering/ndkswift.md`](../research/content-rendering/ndkswift.md), [`docs/research/content-rendering/ndk-svelte-registry.md`](../research/content-rendering/ndk-svelte-registry.md), `docs/product-spec/overview-and-dx.md` §1.5 (D0–D8), ADR-0009 (kernel boundary), ADR-0010 (per-app concrete enums), `docs/design/kind-wrappers.md` (sister design — same skeleton applied to event typing), `docs/design/framework-magic.md`.

## §1 The question + position

**Yes — but only the substrate, and only with the right partition.** NMP ships a **pure-Rust tokenizer + entity-resolver + embed-fetch-deduplicator** as protocol-module code (Layer A). NMP does **not** ship per-platform UI component packages as framework code (Layer C). The "just-works" composable components apps need are scaffolded into the **starter / per-app tree** as editable source (shadcn / jsrepo idiom), not published as `nmp-content-swiftui` / `nmp-content-compose`. Apps own and modify them.

This position is forced by **D0** (kernel never grows app nouns — and a SwiftUI view IS an app noun), the **RMP bible** (native = rendering only — framework-published per-platform UI packages drag the framework across that line), and **D4** (single writer per fact — runtime renderer registries that mutate global state on import are a second writer). The orchestrator's hint to ship a Layer-C package is rejected on those three grounds; the user need it expresses ("apps shouldn't have to wire content rendering from zero") is fully discharged by scaffold-in-starter. Same first-day UX, different ownership boundary. §6 + §9 defend the choice.

## §2 What's hard about Nostr content rendering

| # | Concern | Today's burden | Where it goes in NMP |
|---|---|---|---|
| 1 | Tokenize text → `text \| mention \| event-ref \| hashtag \| url \| media \| emoji` segments | App (regex copy-paste) | **`nmp-content` substrate** (pure Rust) |
| 2 | Decode `nostr:` URIs (NIP-21) into typed entities | App | **`nmp-nip21`** (in flight; T_x) |
| 3 | Async-fetch mentioned profiles + dedupe per pubkey | App + ad-hoc cache | Kernel `EventStore` + claim/release (existing) |
| 4 | Async-fetch embedded events (`nevent`, `naddr`) + dedupe per id | App per-view (NDKSwift gap — each `EventPreviewLoader` re-subscribes) | **`nmp-content` `EmbedClaimRegistry`** (Layer A); one sub per id |
| 5 | Infinite-recursion guard when an event embeds an ancestor | App (NDKSwift + svelte both lack this) | **`nmp-content` depth counter + visited-set** |
| 6 | Markdown vs plaintext detection | App per-call-site (NDKSwift: 3 overlapping APIs anti-pattern) | **One tokenizer with `RenderMode::{Plain, Markdown, Auto}`** flag |
| 7 | NIP-30 custom emoji (`:shortcode:` resolved via `emoji` tags) | App (neither library handles) | **`nmp-content` tokenizer** |
| 8 | Hashtag classification | App (regex copy-paste) | **`nmp-content` tokenizer** |
| 9 | URL → OG-card metadata fetch | App (HTTP egress) | **App layer — substrate emits segment; app fetches if desired** |
| 10 | Lightning invoice (`lnbc…`) / Cashu token / bolt12 | App (both libs ignore) | **`nmp-content` tokenizer (segment types reserved); rendering app-side** |
| 11 | Media classification (image vs video by ext) + consecutive-image grouping into carousel | App (NDKSwift: `ImageGroupingUtils`; svelte: `groupConsecutiveImages`) | **`nmp-content` post-pass grouper** |
| 12 | Deletion (kind 5) handling for embedded events | App (NDKSwift: no "deleted" branch — loader shows "not found") | **Kernel ingest already handles** (C3 framework-magic); embed registry observes |

Twelve concerns; eight resolve in pure-Rust substrate, two in the kernel (existing), two stay app-owned by design (OG fetch + lnbc rendering — see §10).

## §3 NDKSwift's approach — distilled

Three overlapping entry points (`NDKRichText`, `NDKMarkdown`, `NDKUIMarkdownRenderer` — `ndkswift.md` §1) with two separate parsers (regex `ContentParser` for inline; hand-rolled `MarkdownParser` for blocks — §2, §5). Renderer plug points are **six protocols composed via generic-typealias** (`MentionRenderer`, `HashtagRenderer`, `LinkRenderer`, `ImageRenderer`, `VideoRenderer`, `EventRenderer` — `ndkswift.md` §4); apps assemble `typealias PillStyleRichText = NDKUIRichTextView<PillMentionView, …, DefaultEventView>`. Type-safe; ergonomic for whole-style swaps; **awkward for per-instance overrides**. Default `EventRenderer` has **no per-kind switch** — every app re-implements the kind:1 / kind:30023 / kind:39089 dispatch (`ndkswift.md` §4, §9). `EventPreviewLoader` fetches per view with **no dedupe** and **no recursion guard** (`ndkswift.md` §3, §9). `NDKProfile` LRU cache (500) keyed by pubkey is the one well-shaped centralized substrate (`ndkswift.md` §6, §10.3).

## §4 NDK-svelte registry's approach — distilled

A runtime `ContentRenderer` class with **`Map<kind, HandlerInfo>` + four nullable slot components** (mention / hashtag / link / media / fallback) — `ndk-svelte-registry.md` §1, §10.1. Override is three-layer: import-side-effect mutation of `defaultContentRenderer` singleton; `new ContentRenderer()` with `setContext`; per-card `clone()` for callback injection (`ndk-svelte-registry.md` §3, §10.4). `addKind` polymorphism — `addKind([1,1111], NoteCard)` OR `addKind(NDKArticle, ArticleCard)` (duck-typed wrapper auto-extracts kinds via `target.kinds`, `target.from`) (`ndk-svelte-registry.md` §1, §10.3). Resolution invariant **`prop ?? context ?? default`** enforced uniformly post-refactor (`ndk-svelte-registry.md` §3, §10.5). Tokenizer is six regexes producing a flat segment array + grouping passes (`ndk-svelte-registry.md` §4, §10.6). **Markdown is a parallel render path with the same renderer slots**, not a forked parser (better than NDKSwift). **No recursion depth guard**, **no embed fetch dedup** — explicit caveats (`ndk-svelte-registry.md` §5, §10 caveats).

## §5 The NMP-shaped answer — layers

| Layer | Verdict | Where it lives | Why |
|---|---|---|---|
| **A. Pure-Rust tokenizer + entity resolver + embed-fetch dedup** | **SHIP in v1** | `nmp-content` (new crate) + `nmp-nip21` (in flight) | D0-clean: zero UI nouns. Composes with `EventStore` + `ViewModule`. The substrate every platform needs. Adds the two gaps both libraries leak: recursion guard + embed dedup. |
| **B. A Rust-side `RenderableContent` ViewModel projection** | **REJECT** | (would be `nmp-core` / a sibling crate) | `ViewModule::Payload` is already this. Adding a parallel `RenderableContent` type is the NDKSwift "three overlapping APIs" anti-pattern in Rust. Apps already get `ContentTree` as a payload field on existing payloads; no new abstraction needed. |
| **C. Per-platform UI primitives published as `nmp-content-swiftui` / `nmp-content-compose` / `nmp-content-iced` / `nmp-content-web`** | **REJECT** as framework code; **SHIP as starter scaffolds** | App's own tree (copied by `nmp init` / `nmp add component`) | RMP bible: native = rendering only. Framework-published platform-UI packages cross that line and become a v2-migration trap (see NDKSwift's 3-API anti-pattern). Scaffold-in-starter delivers identical first-day UX with app-ownership. NDK-svelte's jsrepo model is the architectural precedent — components ship as **copy-paste**, not as an imported library. |

**Layer A in detail.** `nmp-content::tokenize(content: &str, tags: &[Tag]) -> ContentTree` emits:

```rust
pub enum Segment {
    Text(String),
    Mention(NostrUri),          // Profile-variant only; resolved via NIP-19/21
    EventRef(NostrUri),          // Event / Address variants
    Hashtag(String),
    Url(url::Url),
    Media { urls: Vec<url::Url>, kind: MediaKind },   // grouped post-pass
    Emoji { shortcode: String, url: Option<url::Url> }, // NIP-30
    Invoice(InvoiceKind),       // segment reserved; app renders. lnbc/bolt12/cashu
    MarkdownBlock(MarkdownNode), // populated only when RenderMode::Markdown
}
pub struct ContentTree {
    pub segments: Vec<Segment>,
    pub mode: RenderMode,        // Plain | Markdown | Auto (auto sniffs by kind)
}
```

`nmp-content::EmbedClaimRegistry` holds a `HashMap<EventIdOrAddress, ClaimHandle>` so N references to the same `nevent1…` share **one** subscription + one cached resolved `StoredEvent`. The registry is **a `ViewModule`** (per ADR-0009) named `nmp.content.embed_registry`; it participates in the existing claim/release reactor (the same machinery `ProfileInterestAvatar` uses today — see commit `2cd423a`).

**Recursion guard.** `ContentTree` is opaque to the renderer; the renderer is given a **`RenderContext { depth: u8, visited: SmallVec<[EventId; 8]> }`** via the per-platform consumer pattern. `nmp-content` exposes a helper `render_context_can_descend(ctx, into: &EventId) -> bool` that an embed component calls before mounting its child renderer. Max depth: 4 (configurable per app; default matches the NDK-svelte recursion finding in `docs/research/content-rendering/ndk-svelte-registry.md:135`).

## §6 The composability / override story

The override question NDK-svelte's runtime registry solves (priority-based setter, three-layer resolution, `clone()` for per-instance callbacks — `ndk-svelte-registry.md` §3) **collapses** when components live in the app's own tree: **the app overrides by editing the file**. There is no library to register against. A podcast app whose nevent card shows duration + chapter markers literally edits `NostrEventCard.swift` (copied from the starter) and adds the podcast-specific branch. The diff is reviewable; the override is git-tracked; no priority arithmetic, no import-order dependence, no test-time `renderer.clear()`.

Apps that **do** want runtime polymorphism (a reskinning toggle; per-context variant — notifications use a compact card; a/b tests) get the substrate emitting structured `ContentTree`; their per-platform composition layer dispatches however that platform idioms it:

| Platform | Composition idiom for "swap this token's renderer in this subtree" |
|---|---|
| SwiftUI | `@EnvironmentValues` + `ViewModifier` (the existing `.onMentionTap` pattern in highlighter) |
| Jetpack Compose | `CompositionLocal<MentionRenderer>` |
| iced (desktop) | Closure-pinned widget factories on the parent element |
| Web (wasm) | Component slots / context provider (the NDK-svelte pattern, idiomatic in that ecosystem) |

These aren't portable and shouldn't be. NMP does **not** publish a Rust-side renderer registry that pretends to unify them — the cross-platform substrate is `ContentTree`; the override mechanism is each platform's native idiom. State this and move on.

## §7 Worked example — kind:1 with `nostr:nevent1...` mention

Input event (kind:1, content):

```
GM. Loved this thread by nostr:nprofile1qqsx... — see also nostr:nevent1qqs... and #nostr
```

**Pipeline trace:**

1. **Ingest (kernel).** Raw event arrives via wire, `verify_and_persist` writes to `EventStore`. No content parsing at ingest (D8 hot-path; tokenizing is render-time).
2. **View open (app).** A `TimelineView` `ViewPayload` includes `content: String` per row. The app's row component asks for `nmp_content::tokenize(content, &tags)` lazily on first render. Result is cached on the view payload via the existing `ViewModule::Delta` reactivity (re-tokenize only on `content` change — same shape as NDKSwift's `.task(id: content)` `ndkswift.md` §10.6).
3. **`tokenize` (`nmp-content`).** Walks regex set (mirrors `ndk-svelte-registry.md` §4 regex shape; `nostr:` URI decode delegates to `nmp_nip21::parse_nostr_uri` — sibling worktree, in flight). Emits `[Text("GM. Loved this thread by "), Mention(NostrUri::Profile{...}), Text(" — see also "), EventRef(NostrUri::Event{...}), Text(" and "), Hashtag("nostr")]`. Pure function; no I/O.
4. **Embed claim (`nmp-content::EmbedClaimRegistry`, app dispatches at render).** The Swift row sees `Segment::EventRef(uri)`, calls (FFI) `embed_registry_claim(uri)`. Registry returns `(ClaimHandle, Option<StoredEvent>)` — `None` if cold. If cold, registry compiles a `OpenEvent` interest into the planner (existing M2 machinery); when EOSE fires for that filter the planner closes it (`closeOnEose: true`, the discipline NDKSwift gets wrong per `ndkswift.md` §10.6). Same `nevent` referenced in 10 timeline rows = **one** sub.
5. **Render (Swift, app-owned).** `NostrRichText.swift` walks `ContentTree.segments`, builds a SwiftUI `Text` for inline runs, and uses entity-card views for `EventRef` segments. Recursion-guard: nested note cards render their own content with `NostrRichText` but check `RenderContext.can_descend(into: event.id)` first.
6. **App override.** A future app can edit the starter `NostrEntityCard.swift` to add app-specific card cases. No NMP code changes. The app-specific branch reads that app's domain record (per the kind-wrappers ADR §3.3) and composes with the per-app FFI types (ADR-0010).

**Conflict check with kind-wrappers ADR §6.** The kind-wrappers `DomainModule::ingest_kinds` hook decodes whole events into domain records **at ingest** (typed records into LMDB). `nmp-content::tokenize` decodes inline `nostr:` URIs from inside **content strings at render time**. Different paths; no conflict. The render-time decode does **not** populate the domain store — it just hands the renderer enough to dispatch.

## §8 What "just works" — and what doesn't

**Just works (10) — zero app code beyond instantiating the starter components:**

| # | Behavior |
|---|---|
| 1 | `nostr:npub1…` / `nostr:nprofile1…` mention → inline `@DisplayName` with avatar, name auto-fetched via existing profile claim/release (commit `2cd423a`) |
| 2 | `nostr:nevent1…` / `nostr:note1…` / `nostr:naddr1…` embed → block-level card with per-kind dispatch, event auto-fetched + deduped by `EmbedClaimRegistry` |
| 3 | `#hashtag` → tappable chip (callback = app, default = no-op) |
| 4 | `https://…` URL → tappable link |
| 5 | `https://…/foo.jpg` (and consecutive images) → grouped media segment |
| 6 | NIP-30 `:shortcode:` resolved against event's `emoji` tags |
| 7 | Markdown blocks (when `RenderMode::Markdown`) → headings, lists, code, blockquote, bold/italic — without forking the inline tokenizer |
| 8 | Cold-start placeholder shape (D1): mention shows `@npub1abc…`, card shows skeleton; in-place upgrade on data arrival (existing `ProjectionChange`) |
| 9 | Deleted (kind 5) embed → "deleted" tombstone state (kernel ingest already removes; embed registry observes — NDKSwift gap) |
| 10 | Recursion guard prevents infinite quote-chain mounts (NMP-specific; both libs lack) |

**App owns (5) — by design:**

| # | Concern | Why app-owned |
|---|---|---|
| 1 | Visual styling (fonts, colors, spacing, dark mode) | Per-app brand. Reference impl in starter; app edits. |
| 2 | Tap navigation (where does a mention tap go?) | App routing; NMP has no router |
| 3 | OG-card / link-preview HTTP fetch | Network egress; D7 capabilities pattern — app or app's capability module |
| 4 | Lightning invoice / Cashu rendering (the segments are emitted; rendering of pay UX is app/wallet concern) | App-domain UX; v1 has no wallet anyway (M12 deferred) |
| 5 | Layout density (compact vs expanded) | App-level variant; the per-kind cards in starter ship one shape, app forks |

## §9 Phasing

| Phase | Deliverable | Milestone slot |
|---|---|---|
| **Phase 1 (immediate)** | `nmp-content` crate skeleton: `Segment`, `ContentTree`, `tokenize`, `RenderMode`, `RenderContext`. Lives alongside `nmp-nip21` (which it depends on for URI parse). | Fold into Chirp's content-rendering pipeline first; no new milestone slot. |
| **Phase 2 (M16-adjacent)** | `EmbedClaimRegistry` ViewModule + recursion guard helpers. Wired into Pulse (the builder-guide e2e app) for cold-start visibility. Starter scaffolds (`nmp init` copies `NostrRichText.swift` / `NostrEntityCard.swift` into new apps' trees). Component install/update plan lives in [`../plan/m16-component-registry.md`](../plan/m16-component-registry.md). | **M16 (CLI + starter)** — scaffolding is M16's existing scope. |
| **Phase 3 (post-v1)** | Per-platform reference impls for Compose, iced, web (each scaffolded by `nmp init --platform=…`). Optional `nmp-content-markdown` subcrate if markdown segment shape grows. | Post-v1 with the cross-platform milestone (M15 already in scope). |

**Why no new milestone.** The substrate is ~600–900 LOC of Rust (tokenizer + dedup registry + helpers) plus the relocate-from-Highlighter work; smaller than a milestone arc. The starter scaffolds are M16's job by definition. A new ladder rung for this would inflate the plan; folding into M11.5 follow-up + M16 keeps the ladder honest.

## §10 Risks + anti-patterns to forbid

| # | Anti-pattern | Reason |
|---|---|---|
| 1 | SwiftUI / Compose / iced views in any `nmp-*` Rust crate | RMP bible: native = rendering; framework code stays headless |
| 2 | Three overlapping public APIs (NDKSwift's `NDKRichText` + `NDKMarkdown` + `NDKUIMarkdownRenderer`) | One tokenizer with `RenderMode` flag; ndkswift.md §10.1 explicitly warns |
| 3 | Forked parsers for markdown vs plain | Same shape risk; single `ContentTree` with `MarkdownBlock` variants when mode = Markdown |
| 4 | Per-view embed subscriptions without dedup (NDKSwift's `EventPreviewLoader`) | `EmbedClaimRegistry` is one sub per id, refcounted |
| 5 | No recursion depth guard (both libs) | Required `RenderContext.can_descend` check |
| 6 | Runtime renderer registry as global mutable singleton (NDK-svelte's `defaultContentRenderer`) | D4 violation if cross-thread; test-leak risk; per-app composition is the right idiom |
| 7 | OG-card HTTP fetch in `nmp-core` or `nmp-content` | Kernel-side HTTP egress conflates substrate with policy; D7 violation; lives in app capability module |
| 8 | Markdown crate version baked into the substrate's public API | Pick an internal markdown crate; expose only `MarkdownBlock` enum so we can swap without breaking apps |
| 9 | Cross-platform UI primitives published as framework packages (`nmp-content-swiftui` etc.) | Layer-C rejection (§1, §5) — scaffold-in-starter is the answer |
| 10 | Tokenizer baked to a specific framework's regex engine | Use `regex` crate; emit `Segment` enum; consumers don't see regex |

## §11 Decision matrix

| I want to … | Where it goes |
|---|---|
| Tokenize a kind:1 content string into segments | `nmp_content::tokenize(content, &tags)` — pure fn |
| Render a `nevent1…` mention with avatar + name | `EmbedClaimRegistry.claim(uri)` (Rust) → starter `NostrEntityCard.swift` (app-tree) |
| Override how my podcast app renders a kind:23196 embed | Edit the copied `NostrEntityCard.swift`; add `case 23196` |
| Render markdown article (kind 30023) | `tokenize(content, tags) with RenderMode::Markdown` → starter `NostrMarkdownView.swift` |
| Add custom emoji (NIP-30) | Substrate already emits `Segment::Emoji`; starter component renders |
| Fetch OG card for a URL | **App-owned.** Substrate emits `Segment::Url`; app's link-preview component fetches via its own HTTP capability |
| Show a lightning invoice prettily | Substrate emits `Segment::Invoice(InvoiceKind::Bolt11)`; **app/wallet component renders** (M12-Wallet deferred — segment shape ships v1, renderer waits for wallet) |
| Embed a video | Substrate emits `Segment::Media{kind: Video}`; starter component picks player |
| Render the same `nevent1…` in 10 timeline rows | `EmbedClaimRegistry` shares one fetch + one cached resolved event across all 10 |
| Quote a note that quotes itself transitively | `RenderContext.can_descend` returns false at depth=4; renderer shows "(see full thread)" link |
| Swap mention chip styling per-screen on iOS | SwiftUI `@EnvironmentValues` modifier scoped to that subtree — app-side |
| Disable framework rendering and roll my own | Don't depend on `nmp-content`; tokenize in app code. The substrate is opt-in like every other protocol module (ADR-0010 module set) |

## §12 Open questions (for orchestrator / user)

- **PD-011 — `nmp-content` vs `nmp-nip21` split.** NIP-21 URI parsing clearly belongs in `nmp-nip21` (sibling worktree, T_x in flight). Tokenizer for non-NIP concerns (hashtag, URL, media-ext, NIP-30 emoji, markdown blocks) — separate crate `nmp-content`, or a module in `nmp-nip21`? Recommend separate: NIP-21 is wire-format; `nmp-content` is render-format. Different change radius.
- **PD-012 — Markdown crate choice.** `pulldown-cmark` (mature, fast, CommonMark) vs `comrak` (GFM extensions). Recommend `pulldown-cmark` for stricter spec adherence and smaller dep tree; revisit if NIP-23 grows GFM-isms.
- **PD-013 — `EmbedClaimRegistry` as `ViewModule` vs as a kernel-internal cache.** ViewModule keeps it D0-clean and apps can debug-inspect (D8 diagnostics surface). Kernel-internal is slightly faster but opaque. Recommend ViewModule.
- **PD-014 — Starter-scaffold delivery.** `nmp init` copies static files (jsrepo / shadcn model) vs `nmp add component nostr-rich-text` lazy fetch (svelte-shadcn model). Recommend `nmp init` plants the full set + `nmp add component <name>` for opt-in extras. The starter MUST work without network on first build.
- **PD-015 — Recursion depth default.** NDK-svelte's worked example chains 4 levels (`docs/research/content-rendering/ndk-svelte-registry.md:135`). Recommend default `max_depth = 4`, configurable per app. Beyond depth=4 the embed card collapses to a "see full thread" link.

---

**Bottom line.** Ship the Rust substrate that does the load-bearing work (tokenize, resolve, dedupe, depth-guard) — both research libraries leak gaps a kernel-owned crate naturally closes. Do **not** ship per-platform UI packages as framework code — that path is the NDKSwift "three overlapping APIs" trap one indirection up. Scaffold reference components into the starter tree (jsrepo / shadcn idiom — NDK-svelte's distribution model, refactored for app-ownership), and let each platform's native composition idiom be the override mechanism. The existing highlighter `NostrRichText.swift` + `NostrEntityCard.swift` are the proof of concept; this design generalizes them into Layer A substrate and relocates them as starter reference.
