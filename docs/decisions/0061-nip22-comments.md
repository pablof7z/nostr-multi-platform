# ADR-0061 - NIP-22 Comments (kind:1111) Crate, Projection, Action, and Count

> **Status:** Accepted pending implementation.
> **Date:** 2026-06-20.
> **Issue:** #1633.
> **Companions:** ADR-0037 (typed sidecar wire), ADR-0038 (OP-feed golden
> wire), `crates/nmp-nip25` (reaction analogue), `crates/nmp-nip51`
> (bookmark analogue).

## Context

NIP-22 comments (kind:1111) are threaded comments rooted at any artifact: an
event, an addressable article, or external content (a URL, ISBN, podcast
GUID). They are reusable Nostr infrastructure — article comments, highlight
discussions, room discussions — that any Nostr app could consume, so they
belong in an NMP crate, not in app-specific Rust (AGENTS.md "NMP crates vs.
app-specific crates").

Before this change nmp had no kind:1111 surface at all: no crate, no kind
literal, no comment action, no comment projection, and no comment-count
aggregation. The Highlighter (hl) app therefore carried a bespoke
`comments.rs` and could not move its comment/discussion surfaces onto the
kernel. This was the last fully-blocked Phase-4 content domain
(artifacts/highlights/reactions/bookmarks/search already had nmp APIs).

## Threading and scope model

A kind:1111 comment carries **two scopes** of reference tags:

- **Root scope (UPPERCASE)** — `A` (addressable `kind:pubkey:d`), `E` (event
  id), or `I` (external identifier), plus a companion `K` carrying the root
  kind. The root identifies the artifact the whole thread hangs off and is
  constant for every comment in a thread.
- **Parent scope (lowercase)** — `a` / `e` / `i` identifying the immediate
  parent (the comment being replied to), plus a companion `k` carrying the
  parent kind.

A **top-level** comment's parent *is* the root, so its parent scope mirrors
the root (lowercased): root `["E", <id>]` + `["K", <kind>]` yields parent
`["e", <id>]` + `["k", <kind>]`. A **reply** keeps the same uppercase root but
points its lowercase parent scope at the parent kind:1111 comment:
`["e", <parent-comment-id>]` + `["k", "1111"]`.

This mirrors hl's bespoke `comments.rs` exactly so hl can adopt the nmp surface
without changing its on-wire shape.

## Decision

Add a per-NIP `nmp-nip22` crate (the established per-NIP pattern; `nmp-nip25`,
`nmp-nip51`) with three seams, plus a count integration and a defaults runtime.

### 1. Kind constant in `nmp-kinds` (Layer-0)

`KIND_NIP22_COMMENT = 1111` is declared in the zero-dependency `nmp-kinds`
registry (re-exported through `nmp_core::kinds`). This lets `nmp-nip01`'s
`note_relations` recognise the kind for counting **without** depending on
`nmp-nip22` for just the integer. The decode/build/projection logic stays in
`nmp-nip22`.

### 2. Decode — `nmp_nip22::CommentRecord`

`try_from_kernel_event` parses a kernel event into a flat `CommentRecord` of
**raw protocol facts only**: event id, author, body, root tag name/value/kind,
parent tag name/value/kind, `created_at`. No display strings, no labels, no
counts — presentation belongs in the shell (D1). A comment missing a root
scope tag decodes to `None`.

### 3. Projection — `CommentThreadProjection`

An in-memory `KernelEventObserver` (the same read-model shape nip25 reactions
and nip51 bookmarks use — chosen over a typed FlatBuffers sidecar because
comments are consumed like reactions/bookmarks, not rendered as a standalone
feed). It buckets kind:1111 by root scope value into a bounded map
(`MAX_PROJECTION_MESSAGES`) and, on `snapshot_for(root)`, returns the flat
record set (newest-first) plus the parent/child forest (children oldest-first).
The tree builder promotes comments whose parent is absent from the bounded
window to the top level (fetched content stays visible) and breaks
self-referential parent edges so a malformed thread cannot recurse unbounded.

### 4. Action — `nmp.nip22.post_comment`

A standard `ActionModule` (the `nmp.nip25.react` shape) producing an unsigned
kind:1111 event with the correct two-scope tag set. Serde-typed
`PostCommentAction { root_tag_name, root_tag_value, root_kind, parent_event_id,
root_author_pubkey, parent_author_pubkey, content }`. `parent_event_id == None`
builds a top-level comment (parent mirrors root); `Some(id)` builds a reply
scoped to that comment. When the caller knows them, the root author is emitted
as the uppercase `P` notify tag and the parent author as the lowercase `p`
notify tag (NIP-22 §"who to notify"); both are optional and omitted when
absent. Validation rejects blank content, an empty/unknown root scope, a
non-hex `E`-root value, and non-hex parent/author pubkeys.

The read-path decoder is deliberately lenient (Postel's law for a
projection): it requires the load-bearing root scope tag but tolerates a
top-level comment that omits the lowercase parent scope (inferring parent ==
root) and treats an absent `K`/`k` as an empty kind hint, mirroring hl's
battle-tested `record_from_event`. Rejecting otherwise-threadable comments
that lack a strictly-optional tag would silently drop real content.

### 5. Comment-count aggregation — `nmp-nip01` `note_relations`

`NoteRelationIndex` gains a `Comment` relation that tallies kind:1111 against
its UPPERCASE root scope target (the artifact), alongside the existing
reply/reaction/repost/zap counts. A comment is counted as a `comment`, never
as a kind:1 `reply` — the two are distinct facts and a node can carry both. A
`comments:RelationCount` field is appended (after `zaps`) to the
`NoteRelationCounts` typed sidecar; appending a field is FlatBuffers
forward/backward compatible, so the OP-feed schema version is unchanged and a
decoder reading old bytes sees a known-zero comment count
(`RelationCountInterest::comments` provides the loading-interest seam).

### 6. Defaults runtime — `register_comment_runtime`

Installs the shared `CommentThreadProjection` as the kind:1111 observer and
registers the post-comment action (mirrors `register_bookmark_runtime`). Wired
into the social-features bundle in `register_defaults`.

## Doctrine compliance

- **D0 (substrate purity).** The kernel grows no NIP-22 nouns; kind:1111 lives
  entirely in `nmp-nip22` plus the Layer-0 integer in `nmp-kinds`.
- **D1 (raw projections).** `CommentRecord`, the thread forest, and the count
  hold raw protocol facts; all labels/symbols/counts-as-strings stay in the
  shell.
- **D4 (single writer per fact).** Comment threading is owned solely by
  `nmp-nip22`; the count is owned solely by `note_relations`.
- **D8 (no polling).** The projection is a push observer; the count index
  ingests on the kernel-event tick.

## Alternatives considered

- **Typed FlatBuffers sidecar for the thread (like nip23 articles).** Rejected
  for the thread itself: comments are consumed by snapshot like
  reactions/bookmarks, so the in-memory observer matches house style and
  avoids a second schema. The *count* still rides the existing OP-feed sidecar
  because comment counts surface on timeline cards.
- **Bumping `OP_FEED_SCHEMA_VERSION`.** Unnecessary — an appended optional
  FlatBuffers field is wire-compatible, so old decoders are unaffected.
