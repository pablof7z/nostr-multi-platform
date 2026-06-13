import { Show, createEffect, createMemo, type JSX } from "solid-js";
import { NostrProfileHostProvider } from "./components/user-avatar/NostrProfileHost";
import { NostrAvatar } from "./components/user-avatar/NostrAvatar";
import { NostrProfileName } from "./components/user-name/NostrProfileName";
import { NostrNip05Badge } from "./components/user-nip05/NostrNip05Badge";
import { NostrUserCard } from "./components/user-card/NostrUserCard";
import { createGalleryRuntime } from "./nmp/profileHost";
import { SHOWCASE_PUBKEY, SHOWCASE_RELAYS } from "./showcase";

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
    if (runtime.anyRelayConnected() && !claimed) {
      claimed = true;
      runtime.host.claimProfile(SHOWCASE_PUBKEY, "gallery-root");
    }
  });

  const profile = () => runtime.host.profile(SHOWCASE_PUBKEY);
  const ready = createMemo(() => profile() !== undefined);

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
          <Show when={profile()} fallback={<Resolving />} keyed>
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
          <Show when={profile()} fallback={<Resolving />} keyed>
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
          <Show when={profile()} fallback={<Resolving />} keyed>
            {(p) => (
              <div data-testid="card-demo" style={{ "max-width": "360px" }}>
                <NostrUserCard profile={p} avatarSize={48} />
              </div>
            )}
          </Show>
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

function Resolving(): JSX.Element {
  return (
    <div class="resolving" data-testid="resolving">
      <span class="resolving-dot" /> resolving from relays…
    </div>
  );
}

export { runtime as __galleryRuntime };
