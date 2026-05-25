import { For, Show } from "solid-js";
import { A, useLocation } from "@solidjs/router";
import { COMPONENT_GROUPS } from "../registry";

type Props = {
  open: boolean;
};

/**
 * Left-rail catalog. Grouped by target platform; Compose is "Coming soon"
 * and renders no items. Items use SolidJS `<A>` so the URL drives state.
 */
export default function Sidebar(props: Props) {
  const location = useLocation();

  return (
    <nav class="sidebar" data-open={props.open ? "true" : "false"} aria-label="Component catalog">
      <For each={COMPONENT_GROUPS}>
        {(group) => (
          <div class="sidebar__group">
            <h2 class="sidebar__heading">
              {group.label}
              {group.status === "soon" ? (
                <span class="sidebar__badge sidebar__badge--soon" style="margin-left: 0.5rem">
                  Soon
                </span>
              ) : null}
            </h2>
            <Show
              when={group.components.length > 0}
              fallback={
                <p
                  style="font-size: 0.8rem; color: var(--fg-subtle); padding: 0 0.75rem;"
                  role="note"
                >
                  Compose target coming once SwiftUI ships.
                </p>
              }
            >
              <ul class="sidebar__list">
                <For each={group.components}>
                  {(c) => {
                    const href = `/components/${c.routeId}`;
                    return (
                      <li>
                        <A
                          href={href}
                          class="sidebar__link"
                          aria-current={location.pathname === href ? "page" : undefined}
                        >
                          <span class="mono">{c.slug}</span>
                          <Show when={c.inFlight}>
                            <span class="sidebar__badge sidebar__badge--inflight">
                              in-flight
                            </span>
                          </Show>
                        </A>
                      </li>
                    );
                  }}
                </For>
              </ul>
            </Show>
          </div>
        )}
      </For>
    </nav>
  );
}
