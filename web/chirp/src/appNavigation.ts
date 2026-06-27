export type MainView =
  | "setup"
  | "home"
  | "saved"
  | "search"
  | "notifications"
  | "groups"
  | "signer"
  | "profile"
  | "relays"
  | "offline"
  | "workspaces"
  | "diagnostics";

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
  signer: {
    kicker: "Account session",
    title: "Manage signer",
    support: "Connect or inspect the browser signer that unlocks signed Chirp actions.",
  },
  profile: {
    kicker: "Profile metadata",
    title: "Publish your Nostr profile",
    support: "Edit kind:0 metadata and publish it through the same signed outbox path as notes.",
  },
  relays: {
    kicker: "Relay management",
    title: "Manage relay inventory",
    support: "Inspect, add, remove, and publish relay preferences through runtime diagnostics.",
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
  diagnostics: {
    kicker: "Runtime diagnostics",
    title: "Inspect routing and outbox",
    support: "Review relay state, subscriptions, outbox, action results, and routing traces.",
  },
};

export function viewFromHash(hash: string): MainView {
  const route = hash.split("?")[0];
  if (route === "" || route === "#setup") return "setup";
  if (route === "#signing" || route === "#signer") return "signer";
  if (route === "#saved") return "saved";
  if (route === "#search") return "search";
  if (route === "#notifications") return "notifications";
  if (route === "#groups") return "groups";
  if (route === "#profile") return "profile";
  if (route === "#relays") return "relays";
  if (route === "#offline") return "offline";
  if (route === "#workspaces" || route === "#messages" || route === "#wallet" || route === "#moderation") {
    return "workspaces";
  }
  if (route === "#diagnostics") return "diagnostics";
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
