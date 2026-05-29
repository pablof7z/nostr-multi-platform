---
title: Content-Mention Profile Discovery Is Missing (V-56)
slug: content-mention-profile-discovery
summary: "Profile mentions in note content (nostr:npub1 / nostr:nprofile1 URIs) are tokenized for rendering but never fed to UnknownIds for relay discovery REQs, so mentioned-only-in-content profiles are never fetched."
tags:
  - ingest
  - discovery
  - content
  - mentions
  - v56
  - nmp-core
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
---

# Content-Mention Profile Discovery Is Missing (V-56)

> Profile mentions in note content (nostr:npub1 / nostr:nprofile1 URIs) are tokenized for rendering but never fed to UnknownIds for relay discovery REQs, so mentioned-only-in-content profiles are never fetched.

## The Bug

The content-mention ingest path does NOT tokenize `event.content` for profile discovery:

- The tokenizer at `crates/nmp-content/src/tokenizer.rs:206-210` correctly parses `nostr:npub1*` and `nostr:nprofile1*` URIs into `Segment::Mention(NostrUri::Profile {...})`.
- However, the ingest path at `crates/nmp-core/src/kernel/ingest/timeline.rs:164` calls `collect_unknown_refs(&event.tags)` only.
- `collect_unknown_refs` visits only the event's **tags** (via `unknown_ids.visit_tags(tags, ...)`), extracting `"p"` (pubkey) and `"e"`/`"q"` (event) tag references.
- Profile mentions embedded in note **content** (e.g., `nostr:npub1...` or `nostr:nprofile1...` in the text) are never extracted into `UnknownIds` for discovery REQs.

Result: referenced profiles mentioned only in content are never fetched. [^42908-22]

## Required Fix (V-56)

The ingest path must tokenize `event.content` to extract profile mentions and feed discovered profile pubkeys to `UnknownIds`. The fix point is `crates/nmp-core/src/kernel/ingest/timeline.rs` — after tag-based discovery, also tokenize content and visit any `Profile` mentions. [^42908-23]

## See Also

