import { Bolt, House, MessageSquare, Radio, Settings, UsersRound } from "lucide-solid";
import type { FeatureSnapshot } from "../nmp/snapshot";

export type AppTab = "home" | "chats" | "groups" | "wallet" | "settings";

const tabs = [
  { id: "home", label: "Home", icon: House },
  { id: "chats", label: "Chats", icon: MessageSquare },
  { id: "groups", label: "Groups", icon: UsersRound },
  { id: "wallet", label: "Wallet", icon: Bolt },
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
    case "chats":
      return count(feature.dmConversations.length);
    case "groups":
      return count(feature.discoveredGroups.length + feature.groupMessages.length);
    case "settings":
      return count(feature.outbox.length);
    case "wallet":
      return feature.wallet.status;
    case "home":
      return "";
  }
}

function count(value: number): string {
  return value > 0 ? String(value) : "";
}
