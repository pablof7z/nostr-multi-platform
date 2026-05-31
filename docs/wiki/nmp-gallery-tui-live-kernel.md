---
title: NMP Gallery TUI LiveKernel and Relay Data Pipeline
slug: nmp-gallery-tui-live-kernel
summary: "The gallery uses a LiveKernel from nmp-gallery-tui with the same relay path (purplepag.es + relay.primal.net) as the TUI, reading kind:0 data via a blocking Rec"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:6e8af009-f065-464a-98f1-3ec1ee4ed933
---

# NMP Gallery TUI LiveKernel and Relay Data Pipeline

## LiveKernel & Relay Configuration

The gallery uses a LiveKernel from nmp-gallery-tui with the same relay path (purplepag.es + relay.primal.net) as the TUI, reading kind:0 data via a blocking Receiver thread. [^6e8af-3]


## Dependencies

Cargo.toml depends on iced 0.14 with the tokio feature enabled and on nmp-gallery-tui for LiveKernel, REGISTRY_SECTIONS, ProfileWire, and GalleryData. [^6e8af-4]
## See Also

