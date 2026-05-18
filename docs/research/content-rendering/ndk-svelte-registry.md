# NDK Svelte Registry: Composable Event Content Rendering — Research

## §1 Registry schema + entry point

The word "registry" in NDK-svelte refers to **two different things**. Do not conflate them:

1. **`registry.json` (the jsrepo packaging manifest)** — a 252 KB build artifact at `/Users/pablofernandez/Work/NDK-nhlteu/svelte/registry/registry.json` (root is a symlink to `registry/registry.json`). It is consumed by the `jsrepo` CLI for shadcn-style copy/paste install. Shape: `{ name, version, defaultPaths: {blocks, builders, components, icons, ui, utils}, items: [{ name, type, files, dependencies, registryDependencies, _imports_, ... }, ...] }` (see header at `registry/registry.json:1`). Items get auto-discovered by `jsrepo.config.ts:74-168` walking `src/lib/registry/{blocks,builders,components,icons,ui,utils}/`. The schema rule worth knowing: jsrepo item names are **flat** — `user-profile` resolves, `components/user-profile` does not (smoke-test caveats list at `temp/ndk-svelte-registry-smoke/src/routes/+page.svelte:59`).

2. **`ContentRenderer` (the runtime rendering registry)** — `registry/src/lib/registry/ui/content-renderer/index.svelte.ts:109-409`. A plain TS class with a `Map<number, HandlerInfo>` for per-kind embedded-event handlers plus four/five nullable component slots for inline tokens (mention, hashtag, link, media, fallback). This is what the NMP synthesizer actually cares about. A `defaultContentRenderer` singleton (`:427`) is the global default.

**Entry-point call sites:**

- `ui/event-content.svelte:30` — `renderer = $derived(rendererProp ?? parentContext?.renderer ?? defaultContentRenderer)`. Top-level entry: parses event content into segments and routes each segment to the renderer.
- `ui/embedded-event.svelte:28` — same resolution; fetches the bech32-referenced event and routes by `kind` to `renderer.getKindHandler(event.kind)?.component`.
- `ui/markdown-event-content/markdown-event-content.svelte:34` — alternate entry that goes through markdown first, then `mount()`s components into placeholder DOM nodes.
- `components/event-card/event-card-root.svelte:73-93` — every `EventCard.Root` provides `ContentRendererContext` via `setContext(CONTENT_RENDERER_CONTEXT_KEY, { get renderer() {...} })`, optionally cloning a parent renderer to inject per-card callbacks.

**HandlerInfo shape** (`content-renderer/index.svelte.ts:16-23`):
```ts
type HandlerInfo = { component: Component<{ndk; event}>; wrapper: NDKWrapper | null; priority: number };
```
`NDKWrapper` is duck-typed (`:8-11`): any object with `kinds?: number[]` and `from?: (event) => event`. The whole NDK class hierarchy (NDKArticle, NDKHighlight, etc.) satisfies it without an explicit interface.

## §2 Composition primitives

The registry is organized in six directories under `src/lib/registry/` (`jsrepo.config.ts:84`):

| Dir | Role | Examples |
|---|---|---|
| `utils/` | pure helpers | `cn.ts`, `hashtag.ts`, `kind-label.ts` |
| `builders/` | reactive state factories (no DOM) | `event-content/event-content.svelte.ts`, `event/thread/`, `media-render/`, `markdown-nostr-extensions/` |
| `ui/` | low-level **headless primitives** with Root + Slot pattern | `ui/user/` (Root, Avatar, Name, Nip05, Banner, Bio, Field, Handle), `ui/event/`, `ui/content-renderer/` |
| `components/` | **molecules** — opinionated, styled, NDK-wired (often variants of the same molecule) | `event-card-classic`, `event-card-compact`, `event-card-inline`, `event-card-fallback`, `article-card`, `article-card-portrait`, `article-card-hero`, `article-card-neon`, `article-card-compact`, `article-card-inline`, `mention`, `mention-modern`, `hashtag`, `hashtag-modern`, `media-basic`, `media-bento`, `media-carousel`, `media-lightbox`, `link-embed`, `link-inline-basic`, `fallback-event-basic`, `event-card-fallback` |
| `blocks/` | **templates / full sections** | `login-compact.svelte` (435 LOC), `signup-block.svelte`, `thread-view-twitter.svelte`, `progressive-reveal-auth/` (multi-section flow), `session-switcher.svelte` |
| `icons/` | SVG components | `arrow-left`, `reply`, `user-add`, ... |

Composition pattern across the three rendering layers (`event-rendering-flow.md:1-138`):

1. **Atoms = UI primitives** (`ui/user/*`) — `User.Root` sets `USER_CONTEXT_KEY`; `<User.Name field="name">`, `<User.Avatar/>`, etc. read context. Render-as-children via Svelte snippets.
2. **Molecules = components** — `EventCard.{Root,Header,Content,Actions,Dropdown}` exported from `components/event-card/index.ts:8-15`. Same Root-context-children pattern. Variants like `event-card-classic.svelte`, `event-card-compact.svelte`, `event-card-inline.svelte` are pre-composed layouts on top of the primitives.
3. **Templates = blocks** — full screens (`thread-view-twitter.svelte`, `login-compact.svelte`).

**Recursion via the embedded loop:** an `EventCard.Content` body contains `EventContent` which routes `event-ref` segments to `EmbeddedEvent`, which looks up a kind handler (typically another `EventCard.*` variant) and recurses (see depth diagram `event-rendering-flow.md:255-306`).

## §3 Override mechanism

Three layered mechanisms, all routing through the same `ContentRenderer`:

**(a) Auto-registration via import side-effects.** Every kind- or token-bound component ships an `index.ts` that calls `register(defaultContentRenderer)` at module load. Examples:

- `components/mention/index.ts:10-15` — `renderer.setMentionComponent(Mention, registration.priority); register();`
- `components/hashtag/index.ts:10-15` — same shape for hashtag.
- `components/media-basic/index.ts:10-15` — `renderer.setMediaComponent(MediaBasic, ...)`.
- `components/event-card-fallback/index.ts:10-15` — `renderer.setFallbackComponent(FallbackCard, ...)`.
- `components/article-card/index.ts:1-16` — `renderer.addKind(NDKArticle, ArticleCardMedium, registration.priority);` (kinds extracted from `NDKArticle.kinds`, wrapping done by `NDKArticle.from`).
- `components/event-card-compact/index.ts:1-16` — `renderer.addKind(registration.kinds, EventCardCompact, ...)`.

Priority is read from `metadata.json` per component (e.g. `mention/metadata.json:19-23` → `{ "autoRegister": true, "priority": 1, "type": "mention" }`). The setter pattern in the class (`content-renderer/index.svelte.ts:232-285`) only writes if `priority >= existing`, so apps can ship a higher-priority alternative without uninstalling.

**(b) Programmatic override on a custom renderer.** Apps construct a `new ContentRenderer()`, configure it, and either pass it as a prop or set it in context. Best example is `ui/notification/notification-content.svelte:28-41`:
```ts
const r = new ContentRenderer();
r.addKind([1, 1111], EventCardCompact);     // notifications use compact variant
r.addKind(NDKArticle, ArticleEmbedded);
r.addKind(NDKHighlight, HighlightEmbedded);
r.mentionComponent = MentionModern;
setContext(CONTENT_RENDERER_CONTEXT_KEY, { get renderer() { return r } });
```

**(c) Per-card callback injection via `clone()`.** `EventCard.Root` (`event-card-root.svelte:72-88`) clones the inherited renderer when local `onUserClick`/`onEventClick`/`onHashtagClick`/`onLinkClick`/`onMediaClick` props are present, preserving all registered components but binding to local callbacks. `clone()` impl at `content-renderer/index.svelte.ts:371-408` copies all handler entries plus priorities.

**Resolution order** (codified at `event-content.svelte:30`, `embedded-event.svelte:28`, `markdown-event-content.svelte:34` and documented in `RENDERER_CONSISTENCY_UPDATE.md:22-40`): **prop > Svelte context > `defaultContentRenderer` singleton**.

**`addKind` polymorphism** (`content-renderer/index.svelte.ts:204-225`): one method, two input shapes — `addKind([1, 1111], NoteCard)` (manual kinds, no wrapper) or `addKind(NDKArticle, ArticleCard)` (kinds and `from()` wrapper extracted from the class).

## §4 Token handling

Tokenizer lives in `builders/event-content/utils.ts`. The full set of `ParsedSegment` types (`:3-14`): `text | mention | event-ref | link | media | emoji | hashtag`. Note: `mention` covers both `npub1…` and `nprofile1…`; `event-ref` covers `note1 | nevent1 | naddr1`.

**Regex set** (`builders/event-content/utils.ts:20-28`):
- `EMOJI_SHORTCODE: /:([a-zA-Z0-9_]+):/g`
- `NOSTR_URI: /nostr:(npub1[a-z0-9]{58}|nprofile1[a-z0-9]+|note1[a-z0-9]{58}|nevent1[a-z0-9]+|naddr1[a-z0-9]+)/gi`
- `HASHTAG: /(^|\s)#([a-zA-Z0-9_-￿]+)(?=\s|$|[^\w])/g`
- `MEDIA_FILE: /https?:\/\/[^\s<>"]+\.(jpg|jpeg|png|gif|webp|svg|mp4|webm|mov)(\?[^\s<>"]*)?/gi`
- `YOUTUBE: /https?:\/\/(www\.)?(youtube\.com\/watch\?v=|youtu\.be\/|youtube\.com\/embed\/)([a-zA-Z0-9_-]{11})[^\s<>"]*/gi`
- `URL: /https?:\/\/[^\s<>"]+/gi`

**Pipeline** (`builders/event-content/event-content.svelte.ts:44-72` calls `utils.ts:149-308`):
1. `collectMatches` runs every pattern, sorts by index.
2. `parseContentToSegments` walks matches, emits interleaved `text` segments for gaps, classifies each match via `classifyMatch` (`:108-143`).
3. `decodeNostrUri` validates via `nip19.decode` and rejects malformed bech32 back to text.
4. `groupConsecutiveImages` (`:220-261`) collapses runs of `media`+whitespace into one `media` segment with `data: string[]`. Same for `groupConsecutiveLinks` (`:267-308`).
5. Custom emojis: tags pulled from `event.tags.filter(t => t[0] === 'emoji')` (`event-content.svelte.ts:51-53`), shortcode `:foo:` resolves to URL via `buildEmojiMap` (`utils.ts:34-49`).

**Out of scope** (no segment type and no regex):
- **Markdown** — handled by a separate alternate entry (`markdown-event-content.svelte`) that delegates to `marked` + a custom Nostr extension (`builders/markdown-nostr-extensions/`). Not produced by the main parser.
- **Lightning invoices (`lnbc…`)** — not parsed (no pattern).
- **Cashu tokens** — not parsed.
- **Code blocks / inline code** — only via markdown path.
- **Vine/Spotify/etc oEmbed** — only YouTube has a dedicated pattern; everything else falls to generic URL.
- **NIP-30 emoji vs Unicode emoji** — only `:shortcode:` form is processed; raw Unicode emoji flow through as plain text.

**Rendering switch** lives in `event-content.svelte:43-101` — a single `{#each parsed.segments}` block with `{#if segment.type === ...}` branches; each branch checks `renderer.{token}Component` and falls back to raw text/URL if not registered.

## §5 Async fetch lifecycle

Fully reactive via Svelte 5 runes — no manual cache layer in the rendering registry; caching is delegated to the NDK + adapter underneath.

**Mention fetch** (`components/mention/mention.svelte:15-19`):
```ts
let user = $state<NDKUser | null>();
$effect(() => { ndk.fetchUser(bech32).then(u => user = u); });
```
Trivial — fire-and-forget promise into reactive `$state`. Cache hit/miss handled by `ndk.fetchUser`.

**Embedded event fetch** uses a dedicated builder `createFetchEvent` from `@nostr-dev-kit/svelte` (`svelte/src/lib/builders/fetch-event.svelte.ts:50-120`):
- Returns `{ get event, get loading, get error }` (reactive getters).
- Decodes bech32 → filter via `filterAndRelaySetFromBech32` (`:71`).
- Spins up `ndk.subscribe(...)` with `closeOnEose: true, wrap: true` (`:82-84`); `wrap` causes NDK to return wrapped kind-class instances when applicable.
- `onEvent` rejects stale events (keeps newest `created_at`, dedupes by `id`).
- `$effect` returns a cleanup that calls `sub.stop()`.

**Consumed at `embedded-event.svelte:35-51`:**
```ts
const eventFetcher = createFetchEvent(ndk, () => ({ bech32 }))
let handlerInfo = $derived(renderer.getKindHandler(eventFetcher.event?.kind));
let wrappedEvent = $derived(
  eventFetcher.event && handlerInfo?.wrapper?.from
    ? handlerInfo.wrapper.from(eventFetcher.event)
    : eventFetcher.event
);
```
Triple-state UI render at `:62-91`: spinner, error, kind-handler, fallback handler, raw bech32 — strict ordering.

**Recursion budget:** none. The doc's recursive-embedding example (`event-rendering-flow.md:254-306`) walks four levels deep with no guard. If a kind handler embeds the same `bech32` reachable from itself you will recurse forever. No depth counter in any of the source files inspected.

**Render-time cost:** segment parsing is computed inside a `$derived` getter (`event-content.svelte.ts:48`) — re-runs on every event-content change but not per-frame.

## §6 Theming / context layers

NDK-svelte does **not** carry a dedicated theme registry. What it has is layered context overrides, all keyed off the same symbol (`CONTENT_RENDERER_CONTEXT_KEY`, `content-renderer.context.ts:3`):

1. **App-level default**: import the auto-register `index.ts` for whatever set of components you want; they mutate `defaultContentRenderer`. This is the "import-side-effect theme".
2. **Per-context override** (e.g. notifications): construct a custom `ContentRenderer`, register compact variants, `setContext(CONTENT_RENDERER_CONTEXT_KEY, ...)`. Children read it (`notification-content.svelte:27-41`). Pattern documented in `RENDERER_CONSISTENCY_UPDATE.md:120-138`.
3. **Per-component override**: pass `renderer` prop (`event-content.svelte:19-26`).
4. **Per-callback override**: `clone()` with new callbacks (`event-card-root.svelte:81-87`, impl `:371-408`).

Dark/light, density, branding are **not** registry concerns — they're handled at the styling layer (Tailwind v4 + CSS vars `--primary`, `--muted`, `--border`, e.g. `markdown-event-content.svelte:155-250`) and via component variants in `components/` (`article-card-classic` vs `article-card-neon` vs `article-card-hero`).

NSFW is the one global flag inside the renderer: `blockNsfw: boolean = true` (`content-renderer/index.svelte.ts:114`). The cloning path preserves it (`:381`).

## §7 Smoke test trace

The smoke app at `temp/ndk-svelte-registry-smoke/` is **not** a content-rendering trace. What it actually demonstrates:

- jsrepo install flow: `bun x sv@0.13.0 create ...` → `jsrepo init @ndk/svelte@latest` → `jsrepo add ui/user user-profile` (per `temp/ndk-svelte-registry-smoke/jsrepo.config.ts:5-14`, README at `temp/ndk-svelte-registry-smoke/src/routes/+page.svelte:19-23`).
- NDK bootstrap: `createNDK({ explicitRelayUrls, cacheAdapter: NDKCacheAdapterSqliteWasm })` and `setContext(NDK_CONTEXT_KEY, ndk)` in `+layout.svelte:9-14`, NDK ctor at `src/lib/ndk.ts:9-12`.
- Two render paths for a profile: the molecule `UserProfile` (lines 29-39 in `+page.svelte`) and the headless `User.Root → User.Avatar / User.Name / User.Nip05` primitive composition (`+page.svelte:42-53`). `User.Root` (`src/lib/components/ui/user-root.svelte:38-72`) shows the actual context pattern: resolve `NDKUser` from any of `user|pubkey|npub`, fetch profile via `createProfileFetcher`, expose all of it through `USER_CONTEXT_KEY` using reactive getters.
- The "Observed caveats" section (`+page.svelte:56-62`) is the load-bearing finding: jsrepo names are flat, the installed `user-profile` still requires explicit `ndk` prop, and registry utilities expect the app to provide NDK context under `NDK_CONTEXT_KEY`.

**The real end-to-end content-rendering trace lives in `event-rendering-flow.md:254-306`** — the recursive embedding diagram with kind 1 → article 30023 → note 1 → npub. No live app version exists in the inspected tree.

## §8 Design doc highlights

`event-rendering-flow.md` (350 LOC) is a five-section visual map. Load-bearing claims:

1. **5-level rendering chain** (`:5-138`) — `EventCardClassic` → `EventCard.Content` → `EventContent` → `EmbeddedEvent` → kind handler. Cited lines into each source file are accurate (verified at `event-content.svelte:30`, `embedded-event.svelte:38`, etc.).
2. **Two-context invariant** (`:308-349`): `EVENT_CARD_CONTEXT_KEY` is **local** to each `EventCard.Root` (overwritten by nested cards), while `CONTENT_RENDERER_CONTEXT_KEY` **flows down** through nesting. This split is what enables recursive embedding without forgetting the renderer config but also without leaking the outer event identity into the inner card.
3. **`addKind` dual modes** (`:200-241`): NDK wrapper classes auto-extract kinds via `target.kinds` and store `.from` as wrapper; arrays of kinds register without wrapping.
4. **Registry is import-time stateful** (`:235-251`): registering happens as a side effect of importing a component's `index.ts`. The runtime state of `handlers` literally depends on which `import` statements have been evaluated.
5. **Parser produces a flat segment array** (`:147-197`); image/link grouping is a second pass.

The companion doc `RENDERER_CONSISTENCY_UPDATE.md` (271 LOC) records a deliberate refactor enforcing the `prop ?? context ?? default` invariant across **all three** rendering entry points. Read this for the rationale and migration guide.

## §9 Code shape + LOC + patterns

**Whole registry (`src/lib/registry/`, excluding tests):** 25,144 LOC across 434 files (206 `.svelte`, 228 `.ts`).

**Core content-rendering surface:**
- `ui/content-renderer/index.svelte.ts` — 427 LOC (the class itself; only ~75 LOC of essential logic, rest is doc + clone + diagnostics).
- `ui/content-renderer/content-renderer.context.ts` — 8 LOC (just the symbol + interface).
- `ui/content-renderer/index.ts` — 2 LOC (barrel).
- `ui/event-content.svelte` — 102 LOC.
- `ui/embedded-event.svelte` — 91 LOC.
- `ui/markdown-event-content/markdown-event-content.svelte` — 252 LOC (the markdown alternate, includes scoped CSS).
- `builders/event-content/utils.ts` — 308 LOC (tokenizer + groupers).
- `builders/event-content/event-content.svelte.ts` — 72 LOC (reactive wrapper).
- Total content-rendering core: roughly **1,250 LOC**, plus per-renderer components (mention 42 LOC, hashtag, fallback-card, media-basic, etc.).

**Public API entry points (per file inspection):**
- `ContentRenderer` class + `defaultContentRenderer` singleton (`content-renderer/index.svelte.ts:109, 427`).
- `<EventContent ndk event|content emojiTags renderer class>` (`event-content.svelte:10-17`).
- `<EmbeddedEvent ndk bech32 renderer onclick class>` (`embedded-event.svelte:8-14`).
- `<MarkdownEventContent ndk content emojiTags renderer class>` (`markdown-event-content.svelte:14-20`).
- `EventCard.{Root,Header,ReplyIndicator,Content,Actions,Dropdown}` (`components/event-card/index.ts:8-15`).
- `createFetchEvent(ndk, () => config)` (`svelte/src/lib/builders/fetch-event.svelte.ts:50`).
- `CONTENT_RENDERER_CONTEXT_KEY` + `ContentRendererContext` (`content-renderer.context.ts:3-7`).
- Per-token interfaces: `MentionComponent`, `HashtagComponent`, `LinkComponent`, `MediaComponent` (`content-renderer/index.svelte.ts:28-64`).

**Patterns used (Svelte 5 idioms):**
- **Runes everywhere:** `$state`, `$derived`, `$derived.by`, `$effect`, `$props`. Example: `embedded-event.svelte:38-51`.
- **Reactive context via getters:** `setContext(KEY, { get renderer() { return renderer } })` (`event-content.svelte:33-35`, `embedded-event.svelte:31-33`, `event-card-root.svelte:91-93`). Destructuring breaks reactivity — `ARCHITECTURE.md` calls this out explicitly.
- **Render-prop / slot via Svelte snippets:** `Snippet<[{event: NDKEvent}]>` for headers/footers (`notification-content.svelte:19, 44-46`; `event-card-root.svelte:17-127`).
- **Root + Slot context primitives:** `User.Root` / `EventCard.Root` set context; children read it (`event-card-root.svelte:60-66`, `user-root.svelte:62-71`).
- **Polymorphic dispatch via `{@const Component = renderer.fooComponent}`** then `<Component {...} />` (`event-content.svelte:53, 66, 75, 88`).
- **Imperative mount for markdown hydration:** `mount(renderer.mentionComponent, { target: placeholder, props })` (`markdown-event-content.svelte:71-79`), with manual unmount tracked in a `mountedComponents` array.
- **Pattern is not present in the React package**: `react/src` is hooks-only.

## §10 Worth borrowing for NMP

**Portable to NMP (any UI framework):**

1. **Two-tier registry: per-kind handlers + per-token slots.** Map (kind → Component) for embedded-event routing, plus a small fixed set of nullable slot properties (`mentionComponent`, `hashtagComponent`, `linkComponent`, `mediaComponent`, `fallbackComponent`) for inline tokens. Don't try to make tokens also kind-based — they have different rendering shapes (one URL vs many, with-data vs without). See `content-renderer/index.svelte.ts:120-149` and the switch at `event-content.svelte:48-99`.

2. **Priority-based override on every setter.** `setMentionComponent(c, priority)`, `addKind(target, c, priority)` only overwrites if `priority >= existing` (`content-renderer/index.svelte.ts:208-211, 233-236`). This lets registration be import-order-independent without making apps explicitly deregister defaults.

3. **`NDKWrapper` duck-typed registration.** `addKind(NDKArticle, ArticleCard)` auto-extracts kinds from `target.kinds` and stores `target.from` as the wrapper for `.from(event)` upgrading at render time (`content-renderer/index.svelte.ts:213-225`). Same site does manual `addKind([1, 1111], NoteCard)` for kinds without a wrapper class. Single method, two intents — cleaner than `addKindForClass` + `addKindManual`.

4. **`clone()` for per-instance callback injection.** When `EventCard.Root` receives `onUserClick` props, it clones the inherited renderer with new callbacks (`event-card-root.svelte:72-88`, impl `:371-408`). All registered components stay; only callbacks shift. NMP analog: pass action-handler closures, not action enums, and snapshot the registry at the instance boundary.

5. **`prop ?? context ?? default` resolution, enforced everywhere.** `RENDERER_CONSISTENCY_UPDATE.md:22-40` documents the explicit refactor to make this invariant uniform. The bug story (`:7-19`) is worth reading: an inconsistent component broke nested inheritance. NMP should pick one resolution order and enforce it via a single helper.

6. **Tokenizer as a pure function returning a flat segment list, then grouping passes.** `parseContentToSegments` (`builders/event-content/utils.ts:164-210`) + `groupConsecutiveImages` (`:220-261`) + `groupConsecutiveLinks` (`:267-308`). Separating classification from grouping is the right cut: tokenization is the protocol-level concern, grouping is presentational.

7. **`event-ref` as a unified type for `note1|nevent1|naddr1`.** The renderer doesn't care which encoding; the fetcher converts to a filter (`fetch-event.svelte.ts:71`). NMP should not surface bech32-prefix variation to renderers.

8. **Recursion via context propagation, not prop drilling.** Each level resets `EventCard` context (event-local) but inherits `ContentRenderer` context (config-global). Diagram at `event-rendering-flow.md:310-349`. NMP can pick the same split: per-event view state is local, per-app rendering config is propagated.

**Caveats — borrow with care:**

- **No recursion depth guard.** Apps can construct an event chain that loops. Add a depth counter or visited-set in NMP.
- **Markdown is a separate render path.** `MarkdownEventContent` does *not* share a parser with `EventContent` — both call into the same `ContentRenderer` for tokens, but the segment shape isn't reused. NMP should decide if it wants one parser with markdown as a flag, or two parsers. Don't accidentally fork the two.
- **Tokenizer doesn't cover lightning invoices, cashu, code, non-YouTube embeds, raw Unicode emoji.** If NMP wants those, design extensibility into the parser, not just the renderer.
- **Singleton `defaultContentRenderer` is mutable global state via import side effects.** This works for shadcn-style copy/paste, where every consumer ships an explicit `import './hashtag'` to opt in. It does *not* work well for libraries published to NPM that need lazy loading or SSR isolation — tests must call `renderer.clear()` (`content-renderer/index.svelte.ts:348-365`) to avoid leakage between cases. NMP should consider exposing `createDefaultRenderer()` rather than a mutable singleton.

**Svelte/runes-specific (do NOT directly translate, but the *idea* maps):**

- Reactive `getter` in context object (`event-content.svelte:33-35`) — analog is anything that observes the bound value (Combine flow, Compose state, MutableStateFlow). The principle: never bind by value at the context boundary, bind by reference-to-cell.
- `mount()` for markdown hydration (`markdown-event-content.svelte:71-79`) — every UI toolkit needs some answer for "the parser produced HTML with placeholders, attach native components to those placeholders". On iOS/SwiftUI this is `UIViewRepresentable` islands or `AttributedString` runs with `.background` views.
- Snippet-as-prop (`Snippet<[{event}]>`) — equivalent to render-prop / ViewBuilder / Composable lambda. The point: parent provides layout shell, child library provides data slot.

**Do NOT borrow:**

- The jsrepo packaging registry — that's a *distribution* mechanism for shadcn-style code copy, orthogonal to the rendering registry, and not what the NMP synthesizer is comparing.
- 30+ near-duplicate `*-card-{classic,compact,inline,hero,portrait,neon,glass}` variants. That's design exploration, not a pattern.
- The two-doc structure (`event-rendering-flow.md` + `RENDERER_CONSISTENCY_UPDATE.md`) — but **do** borrow the *practice* of writing one design doc and one invariant-enforcement changelog when refactoring a cross-cutting concern.
