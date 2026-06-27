# Product Spec: Chirp Web

Chirp Web is the browser reference client for NMP. It must demonstrate that the
WASM worker runtime can read, render, sign, publish, and diagnose real Nostr
traffic without moving protocol policy into TypeScript.

## First-Run Contract

A new browser profile must open into a usable product surface, not a dead demo.
The first screen must show relay-backed feed state, relay health, signer state,
compose affordances, and diagnostics for publish/action outcomes.

First run is a guided onboarding flow, not a passive status list. The UI must
show the next action needed to reach a signed product session, expose the
available identity paths directly on that screen, and advance to a complete
session only after runtime, relays, signer, and feed projection are all live.
While unsigned, onboarding is the first product workspace, not a secondary
sidebar card. The live feed may remain visible as read-mode proof, but identity
choices and session proof must occupy the primary first-run region.

Users without a browser extension must still have a complete write path:
Chirp Web supports a memory-only local-key session by accepting an `nsec` and
handing it directly to `nmp-browser-runtime` as `set_identity kind=local_key`.
Rust decodes the secret, derives the pubkey, registers the signer, and owns all
signing. TypeScript may read the form value only to send that request; it must
not decode, derive from, cache, or sign with the secret.

NIP-07 remains the preferred browser-extension path. NIP-46 is not a web
onboarding path until the browser runtime wires a bunker signer end to end.

## Search Discovery Contract

Chirp Web must expose NIP-50 search as a first-level product workspace, not as a
shell-local filter over the visible feed. The browser shell may collect the
query, selected scope, and leaf-app search relay policy, but Rust must validate
and bound the query, build the `SearchRequest`, resolve targets, open the
relay-pinned interests, ingest cache hits through the NIP-50 FTS path, and emit
typed `N50S` results under `nmp.nip50.search.<session>`.

TypeScript must decode the typed `N50S` snapshot and render results. It must not
construct NIP-01 search filters, scan feed rows as a substitute for search, or
invent result provenance. The default Chirp Web search relay is app policy from
the generated Chirp config; tests may override it through URL state to keep
acceptance hermetic.

Acceptance must prove that opening the Search workspace sends a real Rust-owned
NIP-50 session through the browser worker, that a fixture relay receives the
search subscription, and that matching signed events render from the typed
search sidecar with relay/cache provenance.

## Profile Publish Contract

Chirp Web must let a signed-in user publish their public identity from the first
product session. The browser shell may collect profile fields such as display
name, about text, and picture URL, but it must send them through the typed
profile publish command. Event construction, signing, outbox routing, relay
selection, and acceptance diagnostics remain owned by Rust and the browser
runtime.

Publishing profile metadata must surface the same proof path as note publishing:
the outbox shows the in-flight action, action results show the runtime verdict,
and relay diagnostics show per-relay acceptance or failure. Local validation must
assert that a fixture relay receives a signed kind:0 event with the requested
metadata.

## Reaction Publish Contract

Chirp Web must let a signed-in user react to a feed or thread note through the
typed NIP-25 action path. The browser shell may expose the Like affordance and
send the selected event id, but Rust owns event construction, target-author tag
resolution, signing, outbox routing, relay selection, and diagnostics.

Reaction acceptance must prove that a fixture relay receives a signed kind:7
event from the active user with the selected event's `e` tag, the target author's
`p` tag, and the requested reaction content. The outbox/action result surfaces
must show the same terminal relay verdicts used by notes and profiles.

## Repost Publish Contract

Chirp Web must let a signed-in user repost a feed or thread note through the
typed NIP-18 action path. The browser shell may expose the Repost affordance and
send the target event id, target kind, target author, and relay hint already
decoded from Rust projections, but Rust owns wrapper-kind selection, NIP-18 tag
construction, signing, outbox routing, relay selection, and diagnostics.

Kind:1 targets must publish kind:6 repost wrappers. Other public target kinds
must publish kind:16 generic repost wrappers. TypeScript must not construct
`e`, `p`, or `k` tags and must not fall back to `nmp.publish`/`PublishRaw` for
reposts.

Acceptance must prove that repost publishes a signed kind:6 or kind:16 event
from the active user with the selected event's `e` tag, the target author's `p`
tag when known, the target-kind `k` tag, and the same outbox/action result
surfaces used by notes, profiles, reactions, follows, and bookmarks.

## Quote Repost Publish Contract

Chirp Web must let a signed-in user quote a feed or thread note through the
typed NIP-18 quote-repost action path. The browser shell may expose the Quote
affordance, the composer target preview, and the user's commentary, but Rust
owns kind:1 event construction, NIP-18 `q` tag construction, target metadata
tags, signing, outbox routing, relay selection, and diagnostics.

Quote reposts must publish kind:1 notes with non-empty commentary, a `q` tag
for the selected event, the target author's `p` tag when known, and the
target-kind `k` tag. TypeScript must not construct `q`, `p`, or `k` tags and
must not fall back to `nmp.publish`/`PublishRaw` for quote reposts.

Acceptance must prove that quote repost publishes a signed kind:1 event from the
active user with the selected event's `q` tag, the target author's `p` tag when
known, the target-kind `k` tag, the requested commentary, and the same
outbox/action result surfaces used by notes, profiles, reactions, reposts,
follows, and bookmarks.

## Follow Publish Contract

Chirp Web must let a signed-in user follow and unfollow a displayed author
through the typed NIP-02 action path. The browser shell may expose the profile
button and selected pubkey, but button state must derive from Rust's
`nmp.follow_list` projection. TypeScript must not maintain an independent
contact graph, construct kind:3 tags, choose relay targets, or decide whether a
contact-list edit is safe.

Rust owns kind:3 read-modify-write construction, contact-list metadata
preservation, signing, outbox routing, relay selection, diagnostics, and the
fail-closed `follow_list_not_loaded` behavior. If the active account's kind:3
baseline is not loaded, Chirp Web must surface the action failure honestly
instead of publishing an empty-list replacement.

Acceptance must prove that follow publishes a signed kind:3 event from the
active user with the selected author's `p` tag present, unfollow publishes a
signed kind:3 event with that `p` tag removed, the visible button flips from the
Rust follow-list projection, and the same outbox/action result surfaces show the
terminal relay verdict.

## Bookmark Publish Contract

Chirp Web must let a signed-in user save and unsave feed or thread notes through
the typed NIP-51 bookmark action path. The browser shell may expose the save
affordance, selected event id, and relay hint, but button state must derive from
Rust's `nmp.nip51.bookmarks` projection. TypeScript must not maintain an
independent bookmark set, construct kind:10003 tags, choose relay targets, or
decide whether a bookmark-list edit is safe.

Chirp Web must also expose a Saved view. The Saved view is a presentation filter
over notes already hydrated from Rust-owned feed projections, with membership
coming only from `nmp.nip51.bookmarks`. If the bookmark list contains ids whose
events have not hydrated yet, the UI must say the saved notes are syncing instead
of fabricating placeholder notes or maintaining a shell-side saved-note cache.

Rust owns kind:10003 read-modify-write construction, metadata preservation,
signing, outbox routing, relay selection, diagnostics, and active-account
authorization. If the active account's bookmark baseline is unavailable or the
requested item conflicts with the loaded list, Chirp Web must surface the action
failure honestly instead of publishing a replacement from shell-local state.

Acceptance must prove that bookmark publishes a signed kind:10003 event from the
active user with the selected note's `e` tag present, removing the bookmark
publishes a signed kind:10003 event with that `e` tag removed, the visible button
flips from the Rust bookmark projection, and the same outbox/action result
surfaces show the terminal relay verdict. Reload acceptance must prove that a
fresh browser session can refetch the bookmark list from relays and show the
saved note in the Saved view without relying on in-memory UI state.

## Secret Storage

Pasted `nsec` values are session-memory only. Chirp Web must not persist them to
localStorage, sessionStorage, IndexedDB, OPFS, snapshots, action history, debug
logs, or URL state. Reloading the page requires the user to paste the key again
unless a future secure-storage decision changes this spec.

All user-visible and diagnostic outputs must be log-safe: redacted request debug,
action stages, action results, and publish outbox projections must never include
the raw secret.

## Blocked Web Workspace Contract

Chirp Web must not hide missing major product areas behind absent navigation or
fake local-only controls. Until web-ready Rust projections and actions exist for
notifications, NIP-17 private messages, groups, wallet/zap flows, moderation/WoT,
or offline replay ownership, the browser product must expose those destinations
as blocked workspaces with clear reasons.

Blocked workspace controls may emit log-safe `capability_failure` diagnostics so
users and tests can prove the unsupported state is deliberate. They must not
construct Nostr events, maintain shell-local unread counts, fabricate private
message threads, simulate wallet state, or persist policy choices in TypeScript.

When any blocked area becomes supported, the same navigation destination should
graduate to Rust-owned projections/actions and browser acceptance that proves the
real workflow rather than adding a second parallel surface.
