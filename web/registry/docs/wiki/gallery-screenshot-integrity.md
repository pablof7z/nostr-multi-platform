---
title: Gallery Screenshot Integrity
slug: gallery-screenshot-integrity
summary: Screenshots must be verified by direct pixel inspection (accessibility tree or visual review), never trusted from agent reports or assumed from compile-green st
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

# Gallery Screenshot Integrity

## Verification Standard

Screenshots must be verified by direct pixel inspection (accessibility tree or visual review), never trusted from agent reports or assumed from compile-green status. A gallery screenshot must never show 'Loading…'/'Fetching…' as a final state, never have blank image placeholders, and 'probably ok' is banned — the image must actually render. [^6a951-13]


## Warm-Session Capture Methodology

The warm-session screenshot methodology uses one continuous session without force-stopping the app between components, allowing the cold kernel 20-30 seconds to resolve claims before capture. [^6a951-14]

## Integrity Remediations

Five broken live-site screenshots were caught and repointed to verified captures: a hex byline on content-kind-30023, a 'loading…' state on content-kind-9802, a wrong-page capture on content-kind-registry, a raw-hex quote on tui-embed-highlight, and unverified pre-fix Android duplicates. 27 stale/broken orphan screenshots (each verified as zero real references) were deleted from the repository. [^6a951-15]
## See Also

