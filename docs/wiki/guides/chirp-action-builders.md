---
title: Chirp Action-Builders Registry and Codegen
slug: chirp-action-builders
topic: app-codegen
summary: The NMP CLI no longer has the `nmp gen swift` subcommand or the `nmp-core` `codegen-schema` feature
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# Chirp Action-Builders Registry and Codegen

## Action Builders Generation

The NMP CLI no longer has the `nmp gen swift` subcommand or the `nmp-core` `codegen-schema` feature. They are replaced by granular subcommands (`gen typed-decoders`, `gen projection-cache`, `gen action-builders`, `gen concept-reads`, `gen feed-helpers`, etc.). The action-builders generator is invoked per platform via `cargo run -p nmp-codegen -- gen action-builders --platform swift|kotlin` for iOS and Android. Web's action builders are not generated locally; they come from the external `@nmp/runtime-web` npm package.

The re-pinning of Chirp's NMP dependency drops three action builders (`zap`, `postComment`, and `topicArticles`) and renames `createPublicGroup` to `createGroup` matching the NIP-29 rename.

The KernelTypes.generated.swift drift CI check is converted to an explicit tracked-skip that exits 0 with a notice pointing to chirp#37 and NMP#2918, preserving the old baseline in a header comment. <!-- [^dcc80-17956] -->

If any NMP search primitive (open_search/NIP-50, ref resolution, nmp-nip-ad) is not cleanly exposable to the Swift facade, it is filed as an NMP bug rather than hacked around in Chirp. <!-- [^dcc80-58d97] -->

<!-- citations: [^dcc80-5df63] [^dcc80-ea129] [^dcc80-66957] [^dcc80-27010] [^dcc80-f578a] [^dcc80-7decc] [^dcc80-bee88] -->
