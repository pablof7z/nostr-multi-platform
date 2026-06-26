---
type: noun-entry
slug: crates-nmp-browser-runtime
name: "crates/nmp-browser-runtime"
origin: extracted
source_refs:
  - transcript:296-301
  - transcript:323-348
---

# crates/nmp-browser-runtime

The browser platform adapter (Layer-6, per ADR-0067); a composition root that owns the Worker event loop, WebSocket transport (transport-only, no policy), BrowserAppBuilder typed composition, capability/signer provider registry, browser timer/clock seams, and composes NMP defaults through the same registration surface native uses
