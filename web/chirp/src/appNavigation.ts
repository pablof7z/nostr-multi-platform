export type MainView =
  | "setup"
  | "home"
  | "saved"
  | "search"
  | "notifications"
  | "groups"
  | "offline"
  | "workspaces";

type ViewCopy = {
  kicker: string;
  title: string;
  support: string;
};

const VIEW_COPY: Record<MainView, ViewCopy> = {
  setup: {
    kicker: "First run",
    title: "Set up Chirp Web",
    support: "Start with a live runtime, relay feed, signer choice, and signed-action proof.",
  },
  home: {
    kicker: "Home feed",
    title: "Real relay timeline",
    support: "Read, publish, and verify every action through relay diagnostics.",
  },
  saved: {
    kicker: "NIP-51 bookmarks",
    title: "Saved notes",
    support: "Review notes from the Rust-owned bookmark projection and relay-hydrated feed.",
  },
  search: {
    kicker: "NIP-50 discovery",
    title: "Search relays and cache",
    support: "Find notes, profiles, and long-form posts with relay and cache provenance.",
  },
  notifications: {
    kicker: "Notifications",
    title: "Notifications",
    support: "Review replies, mentions, reactions, reposts, comments, and zaps with source relays.",
  },
  groups: {
    kicker: "NIP-29 groups",
    title: "Discover public groups",
    support: "Browse Rust-projected NIP-29 group metadata from the configured public group relay.",
  },
  offline: {
    kicker: "Storage and replay",
    title: "Inspect storage health",
    support: "Inspect store health, active replay interests, relay coverage, and pending publish state.",
  },
  workspaces: {
    kicker: "Product coverage",
    title: "More Chirp workspaces",
    support: "Private, value, and moderation surfaces stay disabled until Rust-owned web flows exist.",
  },
};

export function viewFromHash(hash: string): MainView {
  if (hash === "" || hash === "#setup" || hash === "#signing") return "setup";
  if (hash === "#saved") return "saved";
  if (hash === "#search") return "search";
  if (hash === "#notifications") return "notifications";
  if (hash === "#groups") return "groups";
  if (hash === "#offline") return "offline";
  if (hash === "#workspaces" || hash === "#messages" || hash === "#wallet" || hash === "#moderation") {
    return "workspaces";
  }
  return "home";
}

export function viewCopy(view: MainView, signedIn: boolean): ViewCopy {
  if (view !== "home" || signedIn) return VIEW_COPY[view];
  return {
    ...VIEW_COPY.home,
    title: "Set up Chirp Web",
    support: "Browse signed out, connect a signer when you are ready to publish.",
  };
}
