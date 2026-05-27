import { A } from "@solidjs/router";

export default function Hero() {
  return (
    <section class="hero">
      <p class="hero__eyebrow">Nostr Multi-Platform</p>
      <h1 class="hero__title">One Rust core. Four native shells. Zero protocol bugs.</h1>
      <p class="hero__sub">
        Build Nostr applications that ship on iOS, Android, desktop, and web from a single
        codebase — without inheriting the dozen correctness problems every other client ships
        broken.
      </p>
      <p class="hero__sub">
        Stale replaceable events, lost subscriptions, mis-routed posts, multi-account desync,
        leaked signing operations, races between local and relay state. NMP makes these{" "}
        <strong>structurally impossible</strong> — ruled out by the type system and actor
        model, not documented as footguns in a README.
      </p>
      <div class="hero__cta">
        <A href="/components/content-core" class="btn btn--primary">
          Browse the registry
        </A>
        <A href="/get-started" class="btn">
          Get started
        </A>
        <a
          href="https://github.com/pablof7z/nostr-multi-platform"
          target="_blank"
          rel="noreferrer noopener"
          class="btn"
        >
          GitHub
        </a>
      </div>
    </section>
  );
}
