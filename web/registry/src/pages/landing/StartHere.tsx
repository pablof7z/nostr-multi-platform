import { A } from "@solidjs/router";
import { JSX } from "solid-js";

type Link = {
  num: string;
  title: string;
  desc: string;
  href: string;
  external?: boolean;
};

const links: Link[] = [
  {
    num: "01",
    title: "Read the doctrine",
    desc: "The eleven principles in full, with the reasoning. Thirty minutes. Read it before you write a line of code against the framework.",
    href: "https://github.com/pablof7z/nostr-multi-platform/blob/master/docs/product-spec/doctrine.md",
    external: true,
  },
  {
    num: "02",
    title: "Browse the registry",
    desc: "UI components for SwiftUI and Compose. Install the ones you need; edit them however you want.",
    href: "/components/content-core",
  },
  {
    num: "03",
    title: "Read the source",
    desc: "The kernel, the FFI, the registry, the apps. Everything is on GitHub.",
    href: "https://github.com/pablof7z/nostr-multi-platform",
    external: true,
  },
];

function StartLink(props: { link: Link }): JSX.Element {
  const inner = (
    <>
      <span class="start-link__num">{props.link.num}</span>
      <span class="start-link__text">
        <div class="start-link__title">{props.link.title}</div>
        <div class="start-link__desc">{props.link.desc}</div>
      </span>
      <span class="start-link__arrow">{props.link.external ? "↗" : "→"}</span>
    </>
  );

  if (props.link.external) {
    return (
      <a
        href={props.link.href}
        class="start-link"
        target="_blank"
        rel="noreferrer noopener"
      >
        {inner}
      </a>
    );
  }
  return (
    <A href={props.link.href} class="start-link">
      {inner}
    </A>
  );
}

export default function StartHere() {
  return (
    <section class="l-section">
      <p class="l-section__label">Getting started</p>
      <h2 class="l-section__heading">Start here</h2>
      <p class="l-section__lead">
        NMP is open source and in active development. The fastest way in is to read the doctrine,
        then build something small.
      </p>
      <div class="start-links">
        {links.map((link) => (
          <StartLink link={link} />
        ))}
      </div>
      <p
        style="margin-top: 2.5rem; font-size: 0.85rem; color: var(--fg-subtle); max-width: 60ch; line-height: 1.65;"
      >
        NMP is built in the open. Issues and pull requests are read by people who care about
        correctness. If you find a bug, the framework has a place where that class of bug can
        never happen again — and we'll put it there.
      </p>
    </section>
  );
}
