// Single canonical source for native (SwiftUI / Compose / TUI / Desktop)
// component bodies shown in the showcase's copy-paste code panels.
//
// These bodies are read out of `public/registry.json` — the committed,
// drift-gated artifact produced by `nmp export jsrepo` from the canonical
// component registry (`crates/nmp-component-registry/registry/`) and verified by
// `crates/nmp-cli/tests/export.rs`. There is no hand-copied vendor fork to
// drift: this is the one canonical path.
//
// The web component templates are canonical in `@nmpis/components-web` and gated
// by the gallery; they are imported with `?raw` directly from that package.
import registryJson from "../../public/registry.json";

interface RegistryFile {
  path: string;
  content: string;
}

interface RegistryItem {
  files?: RegistryFile[];
}

interface RegistryExport {
  items?: RegistryItem[];
}

const sourceByPath: Map<string, string> = (() => {
  const map = new Map<string, string>();
  const { items = [] } = registryJson as RegistryExport;
  for (const item of items) {
    for (const file of item.files ?? []) {
      if (typeof file?.path === "string" && typeof file?.content === "string") {
        map.set(file.path, file.content);
      }
    }
  }
  return map;
})();

/**
 * Returns the canonical native source body for a registry file path
 * (e.g. "registry/swiftui/content-core/NostrContentRenderer.swift").
 *
 * Throws at call time (module-load, since callers resolve at module top level)
 * if the path is absent from `registry.json`, so a missing/renamed file fails
 * loud instead of silently rendering an empty panel.
 */
export function nativeSource(path: string): string {
  const content = sourceByPath.get(path);
  if (content === undefined) {
    throw new Error(
      `nativeSource: "${path}" not found in registry.json. ` +
        `The showcase reads native source from the gated registry export; ` +
        `regenerate it with \`nmp export jsrepo\` if the registry changed.`,
    );
  }
  return content;
}
