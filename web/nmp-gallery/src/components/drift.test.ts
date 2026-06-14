import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, relative } from "node:path";

// The gallery vendors the web user-* components byte-identical from the
// registry canonical source so it deploys self-contained. This test is the
// drift gate: any divergence fails CI. Re-sync by copying the registry tree.
const VENDORED = fileURLToPath(new URL(".", import.meta.url));
const CANONICAL = fileURLToPath(
  new URL("../../../registry/src/vendor/web", import.meta.url),
);

function walk(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) {
      out.push(...walk(full));
    } else if (!name.endsWith(".test.ts")) {
      out.push(full);
    }
  }
  return out;
}

describe("web component vendor drift gate", () => {
  const canonicalFiles = walk(CANONICAL)
    .map((f) => relative(CANONICAL, f))
    .sort();

  it("vendored set matches the canonical file set", () => {
    const vendoredFiles = walk(VENDORED)
      .map((f) => relative(VENDORED, f))
      .sort();
    expect(vendoredFiles).toEqual(canonicalFiles);
  });

  for (const rel of canonicalFiles) {
    it(`is byte-identical: ${rel}`, () => {
      const canonical = readFileSync(join(CANONICAL, rel), "utf8");
      const vendored = readFileSync(join(VENDORED, rel), "utf8");
      expect(vendored).toBe(canonical);
    });
  }
});
