import { A } from "@solidjs/router";

const cats = [
  {
    href: "/components/content-core",
    name: "Content",
    desc: "Rendering Nostr notes, markdown, embedded media, quoted notes, link previews. Everything between kind:1 and a finished feed cell.",
  },
  {
    href: "/components/user-core",
    name: "User",
    desc: "Profiles, avatars, display names, follow buttons, follow lists. Anything keyed by pubkey.",
  },
  {
    href: "/components/relay-list",
    name: "Relay",
    desc: "Status indicators, health badges, connection state. The UI for the relays you didn't have to manage.",
  },
];

export default function RegistrySection() {
  return (
    <section class="l-section">
      <p class="l-section__label">Components</p>
      <h2 class="l-section__heading">A registry of components, not a package of components</h2>
      <p class="l-section__lead">
        NMP ships UI components for SwiftUI and Compose. You don't install them as a package.
        You install them as files — the source code lands in your repository, and it's yours to
        edit. When upstream improves a component, a three-way merge brings the diff in without
        touching the edits you made.
      </p>
      <p class="l-section__lead" style="margin-top: 1rem;">
        Every component consumes the same framework projection —{" "}
        <code class="inline-code">ContentTreeWire</code> from{" "}
        <code class="inline-code">nmp-content</code> — so the wire shape stays stable across the
        registry. The rendering is yours. The parsing is ours.
      </p>
      <div class="registry-cats">
        {cats.map((c) => (
          <A href={c.href} class="registry-cat">
            <div class="registry-cat__name">{c.name}</div>
            <div class="registry-cat__desc">{c.desc}</div>
          </A>
        ))}
      </div>
    </section>
  );
}
