---
type: noun-entry
slug: nostrinlinevideoplayer
name: "NostrInlineVideoPlayer"
origin: extracted
source_refs:
  - transcript:2227-2228
---

# NostrInlineVideoPlayer

A dedicated SwiftUI view that holds an AVPlayer in @State, constructed exactly once per view identity, replacing the previous inline `VideoPlayer(player: AVPlayer(url:))` pattern inside NostrContentView's body that rebuilt the entire AVPlayerViewController (with full KVO observer churn) on every SwiftUI re-render.
