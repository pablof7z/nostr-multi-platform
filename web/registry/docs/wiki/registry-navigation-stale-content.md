---
title: Registry Navigation Stale Content
slug: registry-navigation-stale-content
summary: Navigating between two valid component slugs on the registry site updates the URL but leaves the page content frozen on the previous component
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:36e16a9d-c2dc-4710-9bed-06a1c4517dc3
---

# Registry Navigation Stale Content

## Stale Content on Navigation

Navigating between valid (truthy) component slugs on the registry site updates the URL but leaves the page content frozen on the previous component. The SolidJS `Show` component in `ComponentPage.tsx` must use the `keyed` property so that children are re-created when the memo value changes by reference, not just when truthiness changes. This ensures navigating between valid slugs renders a fresh page with a new `platform` signal initialized to the correct default, rather than reusing the stale captured value from the initial render.

<!-- citations: [^36e16-2] [^36e16-3] -->
## See Also

