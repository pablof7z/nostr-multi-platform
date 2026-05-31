import { For, Show, createMemo, createSignal } from "solid-js";
import { A, useParams } from "@solidjs/router";
import {
  findComponent,
  installCommand,
  COMPONENTS,
  PLATFORM_ORDER,
  PLATFORM_LABELS,
  Platform,
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

  return (
    <Show when={component()} fallback={<NotFound />} keyed>
      {(cmp) => {

        const defaultPlatform: Platform =
          PLATFORM_ORDER.find((p) => cmp.platforms[p]?.status === "stable") ?? "swiftui";

        const [platform, setPlatform] = createSignal<Platform>(defaultPlatform);

        const impl = createMemo(() => cmp.platforms[platform()]);

        const language = createMemo<"swift" | "kotlin" | "rust">(() =>
          platform() === "compose" ? "kotlin" : platform() === "tui" ? "rust" : "swift"
        );

        const tabs = createMemo(() => {
          const i = impl();
          if (!i) return [];
          return i.files.map((file) => ({
            id: file.target,
            label: file.target.split("/").pop() ?? file.target,
            panel: (
              <CodeBlock
                source={file.content}
                filePath={file.target}
                language={language()}
              />
            ),
          }));
        });

        return (
          <div class="content">
            <PageMeta
              title={`${cmp.slug} — NMP Registry`}
              description={cmp.description}
            />

            <h1>{cmp.slug}</h1>
            <p class="lead">{cmp.description}</p>

            {/* Platform switcher */}
            <div class="platform-bar" role="tablist" aria-label="Target platform">
              <For each={PLATFORM_ORDER}>
                {(p) => {
                  const pi = cmp.platforms[p];
                  const available = !!pi && pi.status === "stable";
                  return (
                    <button
                      role="tab"
                      type="button"
                      aria-selected={platform() === p}
                      disabled={!available}
                      class="platform-tab"
                      classList={{
                        "platform-tab--active": platform() === p,
                        "platform-tab--soon": !available,
                      }}
                      onClick={() => available && setPlatform(p)}
                    >
                      {PLATFORM_LABELS[p]}
                      {!available && <span class="platform-tab__badge">soon</span>}
                    </button>
                  );
                }}
              </For>
            </div>

            <Show when={impl()?.longDescription}>
              <p>{impl()!.longDescription}</p>
            </Show>

            <h2 id="install">Install</h2>
            <Show
              when={impl()}
              fallback={<p class="soon-note">Not yet available for this platform.</p>}
            >
              {(i) => <InstallCommand command={installCommand(i().installId)} />}
            </Show>

            <h2 id="preview">Preview</h2>
            <Screenshots
              componentId={cmp.slug}
              variants={impl()?.screenshots ?? []}
              platform={platform()}
            />

            <h2 id="files">What you get</h2>
            <p>
              Each tab shows one file the CLI writes into your project. File
              paths are the target locations inside your app.
            </p>
            <Show
              when={tabs().length > 0}
              fallback={<p class="soon-note">No files yet for this platform.</p>}
            >
              <Tabs tabs={tabs()} ariaLabel={`Files installed by ${cmp.slug}`} />
            </Show>

            <h2 id="dependencies">Dependencies</h2>
            <Show
              when={(impl()?.dependencies ?? []).length > 0}
              fallback={
                <p class="dep-list--empty">No dependencies — install standalone.</p>
              }
            >
              <ul class="dep-list">
                <For each={impl()?.dependencies ?? []}>
                  {(depSlug) => {
                    const dep = COMPONENTS.find((d) => d.slug === depSlug);
                    return dep ? (
                      <li>
                        <A href={`/components/${dep.routeId}`}>{depSlug}</A>
                      </li>
                    ) : (
                      <li><span>{depSlug}</span></li>
                    );
                  }}
                </For>
              </ul>
            </Show>

            <h2 id="customization">Customization</h2>
            <For each={impl()?.customization ?? []}>
              {(para) => <p>{para}</p>}
            </For>

            <h2 id="wire">Wire contract</h2>
            <Show
              when={cmp.slug.startsWith("user-")}
              fallback={
                <>
                  <p>
                    Content components consume{" "}
                    <code class="inline-code">ContentTreeWire</code> — the parsed
                    content projection produced by the{" "}
                    <code class="inline-code">nmp-content</code> crate. The wire
                    shape is stable across the framework; what you change in your
                    app is the rendering.
                  </p>
                  <p>
                    Reference fixtures live at{" "}
                    <code class="inline-code">
                      crates/nmp-content-fixtures/src/scenarios/
                    </code>{" "}
                    — one file per scenario family. Run them via the{" "}
                    <code class="inline-code">build-bundle</code> binary to
                    generate the asset bundle the gallery apps consume.
                  </p>
                </>
              }
            >
              <p>
                User components consume <code class="inline-code">ProfileWire</code>:
                a Rust-owned projection containing the raw pubkey plus
                Rust-formatted display strings such as{" "}
                <code class="inline-code">npubShort</code>. Apps own fetching and
                persistence; these files only render the profile snapshot they are
                given.
              </p>
            </Show>
          </div>
        );
      }}
    </Show>
  );
}
