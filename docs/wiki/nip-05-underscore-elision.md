---
title: NIP-05 Underscore Local-Part Elision
slug: nip-05-underscore-elision
summary: NIP-05 badges elide the '_' local-part prefix, displaying 'f7z.io' instead of '_@f7z.io'.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-25
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:53838558-81bd-433d-a46d-d117ecebb361
---

# NIP-05 Underscore Local-Part Elision

## Display

NIP-05 identifiers with the '_@domain' pattern render as just 'domain' (e.g. '_@f7z.io' renders as 'f7z.io') in both Swift and Kotlin badge components. NostrNip05Badge has a failable init?(profile:) so it conditionally renders only when nip05 is present.

<!-- citations: [^6a951-125] [^53838-5] -->
## See Also

