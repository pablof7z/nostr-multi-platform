import { For, JSX, createSignal } from "solid-js";

export type Tab = {
  id: string;
  label: string;
  panel: JSX.Element;
};

type Props = {
  tabs: Tab[];
  initial?: string;
  ariaLabel?: string;
};

/**
 * Accessible tab group — arrow-key navigation between tab buttons,
 * roving tabindex, ARIA roles per WAI-ARIA Authoring Practices.
 */
export default function Tabs(props: Props) {
  const [active, setActive] = createSignal(props.initial ?? props.tabs[0]?.id);

  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const ids = props.tabs.map((t) => t.id);
    const idx = ids.indexOf(active());
    if (idx === -1) return;
    e.preventDefault();
    const next =
      e.key === "ArrowRight"
        ? ids[(idx + 1) % ids.length]
        : ids[(idx - 1 + ids.length) % ids.length];
    setActive(next);
    // Focus the new tab button so screen readers track keyboard focus.
    const button = document.getElementById(`tab-${next}`);
    button?.focus();
  };

  return (
    <div class="tabs">
      <div
        class="tabs__list"
        role="tablist"
        aria-label={props.ariaLabel ?? "File tabs"}
        onKeyDown={onKeyDown}
      >
        <For each={props.tabs}>
          {(tab) => (
            <button
              type="button"
              id={`tab-${tab.id}`}
              class="tabs__tab"
              role="tab"
              aria-selected={tab.id === active()}
              aria-controls={`panel-${tab.id}`}
              tabindex={tab.id === active() ? 0 : -1}
              onClick={() => setActive(tab.id)}
            >
              {tab.label}
            </button>
          )}
        </For>
      </div>
      <For each={props.tabs}>
        {(tab) => (
          <div
            id={`panel-${tab.id}`}
            class="tabs__panel"
            role="tabpanel"
            aria-labelledby={`tab-${tab.id}`}
            data-active={tab.id === active()}
          >
            {tab.panel}
          </div>
        )}
      </For>
    </div>
  );
}
