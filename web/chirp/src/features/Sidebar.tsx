import { House, Radio, Settings } from "lucide-solid";
import type { FeatureSnapshot } from "../nmp/snapshot";

export type AppTab = "home" | "settings";

const tabs = [
  { id: "home", label: "Home", icon: House },
  { id: "settings", label: "Settings", icon: Settings },
] as const;

export function Sidebar(props: {
  active: AppTab;
  feature: FeatureSnapshot;
  onSelect: (tab: AppTab) => void;
}) {
  return (
    <aside class="sidebar" aria-label="Chirp navigation">
      <div class="brand">
        <Radio size={24} />
        <span>Chirp</span>
      </div>
      <nav>
        {tabs.map((tab) => {
          const Icon = tab.icon;
          const value = badge(tab.id, props.feature);
          return (
            <button type="button" class={props.active === tab.id ? "active" : ""} onClick={() => props.onSelect(tab.id)}>
              <Icon size={18} />
              <span>{tab.label}</span>
              {value ? <small>{value}</small> : null}
            </button>
          );
        })}
      </nav>
    </aside>
  );
}

function badge(tab: AppTab, feature: FeatureSnapshot): string {
  switch (tab) {
    case "settings":
      return count(feature.outbox.length);
    case "home":
      return "";
  }
}

function count(value: number): string {
  return value > 0 ? String(value) : "";
}
