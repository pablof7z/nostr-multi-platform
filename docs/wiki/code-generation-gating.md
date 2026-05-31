---
title: Code Generation Gating and Fixture Regeneration
slug: code-generation-gating
summary: Wire golden fixture files must be regenerated via `cargo run -p nmp-content-fixtures --bin build-wire-fixtures` when the wire contract changes.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-28
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:56db993b-6de7-49f9-82b1-a9416cef3294
---

# Code Generation Gating and Fixture Regeneration

## Wire Fixture Regeneration

The existing byte callback ABI remains stable with the golden fixture unchanged. Golden fixtures pin ContentTreeWire and ModularTimelineSnapshot wire shapes. Wire golden fixture files must be regenerated via `cargo run -p nmp-content-fixtures --bin build-wire-fixtures` when the wire contract changes. [^f2605-4]

<!-- citations: [^f2605-4] [^56db9-1] -->
## See Also

