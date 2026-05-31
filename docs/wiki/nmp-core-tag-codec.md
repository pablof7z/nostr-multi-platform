---
title: NMP Core Tag Codec — Kind-Agnostic NIP-10 Parsing and Tag Builders
slug: nmp-core-tag-codec
summary: NMP provides a kind-agnostic tag codec module (`nmp-core/src/tags.rs`) with NIP-10 reference parsing (`parse_nip10` → `Nip10Refs`) and `e`/`p`/`a`/`q` tag build
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
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
---

# NMP Core Tag Codec — Kind-Agnostic NIP-10 Parsing and Tag Builders

## Tag Codec

NMP provides a kind-agnostic tag codec module (`nmp-core/src/tags.rs`) with NIP-10 reference parsing (`parse_nip10` → `Nip10Refs`) and `e`/`p`/`a`/`q` tag builders, re-exported through `nmp_core::substrate::*`. Reply NIP-10 tagging must forward the `root` marker and re-notification `p` tag, not just the `reply` marker, per `publish.rs:72-78`. NIP-10 reference parsing lives in `nmp-core` as a protocol codec (alongside `nip19`/`nip21`), not in a per-kind protocol crate, because D0 only governs per-kind decoders and domain nouns.

<!-- citations: [^590ca-5] [^57528-21] -->
## See Also

