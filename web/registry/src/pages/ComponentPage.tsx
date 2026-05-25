import { For, Show, createMemo } from "solid-js";
import { A, useParams } from "@solidjs/router";
import { findComponent, installCommand, COMPONENTS } from "../registry";
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
    <Show when={component()} fallback={<NotFound />}>
      {(c) => {
        const cmp = c();
        const language: "swift" | "kotlin" =
          cmp.target === "compose" ? "kotlin" : "swift";
        const tabs = cmp.files.map((file) => ({
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
              title={`${cmp.id} — NMP Registry`}
              description={cmp.description}
            />

            <h1>{cmp.slug}</h1>
            <p class="lead">{cmp.description}</p>
            <Show when={cmp.longDescription}>
              <p>{cmp.longDescription}</p>
            </Show>
            <Show when={cmp.inFlight}>
              <div class="callout">
                <strong>In-flight.</strong> This component is described in
                the spec but the source files are still on a feature
                branch. The install command will work once the PR adding it
                merges; until then the code tabs below show placeholders.
              </div>
            </Show>

            <h2 id="install">Install</h2>
            <InstallCommand command={installCommand(cmp)} />

            <h2 id="preview">Preview</h2>
            <Screenshots componentId={cmp.id} variants={cmp.screenshots} />

            <h2 id="files">What you get</h2>
            <p>
              Each tab shows one file the CLI writes into your project.
              File paths are the target locations inside your app.
            </p>
            <Tabs tabs={tabs} ariaLabel={`Files installed by ${cmp.slug}`} />

            <h2 id="dependencies">Dependencies</h2>
            <Show
              when={cmp.dependencies.length > 0}
              fallback={
                <p class="dep-list--empty">No dependencies — install standalone.</p>
              }
            >
              <ul class="dep-list">
                <For each={cmp.dependencies}>
                  {(depId) => {
                    const dep = COMPONENTS.find((d) => d.id === depId);
                    return dep ? (
                      <li>
                        <A href={`/components/${dep.routeId}`}>{dep.id}</A>
                      </li>
                    ) : (
                      <li>
                        <span>{depId}</span>
                      </li>
                    );
                  }}
                </For>
              </ul>
            </Show>

            <h2 id="customization">Customization</h2>
            <For each={cmp.customization}>{(para) => <p>{para}</p>}</For>

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
              lists, edge cases). Run them via the
              {" "}
              <code class="inline-code">build-bundle</code> binary to
              generate the asset bundle the gallery apps consume.
            </p>
          </div>
        );
      }}
    </Show>
  );
}
