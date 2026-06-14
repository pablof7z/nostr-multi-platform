import { Show, createEffect, createMemo, type JSX } from "solid-js";
import { NostrProfileHostProvider } from "./components/user-avatar/NostrProfileHost";
import { NostrAvatar } from "./components/user-avatar/NostrAvatar";
import { NostrProfileName } from "./components/user-name/NostrProfileName";
import { NostrNip05Badge } from "./components/user-nip05/NostrNip05Badge";
import { NostrUserCard } from "./components/user-card/NostrUserCard";
import { NostrRelayList } from "./components/relay-list/NostrRelayList";
import { NostrLoginBlock } from "./components/login-block/NostrLoginBlock";
import { NostrContentView } from "./components/content-view/NostrContentView";
import { createGalleryRuntime, type ClaimedEventWire } from "./nmp/profileHost";
import { SHOWCASE_PUBKEY, SHOWCASE_RELAYS, SHOWCASE_NOTE, SHOWCASE_ARTICLE } from "./showcase";
import { WireNodeKind } from "./nmp/generated/nmp/content/wire-node-kind";

const runtime = createGalleryRuntime();

// Start the kernel immediately. Relays connect asynchronously after Start.
void runtime.start(SHOWCASE_RELAYS);

export default function App(): JSX.Element {
  // Claim the showcase identity only once a relay is actually connected. The
  // kernel's claim send-gate (relays_ready) parks a claim issued before any
  // relay is up, so claiming at mount would never send the kind:0 REQ. Claiming
  // on the connected edge is exactly how Chirp resolves profiles (claims fire
  // as feed cards mount, after the feed opened post-connect).
  let claimed = false;
  createEffect(() => {
    if (runtime.anyIndexerConnected() && !claimed) {
      claimed = true;
      runtime.host.claimProfile(SHOWCASE_PUBKEY, "gallery-root");
    }
  });

  // Claim the showcase events for the content-view component. Event-id fetches
  // route through the content lane, so gate on a CONTENT relay being connected —
  // claiming before the content socket is open drops the REQ (the wasm transport
  // has no on-demand dial or retry). Same edge-trigger discipline as the
  // indexer-gated profile claim above.
  let eventsClaimed = false;
  createEffect(() => {
    if (runtime.anyContentConnected() && !eventsClaimed) {
      eventsClaimed = true;
      runtime.claimEvent(SHOWCASE_NOTE.uri, "gallery-note");
      runtime.claimEvent(SHOWCASE_ARTICLE.uri, "gallery-article");
    }
  });

  // A claimed event is "render-ready" only once the kernel has parsed it into a
  // non-empty, placeholder-free NFCT content tree. This is the honesty gate: it
  // guarantees the screenshot shows tree-derived rendering, never the raw-string
  // fallback (which would look identical but mean the parse path never ran).
  const noteEvent = createMemo(() => contentReady(runtime.claimedEvent(SHOWCASE_NOTE.primaryId)));
  const articleEvent = createMemo(() =>
    contentReady(runtime.claimedEvent(SHOWCASE_ARTICLE.primaryId)),
  );

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

function Resolving(): JSX.Element {
  return (
    <div class="resolving" data-testid="resolving">
      <span class="resolving-dot" /> resolving from relays…
    </div>
  );
}

export { runtime as __galleryRuntime };
