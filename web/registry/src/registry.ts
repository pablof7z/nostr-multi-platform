/*
 * Static component manifest for the registry showcase site.
 *
 * Install-critical metadata is mirrored from the CLI manifest at
 * crates/nmp-cli/registry/registry.toml. The nmp-cli integration tests compare
 * install ids, platform versions, dependencies, and file mappings so this
 * showcase cannot drift from the offline registry apps actually install.
 */

import type { Component, Section } from "./registry/types";
import { contentComponents } from "./registry/content";
import { userComponents } from "./registry/user";
import { relayComponents } from "./registry/relay";

export type { Component, ComponentFile, Platform, PlatformImpl, Section } from "./registry/types";
export { PLATFORM_LABELS, PLATFORM_ORDER } from "./registry/types";

export const COMPONENTS: Component[] = [
  ...contentComponents,
  ...userComponents,
  ...relayComponents,
];

export const SECTIONS: Section[] = [
  { label: "Content", components: contentComponents },
  { label: "User", components: userComponents },
  { label: "Relay", components: relayComponents },
];

export function findComponent(routeId: string): Component | undefined {
  return COMPONENTS.find((c) => c.routeId === routeId);
}

export function installCommand(installId: string): string {
  return `nmp add component ${installId}`;
}
