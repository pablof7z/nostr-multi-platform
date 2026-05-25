import { For } from "solid-js";
import { A, useLocation } from "@solidjs/router";
import { SECTIONS } from "../registry";

type Props = {
  open: boolean;
};

export default function Sidebar(props: Props) {
  const location = useLocation();

  return (
    <nav
      class="sidebar"
      data-open={props.open ? "true" : "false"}
      aria-label="Component catalog"
    >
      <For each={SECTIONS}>
        {(section) => (
          <div class="sidebar__group">
            <h2 class="sidebar__heading">{section.label}</h2>
            <ul class="sidebar__list">
              <For each={section.components}>
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
                      </A>
                    </li>
                  );
                }}
              </For>
            </ul>
          </div>
        )}
      </For>
    </nav>
  );
}
