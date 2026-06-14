import { Show, createEffect, createMemo, createSignal, onCleanup, type JSX } from "solid-js";
import { NostrProfileHostProvider } from "./components/user-avatar/NostrProfileHost";
import { NostrAvatar } from "./components/user-avatar/NostrAvatar";
import { NostrProfileName } from "./components/user-name/NostrProfileName";
import { NostrNip05Badge } from "./components/user-nip05/NostrNip05Badge";
import { NostrUserCard } from "./components/user-card/NostrUserCard";
import { NostrRelayList } from "./components/relay-list/NostrRelayList";
import { NostrLoginBlock } from "./components/login-block/NostrLoginBlock";
import { NostrContentView } from "./components/content-view/NostrContentView";
import { NostrMinimalContentView } from "./components/content-minimal/NostrMinimalContentView";
import { NostrMentionChip } from "./components/content-mention-chip/NostrMentionChip";
import { NostrMediaGrid } from "./components/content-media-grid/NostrMediaGrid";
import { NostrArticleCard } from "./components/content-kind-30023/NostrArticleCard";
import { NostrHighlightCard } from "./components/content-kind-9802/NostrHighlightCard";
import { NostrQuoteCard } from "./components/content-quote-card/NostrQuoteCard";
import { NostrEmbeddedEvent, type EmbeddedEventModel } from "./components/content-kind-registry/NostrKindRegistry";
import { NostrNpubChip } from "./components/user-npub/NostrNpubChip";
import { createGalleryRuntime, type ClaimedEventWire, tagValue } from "./nmp/profileHost";
import {
  SHOWCASE_PUBKEY,
  SHOWCASE_RELAYS,
  SHOWCASE_NOTE,
  SHOWCASE_ARTICLE,
  SHOWCASE_HIGHLIGHT,
} from "./showcase";
import { WireNodeKind } from "./nmp/generated/nmp/content/wire-node-kind";

const runtime = createGalleryRuntime();

// Start the kernel immediately. Relays connect asynchronously after Start.
void runtime.start(SHOWCASE_RELAYS);

export default function App(): JSX.Element {
  // Resolve the showcase profile (and, as events arrive, their authors) with a
  // release-reclaim retry. A profile claim's kind:0 REQ is dropped if its indexer
  // relay (e.g. purplepag) isn't connected yet when the claim fires — and
  // claim_profile then dedupes ("already requested"), so NO later claim ever
  // re-fetches it (it stays poisoned by the dropped REQ). Releasing the sole
  // consumer clears that requested state; re-claiming issues a fresh REQ once
  // more indexers are connected. One consumer per pubkey keeps the release
  // effective (avatar components add their own consumer only after resolution).
  const profileConsumer = (pk: string) => `gallery-profile-${pk.slice(0, 8)}`;
  const profileResolved = (pk: string) => {
    const p = runtime.host.profile(pk);
    return !!p && (!!p.displayName || !!p.pictureUrl);
  };
  let profileRetryStarted = false;
  createEffect(() => {
    if (!runtime.anyIndexerConnected() || profileRetryStarted) return;
    profileRetryStarted = true;
    let attempt = 0;
    const tick = () => {
      const desired = new Set<string>([SHOWCASE_PUBKEY]);
      for (const ev of [noteRaw(), articleRaw(), highlightRaw()]) {
        if (ev?.authorPubkey) desired.add(ev.authorPubkey);
      }
      let allResolved = true;
      for (const pk of desired) {
        if (profileResolved(pk)) continue;
        allResolved = false;
        const c = profileConsumer(pk);
        if (attempt > 0) runtime.host.releaseProfile(pk, c);
        runtime.host.claimProfile(pk, c);
      }
      attempt += 1;
      if (allResolved || attempt >= 20) clearInterval(timer);
    };
    tick();
    const timer = setInterval(tick, 3000);
    onCleanup(() => clearInterval(timer));
  });

  // Claim the showcase events for the content-view component. Event-id fetches
  // route through the content lane, so gate on a CONTENT relay being connected —
  // claiming before the content socket is open drops the REQ (the wasm transport
  // has no on-demand dial or retry). Same edge-trigger discipline as the
  // indexer-gated profile claim above.
  // Claim the showcase events, retrying any that haven't resolved. A claim can
  // be dropped if its hint relay's socket isn't open yet when the REQ is sent
  // (the wasm transport drops a frame to a not-yet-connected relay, no retry),
  // and a relay can be transiently slow. So we re-claim each unresolved event on
  // an interval with a FRESH consumer id (forcing a new REQ) until it resolves
  // or we exhaust the budget. Idempotent and self-stopping.
  let claimStarted = false;
  const claimTargets = [
    { uri: SHOWCASE_NOTE.uri, id: SHOWCASE_NOTE.primaryId, consumer: "gallery-note" },
    { uri: SHOWCASE_ARTICLE.uri, id: SHOWCASE_ARTICLE.primaryId, consumer: "gallery-article" },
    { uri: SHOWCASE_HIGHLIGHT.uri, id: SHOWCASE_HIGHLIGHT.primaryId, consumer: "gallery-highlight" },
  ];
  createEffect(() => {
    if (!runtime.anyContentConnected() || claimStarted) return;
    claimStarted = true;
    let attempt = 0;
    const tick = () => {
      let allResolved = true;
      for (const t of claimTargets) {
        if (runtime.claimedEvent(t.id)) continue;
        allResolved = false;
        // claim_event dedupes ("already requested → no fetch"), so a first REQ
        // dropped because its hint relay wasn't connected yet would never retry.
        // Release first (drops the last consumer → clears the requested state),
        // then re-claim to force a FRESH REQ now that more relays are connected.
        if (attempt > 0) runtime.releaseEvent(t.uri, t.consumer);
        runtime.claimEvent(t.uri, t.consumer);
      }
      attempt += 1;
      if (allResolved || attempt >= 15) clearInterval(timer);
    };
    tick();
    const timer = setInterval(tick, 3000);
    onCleanup(() => clearInterval(timer));
  });

  // Ask the kernel (Rust NIP-19 encoder) for the showcase identity's npub once
  // the worker is up — never bech32-encode in the browser (aim.md §6.9).
  runtime.requestNpub(SHOWCASE_PUBKEY);

  // A claimed event is "render-ready" only once the kernel has parsed it into a
  // non-empty, placeholder-free NFCT content tree. This is the honesty gate: it
  // guarantees the screenshot shows tree-derived rendering, never the raw-string
  // fallback (which would look identical but mean the parse path never ran).
  const noteEvent = createMemo(() => contentReady(runtime.claimedEvent(SHOWCASE_NOTE.primaryId)));
  const articleEvent = createMemo(() =>
    contentReady(runtime.claimedEvent(SHOWCASE_ARTICLE.primaryId)),
  );
  // The article/highlight CARDS render from event tags + content (not the NFCT
  // tree), so they gate on the raw event being resolved, not on a content tree.
  const articleRaw = createMemo(() => runtime.claimedEvent(SHOWCASE_ARTICLE.primaryId));
  const highlightRaw = createMemo(() => runtime.claimedEvent(SHOWCASE_HIGHLIGHT.primaryId));
  const noteRaw = createMemo(() => runtime.claimedEvent(SHOWCASE_NOTE.primaryId));
  const showcaseNpub = createMemo(() => runtime.npub(SHOWCASE_PUBKEY));
  // Captured once at mount for the relative-time labels on quoted events.
  const nowSeconds = Math.floor(Date.now() / 1000);

  // Media-grid honesty: only show images that ACTUALLY load. Old articles can
  // carry dead/relative image links; the goal forbids showing a broken image, so
  // we preload each candidate and keep only the ones that decode. The component
  // stays a pure renderer; the host curates real, loaded media.
  const [loadedMedia, setLoadedMedia] = createSignal<string[]>([]);
  const probedMedia = new Set<string>();
  createEffect(() => {
    const candidates = collectMediaUrls(articleRaw()).filter((u) => /^https?:\/\//.test(u));
    for (const url of candidates) {
      if (probedMedia.has(url)) continue;
      probedMedia.add(url);
      const img = new Image();
      img.onload = () => {
        if (img.naturalWidth > 0) setLoadedMedia((prev) => (prev.includes(url) ? prev : [...prev, url]));
      };
      img.src = url;
    }
  });

  // Card "ready" gates: the embed cards must show a resolved author byline, so
  // they wait for the author's profile to resolve. We read it from the SAME
  // resolved_profiles projection the avatar/name/mention-chip use (host.profile)
  // rather than the kernel's claimed_events author enrichment — the former is the
  // reliable path; the latter can lag for some authors. Same no-unresolved-data
  // discipline as the user-* sections.
  const authorOf = (ev: ClaimedEventWire) => runtime.host.profile(ev.authorPubkey);
  const authorResolved = (ev: ClaimedEventWire | undefined): ClaimedEventWire | undefined => {
    if (!ev) return undefined;
    const p = authorOf(ev);
    return p && (p.displayName || p.pictureUrl) ? ev : undefined;
  };
  const articleCard = createMemo(() => authorResolved(articleRaw()));
  // The highlight card has no author byline (text + context + source only), so it
  // gates only on the event resolving.
  const highlightCard = createMemo(() => highlightRaw());
  const noteCard = createMemo(() => authorResolved(noteRaw()));

  const profile = () => runtime.host.profile(SHOWCASE_PUBKEY);
  // "Resolved" means real kind:0 data arrived — not just a placeholder entry.
  // Demos render only on real resolution so screenshots never show empty cards.
  const ready = createMemo(() => {
    const p = profile();
    return !!p && (!!p.displayName || !!p.pictureUrl);
  });
  const resolvedProfile = createMemo(() => (ready() ? profile() : undefined));

  return (
    <NostrProfileHostProvider host={runtime.host}>
      <div class="gallery">
        <StatusBar />
        <header class="gallery-header">
          <h1>NMP Component Gallery — Web</h1>
          <p>
            Every component below renders the real <code>{SHOWCASE_PUBKEY.slice(0, 8)}…</code>{" "}
            profile resolved live by the NMP kernel (real WASM, real relays). No mocks, no
            fixtures.
          </p>
        </header>

        <Section
          id="user-avatar"
          title="user-avatar"
          desc="Reference-first avatar — claims its profile, shows the real picture, falls back to a deterministic identicon."
        >
          <Show when={ready()} fallback={<Resolving />} keyed>
            {(_) => (
              <div class="avatar-row" data-testid="avatar-row">
                <NostrAvatar pubkey={SHOWCASE_PUBKEY} size={64} consumerId="demo-avatar-64" />
                <NostrAvatar pubkey={SHOWCASE_PUBKEY} size={48} consumerId="demo-avatar-48" />
                <NostrAvatar pubkey={SHOWCASE_PUBKEY} size={32} consumerId="demo-avatar-32" />
              </div>
            )}
          </Show>
        </Section>

        <Section
          id="user-name"
          title="user-name"
          desc="Display-name text from the resolved kind:0."
        >
          <Show when={resolvedProfile()} fallback={<Resolving />} keyed>
            {(p) => (
              <div data-testid="name-demo" style={{ "font-size": "20px", "font-weight": "600" }}>
                <NostrProfileName profile={p} />
              </div>
            )}
          </Show>
        </Section>

        <Section
          id="user-nip05"
          title="user-nip05"
          desc="NIP-05 verified-identity badge — renders only when the profile carries a nip05."
        >
          <Show when={resolvedProfile()} fallback={<Resolving />} keyed>
            {(p) => (
              <div data-testid="nip05-demo">
                <NostrNip05Badge profile={p} />
              </div>
            )}
          </Show>
        </Section>

        <Section
          id="user-card"
          title="user-card"
          desc="Compact author header: avatar + name + NIP-05 badge."
        >
          <Show when={resolvedProfile()} fallback={<Resolving />} keyed>
            {(p) => (
              <div data-testid="card-demo" style={{ "max-width": "360px" }}>
                <NostrUserCard profile={p} avatarSize={48} />
              </div>
            )}
          </Show>
        </Section>

        <Section
          id="content-view"
          title="content-view"
          desc="Full ContentTreeWire renderer — walks the kernel-parsed NFCT tree (nmp-content behind the content-parser seam) into HTML. Below: a real kind:1 note and a real kind:30023 long-form article, both claimed live and parsed by the kernel."
        >
          <div class="content-demos">
            <div class="content-demo">
              <span class="content-demo-label">kind:1 note</span>
              <Show when={noteEvent()} fallback={<Resolving />} keyed>
                {(ev) => (
                  <div data-testid="content-note" class="nostr-content">
                    <NostrContentView tree={ev.contentTree} fallback={ev.content} />
                  </div>
                )}
              </Show>
            </div>
            <div class="content-demo">
              <span class="content-demo-label">kind:30023 article</span>
              <Show when={articleEvent()} fallback={<Resolving />} keyed>
                {(ev) => (
                  <div data-testid="content-article" class="nostr-content nostr-content--article">
                    <NostrContentView tree={ev.contentTree} fallback={ev.content} />
                  </div>
                )}
              </Show>
            </div>
          </div>
        </Section>

        <Section
          id="content-minimal"
          title="content-minimal"
          desc="Minimal inline content renderer — flattens the kernel-parsed tree to a single flowing line (text + links + hashtags + mentions). The simplest timeline-cell renderer."
        >
          <Show when={noteEvent()} fallback={<Resolving />} keyed>
            {(ev) => (
              <div data-testid="content-minimal" class="nostr-content">
                <NostrMinimalContentView tree={ev.contentTree} fallback={ev.content} />
              </div>
            )}
          </Show>
        </Section>

        <Section
          id="content-mention-chip"
          title="content-mention-chip"
          desc="Inline avatar + display-name chip for a referenced profile (also the embed-profile body). Renders the real resolved kind:0 — avatar picture or deterministic identicon + display name."
        >
          <Show when={resolvedProfile()} fallback={<Resolving />} keyed>
            {(p) => (
              <div data-testid="content-mention-chip">
                <NostrMentionChip profile={p} />
              </div>
            )}
          </Show>
        </Section>

        <Section
          id="content-media-grid"
          title="content-media-grid"
          desc="Adaptive 1–4 image grid. Below: the real images the kernel parsed out of the showcase article (hero + inline figures) — network-loaded, no placeholders."
        >
          <Show when={loadedMedia().length > 0} fallback={<Resolving />} keyed>
            {(_) => (
              <div data-testid="content-media-grid" style={{ "max-width": "520px", width: "100%" }}>
                <NostrMediaGrid urls={loadedMedia().slice(0, 4)} />
              </div>
            )}
          </Show>
        </Section>

        <Section
          id="content-quote-card"
          title="content-quote-card"
          desc="Quoted-note card (the embed-note body) — author header + content preview + relative time. Below: the real showcase kind:1 note resolved via its nevent."
        >
          <Show when={noteCard()} fallback={<Resolving />} keyed>
            {(ev) => (
              <div data-testid="content-quote-card" style={{ "max-width": "480px", width: "100%" }}>
                <NostrQuoteCard
                  quote={{
                    authorName: authorOf(ev)?.displayName,
                    authorPicture: authorOf(ev)?.pictureUrl,
                    content: ev.content,
                    createdAt: ev.createdAt,
                  }}
                  nowSeconds={nowSeconds}
                />
              </div>
            )}
          </Show>
        </Section>

        <Section
          id="embed-article"
          title="embed-article / content-kind-30023"
          desc="NIP-23 long-form article card — hero image, title, summary, author byline. Below: the real showcase kind:30023 article resolved via its naddr."
        >
          <Show when={articleCard()} fallback={<Resolving />} keyed>
            {(ev) => (
              <div data-testid="embed-article" style={{ "max-width": "520px", width: "100%" }}>
                <NostrArticleCard
                  article={{
                    title: tagValue(ev, "title") ?? "(untitled)",
                    image: tagValue(ev, "image"),
                    summary: tagValue(ev, "summary"),
                    authorName: authorOf(ev)?.displayName,
                    authorPicture: authorOf(ev)?.pictureUrl,
                  }}
                />
              </div>
            )}
          </Show>
        </Section>

        <Section
          id="embed-highlight"
          title="embed-highlight / content-kind-9802"
          desc="NIP-84 highlight card — pull-quote + optional context + source footer. Below: the real showcase kind:9802 highlight resolved via its nevent."
        >
          <Show when={highlightCard()} fallback={<Resolving />} keyed>
            {(ev) => (
              <div data-testid="embed-highlight" style={{ "max-width": "520px", width: "100%" }}>
                <NostrHighlightCard
                  highlight={{
                    text: ev.content,
                    context: tagValue(ev, "context"),
                    sourceUrl: tagValue(ev, "r"),
                    sourceEventId: tagValue(ev, "e"),
                    sourceEventAddr: tagValue(ev, "a"),
                  }}
                />
              </div>
            )}
          </Show>
        </Section>

        <Section
          id="content-kind-registry"
          title="content-kind-registry"
          desc="Kind-dispatch registry — routes a resolved event to its per-kind card (kind:30023 → article, kind:9802 → highlight, else → quote). Below: the same three real events dispatched through the registry."
        >
          <div class="content-demos" data-testid="content-kind-registry">
            <Show when={articleCard()} fallback={<Resolving />} keyed>
              {(ev) => <NostrEmbeddedEvent event={toEmbedded(ev, authorOf(ev))} nowSeconds={nowSeconds} />}
            </Show>
            <Show when={highlightCard()} keyed>
              {(ev) => <NostrEmbeddedEvent event={toEmbedded(ev, authorOf(ev))} nowSeconds={nowSeconds} />}
            </Show>
            <Show when={noteCard()} keyed>
              {(ev) => <NostrEmbeddedEvent event={toEmbedded(ev, authorOf(ev))} nowSeconds={nowSeconds} />}
            </Show>
          </div>
        </Section>

        <Section
          id="user-npub"
          title="user-npub"
          desc="Copyable short-npub chip. The npub is encoded by the canonical Rust NIP-19 encoder in the WASM kernel (never bech32-encoded in the browser) — click to copy the full npub."
        >
          <Show when={showcaseNpub()?.npubShort} fallback={<Resolving />} keyed>
            {(short) => (
              <div data-testid="user-npub">
                <NostrNpubChip npub={showcaseNpub()?.npub ?? short} npubShort={short} />
              </div>
            )}
          </Show>
        </Section>

        <Section
          id="relay-list"
          title="relay-list"
          desc="Configured relays with live connection-status dots and role badges — folded from the kernel's relay_statuses."
        >
          <Show when={runtime.relays().length > 0} fallback={<Resolving />} keyed>
            {(_) => (
              <div data-testid="relay-list-demo" style={{ "max-width": "420px", width: "100%" }}>
                <NostrRelayList relays={runtime.relays()} />
              </div>
            )}
          </Show>
        </Section>

        <Section
          id="login-block"
          title="login-block"
          desc="NIP-07 browser-signer detection with a one-click sign-in card and a manual key-entry fallback."
        >
          <div data-testid="login-block-demo" style={{ "max-width": "420px", width: "100%" }}>
            <NostrLoginBlock />
          </div>
        </Section>
      </div>
    </NostrProfileHostProvider>
  );
}

function StatusBar(): JSX.Element {
  return (
    <div class="status-bar" data-testid="status-bar">
      <Pill label="kernel" value={String(typeof runtime.status() === "string" ? runtime.status() : "degraded")} ok={runtime.status() === "running"} />
      <Pill
        label="relays"
        value={`${runtime.relays().filter((r) => r.connection.toLowerCase() === "connected").length}/${runtime.relays().length} connected`}
        ok={runtime.anyRelayConnected()}
      />
      <Pill label="profiles resolved" value={String(runtime.resolvedCount())} ok={runtime.resolvedCount() > 0} />
    </div>
  );
}

function Pill(props: { label: string; value: string; ok: boolean }): JSX.Element {
  return (
    <span class="pill" classList={{ "pill--ok": props.ok }}>
      <span class="pill-label">{props.label}</span>
      <span class="pill-value">{props.value}</span>
    </span>
  );
}

function Section(props: { id: string; title: string; desc: string; children: JSX.Element }): JSX.Element {
  return (
    <section class="component-section" id={props.id}>
      <h2>{props.title}</h2>
      <p class="component-desc">{props.desc}</p>
      <div class="component-stage" data-component={props.title}>
        {props.children}
      </div>
    </section>
  );
}

/**
 * Honesty gate for the content-view showcase. Returns the event only when the
 * kernel has parsed it into a content tree that is genuinely renderable from the
 * tree path — non-empty AND free of `Placeholder` nodes (an unresolved `nostr:`
 * URI becomes a placeholder, which the goal forbids showing). Without a tree the
 * component would silently fall back to the raw string, producing a screenshot
 * that looks fine but proves nothing — so we refuse to render until the tree is
 * real.
 */
function contentReady(ev: ClaimedEventWire | undefined): ClaimedEventWire | undefined {
  const tree = ev?.contentTree;
  if (!ev || !tree || tree.rootsLength() === 0) return undefined;
  for (let i = 0; i < tree.nodesLength(); i += 1) {
    if (tree.nodes(i)?.kind() === WireNodeKind.Placeholder) return undefined;
  }
  return ev;
}

/** Project a resolved claimed event into the generic embed envelope the kind
 *  registry dispatches on. Author name/picture come from the host's resolved
 *  profile (passed in) — the reliable resolved_profiles path. */
function toEmbedded(
  ev: ClaimedEventWire,
  author: { displayName?: string; pictureUrl?: string } | undefined,
): EmbeddedEventModel {
  return {
    kind: ev.kind,
    content: ev.content,
    createdAt: ev.createdAt,
    tags: ev.tags,
    authorName: author?.displayName,
    authorPicture: author?.pictureUrl,
  };
}

/** Collect the real image URLs the kernel parsed into a content tree (Image +
 *  Media nodes). Used to feed the media grid real, network-loaded images. */
function collectMediaUrls(ev: ClaimedEventWire | undefined): string[] {
  const out: string[] = [];
  const tree = ev?.contentTree;
  if (tree) {
    for (let i = 0; i < tree.nodesLength(); i += 1) {
      const node = tree.nodes(i);
      if (!node) continue;
      if (node.kind() === WireNodeKind.Image) {
        const url = node.url();
        if (url) out.push(url);
      } else if (node.kind() === WireNodeKind.Media) {
        for (let m = 0; m < node.mediaUrlsLength(); m += 1) {
          const url = node.mediaUrls(m) as string;
          if (url) out.push(url);
        }
      }
    }
  }
  // The article's hero `image` tag is also a real image — include it first.
  if (ev) {
    const hero = tagValue(ev, "image");
    if (hero && !out.includes(hero)) out.unshift(hero);
  }
  return out;
}

function Resolving(): JSX.Element {
  return (
    <div class="resolving" data-testid="resolving">
      <span class="resolving-dot" /> resolving from relays…
    </div>
  );
}

export { runtime as __galleryRuntime };
