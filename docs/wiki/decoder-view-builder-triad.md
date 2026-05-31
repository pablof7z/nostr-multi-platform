---
title: Decoder + ViewModule + Builder Triad Pattern (D4 Doctrine)
slug: decoder-view-builder-triad
summary: NMP intentionally rejects NDK's mutable setter pattern (e.g., `article.title = 'foo'`) as a D4 violation, using instead an immutable decoder + builder triad pat
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-18
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:590ca0cd-3665-42f5-96ab-3ea035a79d67
---

# Decoder + ViewModule + Builder Triad Pattern (D4 Doctrine)

## Decoder-View-Builder Triad

NMP intentionally rejects NDK's mutable setter pattern (e.g., `article.title = 'foo'`) as a D4 violation, using instead an immutable decoder + builder triad pattern. NMP relation accessors use the doctrine-prescribed decoder + ViewModule + builder triad pattern rather than applesauce-style instance-accessor wrappers holding a store reference, to comply with D4/D5/D8 doctrine. [^590ca-2]


## Builder Write Surface

The Article builder (nmp-nip23) produces an UnsignedEvent with no clock or signer — pure tag-shape construction — which is the doctrine-correct write surface per kind-wrappers.md §3.2. NMP's EventFactory / builder pattern for Rust uses a consume-self fluent chain producing UnsignedEvent, with signing as a separate async step (`signer.sign(event).await`), unlike applesauce's Promise-based `.chain()` → returns `this` pattern. [^590ca-3]
## See Also

