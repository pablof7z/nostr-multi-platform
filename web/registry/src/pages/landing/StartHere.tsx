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
    title: "Browse the registry",
    desc: "UI components for SwiftUI and Compose. Install the ones you need; edit them however you want. This is where most developers start.",
    href: "/components/content-core",
  },
  {
    num: "02",
    title: "Scaffold an app",
    desc: "Five minutes from clone to a buildable Nostr app. The CLI generates the Rust core, the FFI layer, and a SwiftUI or Compose shell.",
    href: "/get-started",
  },
  {
    num: "03",
    title: "Read the doctrine",
    desc: "The eleven principles in full. Worth reading once you've built something and want to understand why the framework works the way it does.",
    href: "https://github.com/pablof7z/nostr-multi-platform/blob/master/docs/product-spec/doctrine.md",
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
        NMP is open source and in active development. Start with the registry — you can use
        components without touching the kernel at all.
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
