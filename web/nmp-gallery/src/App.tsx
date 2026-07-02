import { Show, createEffect, createMemo, createSignal, type JSX } from "solid-js";
import {
  NmpComponentHostProvider,
  NostrArticleCard,
  NostrAvatar,
  NostrContentView,
  NostrEmbeddedEvent,
  NostrHighlightCard,
  NostrLoginBlock,
  NostrMediaGrid,
  NostrMentionChip,
  NostrMinimalContentView,
  NostrNip05Badge,
  NostrNpubChip,
  NostrProfileName,
  NostrQuoteCard,
  NostrRelayList,
  NostrUserCard,
} from "@nmpis/components-web";
import {
  Resolving,
  Section,
  StatusBar,
  articleProjectionOf,
  bylineOf,
  collectMediaUrls,
  contentReady,
  highlightProjectionOf,
} from "./gallerySupport";
import { createGalleryRuntime, type ClaimedEventWire } from "./nmp/profileHost";
import {
  SHOWCASE_PUBKEY,
  SHOWCASE_RELAYS,
  SHOWCASE_NOTE,
  SHOWCASE_ARTICLE,
  SHOWCASE_HIGHLIGHT,
} from "./showcase";

const runtime = createGalleryRuntime();

// Start the kernel immediately. Relays connect asynchronously after Start.
void runtime.start(SHOWCASE_RELAYS);

export default function App(): JSX.Element {
  // Claim the showcase profile once an indexer relay is connected. The runtime
  // owns socket buffering, lifecycle drains, reconnect replay, and snapshots;
  // the gallery owns only the demand edge.
  let claimed = false;
  createEffect(() => {
    if (runtime.anyIndexerConnected() && !claimed) {
      claimed = true;
      runtime.host.resolveProfileRef(SHOWCASE_PUBKEY, "gallery-root");
    }
  });

  // Claim each resolved event's author so the embed cards show a real byline.
  // One claim per author; the kernel's deferred-reconnect rebatch handles relays.
  const claimedAuthors = new Set<string>();
  createEffect(() => {
    for (const ev of [noteRaw(), articleRaw(), highlightRaw()]) {
      const pk = ev?.authorPubkey;
      if (pk && !claimedAuthors.has(pk)) {
        claimedAuthors.add(pk);
        runtime.host.resolveProfileRef(pk, `embed-author-${pk.slice(0, 8)}`);
      }
    }
  });

  // Claim the showcase events once for the content-view component. Event-id
  // fetches route through the content lane, so gate on a CONTENT relay being
  // connected; the runtime/transport owns the request mechanics after that.
  let claimStarted = false;
  const claimTargets = [
    { id: SHOWCASE_NOTE.primaryId, hints: SHOWCASE_NOTE.relayHints, consumer: "gallery-note" },
    { id: SHOWCASE_ARTICLE.primaryId, hints: SHOWCASE_ARTICLE.relayHints, consumer: "gallery-article" },
    { id: SHOWCASE_HIGHLIGHT.primaryId, hints: SHOWCASE_HIGHLIGHT.relayHints, consumer: "gallery-highlight" },
  ];
  createEffect(() => {
    if (!runtime.anyContentConnected() || claimStarted) return;
    claimStarted = true;
    for (const t of claimTargets) {
      runtime.claimEvent(t.id, t.consumer, t.hints);
    }
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
  // `refs.profile` row cache the avatar/name/mention-chip use (host.profile)
  // rather than any event-row author enrichment — the former is
  // reliable; the latter can lag. Same no-unresolved-data discipline as user-*.
  const authorOf = (ev: ClaimedEventWire | undefined) =>
    ev ? runtime.host.profile(ev.authorPubkey) : undefined;
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

  // The render-facing embed envelopes are derived from the authoritative
  // `refs.event` row cache. Each gates on its per-card memo so it appears in
  // lockstep with the per-component demos above (article/note also wait for
  // the author profile).
  const articleEmbed = createMemo(() =>
    articleCard() ? runtime.refEventEnvelope(SHOWCASE_ARTICLE.primaryId) : undefined,
  );
  const highlightEmbed = createMemo(() =>
    highlightCard() ? runtime.refEventEnvelope(SHOWCASE_HIGHLIGHT.primaryId) : undefined,
  );
  const noteEmbed = createMemo(() =>
    noteCard() ? runtime.refEventEnvelope(SHOWCASE_NOTE.primaryId) : undefined,
  );

  // The standalone embed-article / embed-highlight showcase sections render the
  // SAME card components, sourcing their fields from the envelope derived from
  // `refs.event`. The byline still comes from host-resolved `authorOf(...)`
  // (the projection's static author is null by design). Narrowing helpers are
  // pure (gallerySupport); reactivity stays here in the memo.
  const articleProjection = createMemo(() => articleProjectionOf(articleEmbed()));
  const highlightProjection = createMemo(() => highlightProjectionOf(highlightEmbed()));

  const profile = () => runtime.host.profile(SHOWCASE_PUBKEY);
  // "Resolved" means real kind:0 data arrived — not just a placeholder entry.
  // Demos render only on real resolution so screenshots never show empty cards.
  const ready = createMemo(() => {
    const p = profile();
    return !!p && (!!p.displayName || !!p.pictureUrl);
  });
  const resolvedProfile = createMemo(() => (ready() ? profile() : undefined));

  return (
    <NmpComponentHostProvider profileHost={runtime.host} resolvedEventEmbeds={runtime.refEventEmbeds} eventRefResolver={runtime.eventRefResolver}>
      <div class="gallery">
        <StatusBar
          status={runtime.status()}
          relays={runtime.relays()}
          anyRelayConnected={runtime.anyRelayConnected()}
          resolvedCount={runtime.resolvedCount()}
        />
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
                    <NostrContentView tree={ev.contentTree} fallback={ev.content} nowSeconds={nowSeconds} />
                  </div>
                )}
              </Show>
            </div>
            <div class="content-demo">
              <span class="content-demo-label">kind:30023 article</span>
              <Show when={articleEvent()} fallback={<Resolving />} keyed>
                {(ev) => (
                  <div data-testid="content-article" class="nostr-content nostr-content--article">
                    <NostrContentView tree={ev.contentTree} fallback={ev.content} nowSeconds={nowSeconds} />
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
          <Show when={articleProjection()} fallback={<Resolving />} keyed>
            {(p) => (
              <div data-testid="embed-article" style={{ "max-width": "520px", width: "100%" }}>
                <NostrArticleCard
                  article={{
                    title: p.title ?? "(untitled)",
                    image: p.heroImageUrl ?? undefined,
                    summary: p.summary ?? undefined,
                    authorName: authorOf(articleRaw())?.displayName,
                    authorPicture: authorOf(articleRaw())?.pictureUrl,
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
          <Show when={highlightProjection()} fallback={<Resolving />} keyed>
            {(p) => (
              <div data-testid="embed-highlight" style={{ "max-width": "520px", width: "100%" }}>
                <NostrHighlightCard
                  highlight={{
                    text: p.highlightedText,
                    context: p.context ?? undefined,
                    sourceUrl: p.sourceUrl ?? undefined,
                    sourceEventId: p.sourceEventId ?? undefined,
                    sourceEventAddr: p.sourceEventAddr ?? undefined,
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
            <Show when={articleEmbed()} fallback={<Resolving />} keyed>
              {(embed) => (
                <NostrEmbeddedEvent
                  event={embed}
                  nowSeconds={nowSeconds}
                  author={bylineOf(authorOf(articleRaw()))}
                />
              )}
            </Show>
            <Show when={highlightEmbed()} keyed>
              {(embed) => <NostrEmbeddedEvent event={embed} nowSeconds={nowSeconds} />}
            </Show>
            <Show when={noteEmbed()} keyed>
              {(embed) => (
                <NostrEmbeddedEvent
                  event={embed}
                  nowSeconds={nowSeconds}
                  author={bylineOf(authorOf(noteRaw()))}
                />
              )}
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
    </NmpComponentHostProvider>
  );
}

export { runtime as __galleryRuntime };
