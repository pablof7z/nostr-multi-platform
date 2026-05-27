import { A } from "@solidjs/router";
import PageMeta from "../components/PageMeta";
import InstallCommand from "../components/InstallCommand";

export default function GetStarted() {
  return (
    <div class="content">
      <PageMeta
        title="Get started — NMP Registry"
        description="Install the NMP CLI, scaffold a new app, install a content kit, and learn how nmp update component preserves local edits."
      />

      <h1>Get started</h1>
      <p class="lead">
        Five minutes from "I want a Nostr renderer" to a working SwiftUI
        timeline cell you fully own.
      </p>

      <h2>1. Install the CLI</h2>
      <p>
        The NMP CLI lives in the workspace. From a clone of the{" "}
        <a
          href="https://github.com/pablof7z/nostr-multi-platform"
          target="_blank"
          rel="noreferrer noopener"
        >
          nostr-multi-platform
        </a>{" "}
        repo:
      </p>
      <InstallCommand command="cargo install --path crates/nmp-cli" />

      <h2>2. Scaffold a new app</h2>
      <p>
        Create a workspace with the framework's app template wired up:
      </p>
      <InstallCommand command="nmp init my-app" />
      <p>
        This drops a <code class="inline-code">Cargo.toml</code>, a{" "}
        <code class="inline-code">nmp.toml</code> manifest, and a placeholder
        app crate wired to the framework's actor, event store, and routing substrate.
      </p>

      <h2>3. Install a content kit</h2>
      <p>
        Pick a component — the easiest start is the minimal renderer:
      </p>
      <InstallCommand command="nmp add component swiftui/content-minimal" />
      <p>
        The CLI resolves dependencies automatically (so this also installs{" "}
        <A href="/components/content-core">
          <code class="inline-code">swiftui/content-core</code>
        </A>
        ) and copies each file into the target paths declared in the
        registry manifest.
      </p>

      <h2>4. Customize</h2>
      <p>
        Open the installed Swift file in your editor. Change the colors,
        swap the layout, add new <code class="inline-code">NostrContentRun.Kind</code>{" "}
        cases — whatever you want. There is no package to vendor, no abstraction
        barrier. The file is yours.
      </p>
      <div class="callout">
        <strong>Tip.</strong> Inject your branded renderer at the app root
        with <code class="inline-code">.nostrContentRenderer(...)</code>{" "}
        rather than editing the component directly. The component reads it
        through the environment, and your tweaks stay isolated from upstream
        updates.
      </div>

      <h2>5. Update safely</h2>
      <p>
        When the registry pushes a new version of a component, run:
      </p>
      <InstallCommand command="nmp update component swiftui/content-view" />
      <p>
        Files you haven't touched are updated silently. Files with local
        edits are compared against their install-time SHA-256 baseline. If
        the file changed upstream and you changed it locally, the CLI reports{" "}
        <code class="inline-code">conflict: path — local edits preserved</code>{" "}
        and skips that file so you can merge the upstream changes manually.
      </p>

      <hr class="section-divider" />

      <p>
        Ready to pick a component? Head to{" "}
        <A href="/components/content-core">the catalog</A>.
      </p>
    </div>
  );
}
