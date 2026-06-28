# Product Spec: Chirp Web

Chirp Web is the browser reference client for NMP. It must prove the WASM
worker can read, render, sign, publish, and diagnose real Nostr traffic without
TypeScript protocol policy.

## Production Deployment Contract

Chirp Web production deploys must use a normal Vercel remote build from the
monorepo root. The source bundle must include the Rust workspace crates, Chirp's
app Rust workspace members, `web/packages`, and `web/chirp` so the deploy builds
`nmp-browser-runtime` WASM from source before bundling the thin TypeScript
shell. The canonical command path is the root `vercel.json`: install the web
workspace, run `@nmp/chirp-web`'s `build:vercel`, and publish
`web/chirp/dist`.

Prebuilt uploads are diagnostic or emergency artifacts only. They are not the
canonical production path and do not satisfy production-proof acceptance for
first-run relay bootstrap. A production proof must come from a fresh normal
remote build that serves `/nmp-browser-runtime/nmp_browser_runtime.js`,
`/nmp-browser-runtime/nmp_browser_runtime_bg.wasm`, and the SQLite WASM vendor
files staged by the runtime build script.

## First-Run Contract

New browser profiles must open into usable product, not a dead demo, with
relay/feed health, signer state, compose affordances, and publish/action diagnostics.

First run is guided onboarding. UI must expose next action and identity paths,
advancing only after runtime, relays, signer, and feed projection are live.
While unsigned, onboarding is primary; feed is proof only when session proof stays primary.
Empty feeds must link to discovery, relay checks, or identity setup.
The setup workspace must also expose concrete readiness for the first product
workspaces (read feed, discovery, private messages, diagnostics) so a new user
can see what is available now, what is locked behind identity, and where to go
next without reading developer diagnostics.

Chirp's production relay bootstrap is app/operator policy, single-sourced in
`apps/chirp/crates/nmp-chirp-config`. It is not an NMP framework default and
must not be duplicated in `nmp-core`, `nmp-defaults`, or `nmp-browser-runtime`. The production default set uses
`wss://relay.primal.net` with role `"both,indexer"` because current
production-browser evidence shows that role shape can keep a connected
write-capable lane and produce signed kind:1 acceptance, which the first-run
proof needs for a terminal relay verdict. `wss://purplepag.es` remains role
`"indexer"` as an additional discovery/profile relay-list lane and must not be
counted as a write-proof target. The generated Chirp web config is regenerated
from the Rust source rather than hand-edited.

`#signing` is the first-level account workspace, not a Setup alias. It must
mark Signer active, keep signer status primary, and hide unrelated feed panes.

Users without a browser extension must still have a complete write path:
Chirp Web supports a memory-only local-key session by accepting an `nsec` and
handing it directly to `nmp-browser-runtime` as `set_identity kind=local_key`.
Rust decodes the secret, derives the pubkey, registers the signer, and owns all
signing. TypeScript may read the form value only to send that request; it must
not decode, derive from, cache, or sign with the secret.

NIP-07 remains preferred for browser-extension accounts. When the extension
exposes both `window.nostr.nip44.encrypt` and `window.nostr.nip44.decrypt`, the
Rust NIP-07 signer may use those methods for NIP-44 through the same signer
trait path; extensions that lack either verb are limited for private-message
work and must fail visibly rather than being treated as NIP-44 capable.
TypeScript must not call `window.nostr.nip44` directly. NIP-46 bunker sign-in is
a supported browser-runtime signer path when the shell supplies a `bunker_uri`;
Rust owns the handshake, signer installation, and subsequent signing.
The browser signer/private-flow capability matrix in
[`docs/wasm-surface.md`](../wasm-surface.md#browser-signerprivate-flow-capability-model)
is the generic NMP contract that this product spec follows.

## Search Discovery Contract

Chirp Web must expose NIP-50 search as a first-level product workspace, not as a
shell-local filter over the visible feed. The browser shell may collect the
query, selected scope, and leaf-app search relay policy, but Rust must validate
and bound the query, build the `SearchRequest`, resolve targets, open the
relay-pinned interests, ingest cache hits through the NIP-50 FTS path, and emit
typed `N50S` results under `nmp.nip50.search.<session>`.

TypeScript must decode the typed `N50S` snapshot and render results. It must not
construct NIP-01 search filters, scan feed rows as search, or invent result
provenance. The default search relay is app policy; tests may override it via URL.

Acceptance must prove that opening the Search workspace sends a real Rust-owned
NIP-50 session through the browser worker, that a fixture relay receives the
search subscription, and that matching signed events render from the typed
search sidecar with relay/cache provenance.

## Group Discovery Contract

Chirp Web must expose NIP-29 public group discovery as a first-level product
workspace. The browser shell may choose the Chirp public group relay from app
policy and request that the workspace opens or closes, but Rust must own the
relay-pinned NIP-29 metadata interest, cache replay, group metadata projection,
and typed `NDGS` sidecar under `nmp.nip29.discovered_groups`.

TypeScript must decode the typed `NDGS` snapshot and render discovered group
metadata. It must not construct NIP-29 filters, keep a parallel group cache,
derive counts, or invent provenance. Opening a group view must request a
Rust-owned relay-pinned NIP-29 `#h` group-events interest for the consumer's
declared kinds (a chat view sends kinds `[9, 11]`) and render the typed `NGEV`
sidecar under `nmp.nip29.group_events`. Until Rust-owned flows exist for
join/leave and moderation, those controls must expose blocked diagnostics.

Acceptance must prove that opening the Groups workspace sends a real
Rust-owned NIP-29 discovery subscription to the configured group relay, and
that signed kind:39000/39001/39002 fixture events render from the typed group
discovery sidecar with relay provenance. Acceptance must also prove that opening
a group sends a real kind:9/11 `#h` subscription and renders signed group
messages from the typed timeline sidecar.

## Notifications Contract

Chirp Web must expose notifications as a first-level product workspace for the
active account. The browser shell may request that a notification session opens
for the Rust-owned active account pubkey, but Rust must own the bounded `#p`
inbox interest, cross-protocol classification, dedupe, ordering, relay
provenance, and typed `NNTF` sidecar under
`nmp.relations.notifications.<session>`.

Notifications include replies and mentions from kind:1 events, NIP-25
reactions, NIP-18 reposts, NIP-22 comments, and NIP-57 zap receipts that p-tag
the active account. TypeScript must decode the typed `NNTF` snapshot and render
rows, including Rust-projected read state. It must not construct notification
filters, maintain a parallel unread store, classify Nostr event kinds, or invent
source relay provenance.

Read/unread state is a Rust projection concern. The browser shell may send a
typed mark-read request for currently visible notification event ids or all
visible rows, but Rust must decide which rows are eligible and emit the updated
`NNTF` unread count and per-row read flags.

Acceptance must prove that opening Notifications sends a Rust-owned bounded
`#p=<active-account>` subscription; signed fixture interactions render with
source relay provenance; and marking visible rows read updates the
Rust-projected unread count without shell-local read storage.

## Private Messages Contract

Chirp Web must expose NIP-17 private messages as a first-level inbox workspace
for the active account. Rust owns kind:1059 gift-wrap interest reconciliation,
gift-wrap decrypt through the signer port, ordering, dedupe, outgoing-vs-incoming
classification, and the typed `NDMI` sidecar under `nmp.nip17.dm_inbox`.

TypeScript must decode the typed `NDMI` snapshot and render conversations. It
must not decrypt, construct private-event filters, fabricate local threads,
store plaintext outside the rendered snapshot, or infer private-message policy.
When no active account exists, the UI must render a signed-out state. When
`decrypt_state` is `limited`, the UI must surface `undecrypted_count` as state.

Outbound NIP-17 send is supported for browser local-key and NIP-46 sessions
through the typed `nmp.nip17.send` action. TypeScript may collect a recipient
pubkey and plaintext draft, but the action must cross the generated
`dispatch_bytes` builder; Rust owns recipient and self-copy kind:10050
relay-list lookup, gift-wrap construction, NIP-44 encryption, signing, explicit
relay routing, outbox diagnostics, and fail-closed errors. If either receiver's
DM relay list is missing, or if the active signer cannot satisfy the required
NIP-44/signing capabilities, the send must fail visibly through Rust action
state rather than falling back to public content relays or a shell-local
simulation.

NIP-07 and NIP-46 outbound private-message parity flows through the same
signer-provider boundary. The browser runtime parks and resumes async NIP-44
provider operations, and the NIP-07 signer bridge advertises NIP-44 only when
the extension exposes both `window.nostr.nip44.encrypt` and
`window.nostr.nip44.decrypt`. The UI may render the same send form, but it must
treat capability failures as product state and must not implement
private-message encryption or relay policy in TypeScript.

Source relay provenance must come from the Rust ingest dispatcher. Live relay
gift-wraps carry the delivering relay URL into the inbox projection; source-free
paths such as cache replay may render provenance as pending. TypeScript must
not fabricate relay names.

Acceptance must prove that signing in opens a real Rust-owned kind:1059 `#p` DM
inbox interest; a signed fixture gift-wrap decrypts through Rust into the typed
`NDMI` sidecar; the browser renders plaintext, peer pubkey, decrypt state, and
live source provenance without a shell-local message store; and a local-key
browser send dispatches typed `N17S` bytes so Rust publishes signed kind:1059
recipient/self-copy envelopes to the receivers' kind:10050 relays.

## Profile Open Contract

Chirp Web must let users open a visible author profile from feed and thread
cards. The profile surface must render Rust-resolved profile metadata when
available, the active-account follow state from `nmp.follow_list`, relay
provenance for hydrated author content, and authored posts already present in
Rust-owned feed projections.

The browser shell may filter the current projected feed rows for presentation,
but it must not maintain an independent author timeline, profile cache, contact
graph, or relay query plan. If the author has more content than the current
projection has hydrated, the UI must represent only the visible projected set
until a Rust-owned profile/feed workspace provides a broader author feed.

## Thread Open Contract

Chirp Web must let users open a visible feed note into a thread detail surface.
The browser shell may render the selected Rust-projected root, relation counts,
relay provenance, and Rust-emitted reply attribution rows from the existing
`nmp.feed.home` projection. TypeScript must not issue relay queries, infer NIP-10
thread membership, or maintain a parallel thread graph.

The current thread detail shows selected-note content, relation counters, relay
provenance, a reply composer, and visible reply attribution rows containing the
reply author, reply event id, and timestamp. Full reply-body hydration remains
pending until Rust emits a dedicated thread/read-model sidecar for browser
sessions.

Acceptance must prove that opening a fixture-fed thread renders reply
attribution from the typed feed projection and that publishing a reply still
uses the Rust-owned NIP-10 publish path.

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

The first-run publish proof follows the Rust terminal relay verdict, including
the verified same-relay event-echo fallback documented in the publish engine
builder guide.

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

## Storage And Replay Contract

Chirp Web must expose storage and replay health as a first-level product
workspace. The browser shell may render the store status, active interests, wire
subscriptions, relay inventory, and publish outbox rows already emitted by the
runtime snapshot, but it must not infer cache coverage, retry ownership,
subscription policy, or offline recovery state.

The Storage workspace must make the current limitation explicit: durable offline
publish replay is not complete until Rust owns the persisted queue and recovery
policy end to end. Until then, the workspace is an inspectable health surface,
not a promise that pending publishes survive reload or reconnect.

Acceptance must prove that opening Storage renders Rust-emitted relay,
interest, and wire-subscription diagnostics, and that the UI keeps the durable
offline publish limitation visible.

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
wallet/zap flows, moderation/WoT, group membership actions, or durable offline
replay ownership, the browser product must expose those destinations as blocked,
disabled, or explicitly partial workspaces with clear reasons. Private messages
are not globally blocked: they must render the live supported surface from the
Private Messages Contract above, and signer-specific NIP-44 gaps must surface as
capability-limited product state.

Blocked controls may emit log-safe `capability_failure` diagnostics proving the
unsupported state is deliberate. They must not construct Nostr events, maintain
shell-local unread counts, fabricate private message threads, simulate wallet
state, or persist policy choices in TypeScript. Deep routes `#messages`,
`#wallet`, and `#moderation` must either render the live supported surface or
focus the requested blocked destination. When any blocked area becomes
supported, the same navigation destination graduates to Rust-owned
projections/actions and browser acceptance for the real workflow.

## Content Rendering Contract

Chirp Web must render common kind:1 note content as a polished social card, not
as an undifferentiated text blob. URLs, hashtags, Nostr references, line breaks,
and image links should be visibly distinct and must preserve layout on desktop
and mobile.

This rendering is presentation only. TypeScript may tokenize already-projected
note text to create anchors and media previews, but it must not interpret those
tokens as protocol state, construct Nostr filters or events from them, or
fabricate hydration for referenced events. Any opened search, profile, thread,
or media workflow must still cross the existing Rust-owned runtime/action seams.
