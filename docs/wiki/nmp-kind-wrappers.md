---
title: "NMP Kind Wrappers: Immutable Records, Builders & Views"
slug: nmp-kind-wrappers
summary: NMP's kind-wrapper design rejects NDK's mutable setter pattern (e.g., `article.title = "foo"`) as a D4 violation, prescribing instead a separated immutable reco
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:590ca0cd-3665-42f5-96ab-3ea035a79d67
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# NMP Kind Wrappers: Immutable Records, Builders & Views

## Architecture

NMP's kind-wrapper design rejects NDK's mutable setter pattern (e.g., `article.title = "foo"`) as a D4 violation, prescribing instead a separated immutable record + pure builder + ViewModule triad. The zero-dep `nmp-kinds` Layer-0 crate is the canonical home for all kind constants (including `KIND_GIFT_WRAP`), satisfying V-57 option c; `nmp_core::kinds` and `nmp_nip59::kinds` are thin re-exports so existing call sites are unchanged. Kind-agnostic tag primitives (e-tag, p-tag, a-tag, q-tag builders, NIP-10 reference parser) live in `nmp-core` as protocol codecs alongside `nip19`/`nip21`, while per-kind decoders and domain nouns remain in protocol crates to satisfy D0. The D17 doctrine-lint rule bans the `[1, 6]` social-kind literal in `nmp-core` production code, with a narrow shape-based anchor and test-file exemption including handling inline `#[cfg(test)]` blocks. All record types are immutable with no setters; builders consume `self`; FFI never panics (malformed JSON is dropped silently); views use accumulator-based insert/remove/replace logic.

<!-- citations: [^590ca-2] [^590ca-3] [^590ca-4] [^4edd4-20] -->
## See Also

