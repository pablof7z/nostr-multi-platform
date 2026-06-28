// Codegen: single-source Chirp relay defaults from the Rust source of truth.
//
// Reads apps/chirp/crates/nmp-chirp-config/src/lib.rs and emits
// web/chirp/src/chirpConfig.generated.ts so the web host cannot drift from the
// authoritative Rust constants (D4: one source per fact). Run via
// `npm run codegen:chirp-config -w @nmp/chirp-web`. No dependencies beyond Node
// built-ins.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = join(scriptDir, "..", "..", "..");
const rustSourceRel = "apps/chirp/crates/nmp-chirp-config/src/lib.rs";
const rustSourcePath = join(repoRoot, rustSourceRel);
const outPath = join(scriptDir, "..", "src", "chirpConfig.generated.ts");

const rust = readFileSync(rustSourcePath, "utf8");

// 1. Extract `pub const CHIRP_*_URL: &str = "wss://...";` constants in order.
const urlConstants = [];
const constRe = /pub const (CHIRP_[A-Z0-9_]*_URL): &str = "(wss:\/\/[^"]+)";/g;
for (let m; (m = constRe.exec(rust)); ) {
  urlConstants.push({ name: m[1], url: m[2] });
}
if (urlConstants.length === 0) {
  throw new Error(`No CHIRP_*_URL constants found in ${rustSourceRel}`);
}

const urlByName = new Map(urlConstants.map((c) => [c.name, c.url]));

// 2. Extract the CHIRP_RELAY_BOOTSTRAP array entries (url const name + role).
const bootstrapBlockRe =
  /pub const CHIRP_RELAY_BOOTSTRAP: &\[ChirpRelayBootstrapEntry\] = &\[([\s\S]*?)\];/;
const bootstrapBlock = bootstrapBlockRe.exec(rust);
if (!bootstrapBlock) {
  throw new Error(`CHIRP_RELAY_BOOTSTRAP array not found in ${rustSourceRel}`);
}
const entryRe =
  /ChirpRelayBootstrapEntry\s*\{\s*url:\s*(CHIRP_[A-Z0-9_]*_URL)\s*,\s*role:\s*"([^"]+)"\s*,?\s*\}/g;
const bootstrap = [];
for (let m; (m = entryRe.exec(bootstrapBlock[1])); ) {
  const url = urlByName.get(m[1]);
  if (!url) {
    throw new Error(`Bootstrap references unknown URL constant ${m[1]}`);
  }
  bootstrap.push({ nameRef: m[1], role: m[2] });
}
if (bootstrap.length === 0) {
  throw new Error(`CHIRP_RELAY_BOOTSTRAP array parsed empty in ${rustSourceRel}`);
}

// 3. Render the TypeScript module. Signature matches the prior hand-written
//    chirpConfig.ts (overrideRelays override + { relays, relay_bootstrap }) so
//    web/chirp/src/nmp/client.ts imports are unchanged.
const constLines = urlConstants.map((c) => `export const ${c.name} = ${JSON.stringify(c.url)};`);
const bootstrapLines = bootstrap.map(
  (e) => `  { url: ${e.nameRef}, role: ${JSON.stringify(e.role)} },`,
);

const out = `// GENERATED — do not edit by hand. Run \`npm run codegen:chirp-config -w @nmp/chirp-web\` to regenerate.
// Source: ${rustSourceRel} (CHIRP_*_URL constants + CHIRP_RELAY_BOOTSTRAP).
//
// Relay defaults are Chirp app/operator policy, not framework policy
// (#1125/#1493): the browser worker protocol carries no built-in relay
// defaults, so the Chirp web host supplies its own \`relays\` +
// \`relay_bootstrap\` in the Start request. These are single-sourced from the
// Rust crate so the two can never drift (#1546 F6).

${constLines.join("\n")}

export type ChirpRelayBootstrapEntry = { url: string; role: string };

export const CHIRP_RELAY_BOOTSTRAP: ChirpRelayBootstrapEntry[] = [
${bootstrapLines.join("\n")}
];

export function chirpDefaultRelayUrls(): string[] {
  return CHIRP_RELAY_BOOTSTRAP.map((entry) => entry.url);
}

/** Resolve the \`relays\` + \`relay_bootstrap\` the Chirp web host supplies in the
 *  Start request. Relay policy is Chirp app/operator policy (#1125/#1493):
 *  the browser worker protocol has no built-in defaults, so the host always sends an
 *  explicit list.
 *
 *  When \`overrideRelays\` is supplied (e.g. the Playwright smoke test via the
 *  \`?relay=\` query parameter), those URLs replace the Chirp defaults. Each is
 *  given role "both,indexer" (not just "both") so a single injected relay also
 *  serves profile-claim discovery requests (BootstrapSeed::IndexerOnly). */
export function chirpStartRelays(overrideRelays?: string[]): {
  relays: string[];
  relay_bootstrap: ChirpRelayBootstrapEntry[];
} {
  if (overrideRelays && overrideRelays.length > 0) {
    return {
      relays: overrideRelays,
      relay_bootstrap: overrideRelays.map((url) => ({ url, role: "both,indexer" })),
    };
  }
  return {
    relays: chirpDefaultRelayUrls(),
    relay_bootstrap: CHIRP_RELAY_BOOTSTRAP,
  };
}
`;

writeFileSync(outPath, out);
console.log(
  `chirpConfig.generated.ts: ${urlConstants.length} URL constants, ${bootstrap.length} bootstrap entries`,
);
