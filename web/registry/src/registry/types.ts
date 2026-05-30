export type Platform = "swiftui" | "compose" | "tui" | "desktop" | "web";

export const PLATFORM_ORDER: Platform[] = ["swiftui", "compose", "tui", "desktop", "web"];

export const PLATFORM_LABELS: Record<Platform, string> = {
  swiftui: "SwiftUI",
  compose: "Compose",
  tui: "TUI",
  desktop: "Desktop",
  web: "Web",
};

export type ComponentFile = {
  source: string;
  target: string;
  role: "source" | "example";
  content: string | null;
};

export type PlatformImpl = {
  status: "stable" | "soon";
  installId: string;
  version: string;
  dependencies: string[];
  files: ComponentFile[];
  screenshots: string[];
  longDescription?: string;
  customization: string[];
};

export type Component = {
  slug: string;
  routeId: string;
  version: string;
  description: string;
  inFlight?: boolean;
  platforms: Partial<Record<Platform, PlatformImpl>>;
};

export type Section = {
  label: string;
  components: Component[];
};
