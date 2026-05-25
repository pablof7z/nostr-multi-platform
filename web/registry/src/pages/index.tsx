import { A } from "@solidjs/router";
import PageMeta from "../components/PageMeta";

export default function Landing() {
  return (
    <div class="content">
      <PageMeta
        title="NMP Registry — Native Nostr UI components"
        description="Native Nostr UI components for SwiftUI and Compose. Copy-paste components you own, customize freely, update without losing edits."
      />

      <section class="hero">
        <h1 class="hero__title">Native Nostr UI components for SwiftUI and Compose.</h1>
        <p class="hero__subtitle">
          Copy-paste components you own, customize freely, update without
          losing edits.
        </p>
        <div class="hero__cta">
          <A href="/components/content-core" class="btn btn--primary">
            Browse Components
          </A>
          <A href="/get-started" class="btn">
            Get Started
          </A>
        </div>
      </section>

      <section aria-labelledby="how-heading">
        <h2 id="how-heading">How it works</h2>
        <p>
          The NMP registry behaves like shadcn/ui — you don't install a
          package, you install <em>files</em>. They live in your repo, ship
          with your binary, and stay yours to edit. When upstream improves a
          component, a structural three-way merge brings the diff into your
          tree without trampling your edits.
        </p>
        <ol class="steps" style="list-style: none; padding: 0;">
          <li class="step">
            <div class="step__num">01</div>
            <div class="step__title">Install</div>
            <div>Pull the component into your app's source tree.</div>
            <code class="step__cmd">nmp add component swiftui/content-view</code>
          </li>
          <li class="step">
            <div class="step__num">02</div>
            <div class="step__title">Edit freely</div>
            <div>
              Change colors, swap layouts, add new cases — you own the file
              now.
            </div>
            <code class="step__cmd">vim Components/NostrContent/...</code>
          </li>
          <li class="step">
            <div class="step__num">03</div>
            <div class="step__title">Update safely</div>
            <div>
              A three-way merge brings upstream improvements in without
              losing your edits.
            </div>
            <code class="step__cmd">nmp update component swiftui/content-view</code>
          </li>
        </ol>
      </section>

      <hr class="section-divider" />

      <section aria-labelledby="why-heading">
        <h2 id="why-heading">Why install files instead of a package?</h2>
        <p>
          Nostr clients tend to be opinionated about how a note looks. The
          same renderer never looks right across two apps. A package locks
          you out of changes you'd otherwise make in five minutes; a copy of
          the file gets out of your way. The registry just keeps the copies
          honest.
        </p>
        <p>
          Components consume <code class="inline-code">ContentTreeWire</code>{" "}
          from the <code class="inline-code">nmp-content</code> crate (the
          framework's parsed-content projection). The wire shape is stable;
          the rendering is yours.
        </p>
      </section>
    </div>
  );
}
