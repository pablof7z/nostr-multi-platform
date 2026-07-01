---
title: NMP Registry Vercel Deployment
slug: nmp-registry-deployment
summary: The nmp-registry Vercel project does not auto-deploy on master merges
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# NMP Registry Vercel Deployment

## Deployment

The nmp-registry Vercel project does not auto-deploy on master merges. Deploy manually by running `vercel build --prod && vercel deploy --prebuilt --prod` from `web/registry`. [^6a951-25]


After editing any registry source file, regenerate `registry.json` by running `cargo run -p nmp-cli --bin nmp -- export jsrepo --registry crates/nmp-component-registry/registry --output web/registry/public`. The `committed_registry_json_matches_generated_output` test requires this output to be up to date. [^6a951-26]

## Tests

The `web_registry_install_metadata_mirrors_cli_manifest` test validates that website `.ts` component installIds match the CLI `registry.toml` manifest by reading `content.ts`, `user.ts`, and `relay.ts` (not `embeds.ts`). [^6a951-27]
## See Also
