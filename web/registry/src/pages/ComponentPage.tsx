import { For, Show, createMemo, createSignal } from "solid-js";
import { A, useParams } from "@solidjs/router";
import {
  COMPONENTS,
  PLATFORM_LABELS,
  PLATFORM_ORDER,
  findComponent,
  installCommand,
  type Platform,
} from "../registry";
import PageMeta from "../components/PageMeta";
import InstallCommand from "../components/InstallCommand";
import Screenshots from "../components/Screenshots";
import Tabs from "../components/Tabs";
import CodeBlock from "../components/CodeBlock";
import NotFound from "./not-found";

export default function ComponentPage() {
  const params = useParams<{ id: string }>();
  const component = createMemo(() => findComponent(params.id));
  const [selectedPlatform, setSelectedPlatform] =
    createSignal<Platform>("swiftui");

  const availablePlatforms = createMemo(() =>
    PLATFORM_ORDER.filter((platform) => component()?.platforms[platform])
  );
  const activePlatform = createMemo(
    () => {
      const available = availablePlatforms();
      return available.includes(selectedPlatform())
        ? selectedPlatform()
        : available[0];
    }
  );
  const activeImpl = createMemo(() => {
    const platform = activePlatform();
    return platform ? component()?.platforms[platform] : undefined;
  });

  return (
    <Show when={component()} fallback={<NotFound />}>
      {(c) => (
        <Show when={activeImpl()} fallback={<NotFound />}>
          {(implAccessor) => {
            const cmp = c();
            const impl = implAccessor();
            const platform = activePlatform() ?? "swiftui";
            const language =
              platform === "compose" ? "kotlin" : platform === "swiftui" ? "swift" : "text";
            const tabs = impl.files.map((file) => ({
              id: file.target,
              label: file.target.split("/").pop() ?? file.target,
              panel: (
                <CodeBlock
                  source={file.content}
                  filePath={file.target}
                  language={language}
                />
              ),
            }));

            return (
              <div class="content">
                <PageMeta
                  title={`${impl.installId} — NMP Registry`}
                  description={cmp.description}
                />

                <h1>{cmp.slug}</h1>
                <p class="lead">{cmp.description}</p>
                <Show when={impl.longDescription}>
                  <p>{impl.longDescription}</p>
                </Show>

                <Show when={availablePlatforms().length > 1}>
                  <div class="platform-switcher" role="group" aria-label="Component platform">
                    <For each={availablePlatforms()}>
                      {(candidate) => {
                        const candidateImpl = cmp.platforms[candidate];
                        return (
                          <button
                            type="button"
                            class="platform-switcher__button"
                            data-active={platform === candidate ? "true" : "false"}
                            onClick={() => setSelectedPlatform(candidate)}
                          >
                            <span>{PLATFORM_LABELS[candidate]}</span>
                            <small>{candidateImpl?.version}</small>
                          </button>
                        );
                      }}
                    </For>
                  </div>
                </Show>

                <Show when={impl.status === "soon"}>
                  <div class="callout">
                    <strong>In-flight.</strong> This platform implementation is
                    described in the spec, but the source files are not ready
                    to install yet.
                  </div>
                </Show>

                <h2 id="install">Install</h2>
                <InstallCommand command={installCommand(impl.installId)} />

                <h2 id="preview">Preview</h2>
                <Screenshots componentId={impl.installId} variants={impl.screenshots} />

                <h2 id="files">What you get</h2>
                <p>
                  Each tab shows one file the CLI writes into your project.
                  File paths are the target locations inside your app.
                </p>
                <Tabs tabs={tabs} ariaLabel={`Files installed by ${impl.installId}`} />

                <h2 id="dependencies">Dependencies</h2>
                <Show
                  when={impl.dependencies.length > 0}
                  fallback={
                    <p class="dep-list--empty">No dependencies — install standalone.</p>
                  }
                >
                  <ul class="dep-list">
                    <For each={impl.dependencies}>
                      {(depSlug) => {
                        const dep = COMPONENTS.find((d) => d.slug === depSlug);
                        const depImpl = dep?.platforms[platform];
                        return dep ? (
                          <li>
                            <A href={`/components/${dep.routeId}`}>
                              {depImpl?.installId ?? dep.slug}
                            </A>
                          </li>
                        ) : (
                          <li>
                            <span>{depSlug}</span>
                          </li>
                        );
                      }}
                    </For>
                  </ul>
                </Show>

                <h2 id="customization">Customization</h2>
                <For each={impl.customization}>{(para) => <p>{para}</p>}</For>

                <h2 id="wire">Wire contract</h2>
                <p>
                  Content components consume{" "}
                  <code class="inline-code">ContentTreeWire</code> — the parsed
                  content projection produced by the{" "}
                  <code class="inline-code">nmp-content</code> crate. The wire
                  shape is stable across the framework; what you change in your
                  app is the rendering.
                </p>
                <p>
                  Reference fixtures live in the workspace at{" "}
                  <code class="inline-code">crates/nmp-content-fixtures/src/scenarios/</code>{" "}
                  — one file per scenario family (text, mentions, quotes,
                  lists, edge cases). Run them via{" "}
                  <code class="inline-code">build-bundle</code> to generate the
                  asset bundle the gallery apps consume.
                </p>
              </div>
            );
          }}
        </Show>
      )}
    </Show>
  );
}
