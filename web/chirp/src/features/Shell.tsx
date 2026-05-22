import { For } from "solid-js";
import { Radio } from "lucide-solid";
import { navItems, type ClientView } from "../chirp/model";

export function Shell(props: {
  view: ClientView;
  onView: (view: ClientView) => void;
  children: import("solid-js").JSX.Element;
  aside: import("solid-js").JSX.Element;
}) {
  return (
    <main class="app-shell">
      <aside class="sidebar" aria-label="Chirp navigation">
        <div class="brand">
          <Radio size={24} />
          <span>Chirp</span>
        </div>
        <nav>
          <For each={navItems}>
            {(item) => (
              <button
                class={props.view === item.id ? "active" : ""}
                type="button"
                onClick={() => props.onView(item.id)}
              >
                <item.Icon size={18} />
                {item.label}
              </button>
            )}
          </For>
        </nav>
      </aside>
      <section class="content-panel">{props.children}</section>
      {props.aside}
    </main>
  );
}
