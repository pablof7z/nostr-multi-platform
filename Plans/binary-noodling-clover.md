# Chirp iOS — top-bar search / go-to box

## Context

The owner wants a **search button at the very top of the Home screen, next to the
user's avatar**. Tapping it opens a box where they can paste/type one string and
be taken somewhere:

- a NIP-19 entity (`npub` / `nprofile` / `nevent` / `note`) → jump to that profile or thread
- a `#hashtag` → open a hashtag feed
- (fast-follow, not this PR) a NIP-05 `name@domain` → jump to that profile
- (fast-follow, not this PR) free text → NIP-50 full-text search

**Scope decided with owner:** build *paste-to-navigate + #hashtag* now. NIP-05
resolution and NIP-50 freetext search are a fast-follow (they need a brand-new
kernel→snapshot→FFI→view stack — `nmp-nip50` is currently a dead island, and no
reverse NIP-05 resolver exists). **`naddr` is skipped entirely for now** (Chirp
is a microblog with no addressable/long-form reader view).

The box's classifier is built Rust-side **once** and already understands the
fast-follow cases, so landing NIP-05/NIP-50 later is a Swift-routing change only.

Doctrine note: Chirp is a thin shell (zero DOMAIN logic — aim.md §2 #4). **All** query parsing /
classification lives in Rust; Swift only switches on a typed result.

## What already exists (reuse, don't rebuild)

- `nmp_nip21_decode_uri(input)` — `crates/nmp-ffi/src/nip21_ffi.rs`; stateless decode of
  nip19/nip21 → `{ok, target: profile|event|address, pubkey, event_id, relays, …}`. Already
  declared in `ios/Chirp/Chirp/Bridge/NmpCore.h:165`.
- `nmp_app_chirp_open_tag_feed(app, tag)` — `apps/chirp/nmp-app-chirp/src/ffi/tag_feed.rs`;
  normalizes the tag and opens `{"kinds":[1],"#t":[tag]}` global interest. **Incomplete** — see below.
- Flat-feed projection pattern — `apps/chirp/nmp-app-chirp/src/ffi/interest_feed.rs`
  (author/thread feeds): `FlatFeed` + `PullFeedController` + typed sidecar registered under
  `nmp.feed.<type>.<key>`, read by the view via `model.flatFeeds[key]`.
- Render components — `ios/Chirp/Chirp/Components/ProfileNoteRow.swift` (+ `NoteRenderContext`);
  `ProfileView.swift` is the screen template (open feed on `.task`, close on `.onDisappear`,
  render `snapshot.cards` with `ProfileNoteRow`).
- Routing — `ChirpRoute` / `ChirpRouter` / `TabStack` in `ios/Chirp/Chirp/App/RootShell.swift`;
  `.profile(pubkey)` and `.thread(eventID)` already wired.

## Plan

### A. Rust — finish the hashtag feed projection

`nmp_app_chirp_open_tag_feed` today only opens the wire interest; tag events have **nowhere to
render**. Make it mirror `interest_feed.rs`:

1. `crates/nmp-nip01/src/flat_feed.rs` — add `tag_feed_predicate(tag: String, kinds: Vec<u32>)`
   (admit event when `event.kind ∈ kinds` AND a `#t` tag equals `tag`). Export from
   `crates/nmp-nip01/src/lib.rs` next to `author_feed_predicate` / `thread_feed_predicate`.
2. `apps/chirp/nmp-app-chirp/src/ffi/helpers.rs` — add `tag_feed_shape(tag, kinds)` (mirror
   `author_feed_shape`; an `InterestShape` with the `#t` tag map + kinds for `PullFeedController`).
3. `apps/chirp/nmp-app-chirp/src/ffi/tag_feed.rs` — rewrite `nmp_app_chirp_open_tag_feed` to,
   after opening the interest: build `FlatFeed::new(tag_feed_predicate(tag, [1]))`, register via
   `register_feed_with_observer(tag_feed_key(tag), pull_ctrl, feed)` +
   `register_typed_feed_sidecar` under `nmp.feed.tag.<tag>` (key helper `fn tag_feed_key`).
   Add `nmp_app_chirp_close_tag_feed(app, tag)` = `unregister_feed` + `close_interest`
   (mirror `close_author_feed`). Store-seeding is best-effort/optional (a public tag fills from
   relays; skip the store seed for v1 unless a `#t` `StoreQuery` is trivially available).
   Keep existing tag-normalization + consumer-id + tests; add open/close projection tests.

### B. Rust — the go-to classifier (thin-shell chokepoint)

4. `crates/nmp-core/src/query.rs` (new) — pure `classify_query(&str) -> QueryClass` reusing
   `nmp_core::nip19` + `nmp_core::nip21`. Rules, in order: `nostr:`/bech32 entity →
   `Profile`/`Event`/`Address`/`Rejected(nsec)`; starts with `#` or is a single bare tag-word →
   `Hashtag(tag)`; matches `local@domain.tld` shape → `Nip05(identifier)`; else `Freetext(query)`.
   Register module in `crates/nmp-core/src/lib.rs`. Unit-test each branch.
5. `crates/nmp-ffi/src/query_classify_ffi.rs` (new) — `nmp_app_search_classify(input) -> *mut c_char`,
   stateless, modeled on `nip21_ffi.rs`. Returns tagged JSON:
   `{"kind":"profile","pubkey","relays"}` · `{"kind":"event","event_id","relays","author?","kind?"}`
   · `{"kind":"hashtag","tag"}` · `{"kind":"nip05","identifier"}` · `{"kind":"search","query"}`
   · `{"kind":"unsupported","reason"}` (naddr/nsec). Never NULL (D6). Register in
   `crates/nmp-ffi/src/lib.rs` and `#[allow]` the C-ptr-deref lint as the sibling does.

### C. Swift — bridge

6. `ios/Chirp/Chirp/Bridge/NmpCore.h` — declare `nmp_app_chirp_open_tag_feed`,
   `nmp_app_chirp_close_tag_feed`, `char *nmp_app_search_classify(const char *input)`.
7. `ios/Chirp/Chirp/Bridge/KernelBridge.swift` — `openTag(tag:)` / `closeTag(tag:)` wrappers;
   `classify(query:) -> SearchClassification` (calls `nmp_app_search_classify`, frees via
   `nmp_free_string`, JSON-decodes).
8. `ios/Chirp/Chirp/Bridge/KernelModel.swift` — `openTag/closeTag` + `tagFeed(tag:) ->
   ChirpTimelineSnapshot?` reading `flatFeeds["nmp.feed.tag.\(tag)"]` (mirror `authorFeed`).
9. `SearchClassification` — `Codable enum` mirroring the JSON `kind` tag (new small file or in
   `ChirpActionSpecBridge.swift` alongside `InterestScope`).

### D. Swift — UI

10. `ios/Chirp/Chirp/App/RootShell.swift` — add `case hashtag(tag: String)` to `ChirpRoute`;
    in `TabStack.navigationDestination`, `.hashtag(let t): HashtagFeedView(tag: t)`.
11. `ios/Chirp/Chirp/Features/HashtagFeedView.swift` (new) — model on `ProfileView`: `.task {
    model.openTag(tag:) }`, `.onDisappear { model.closeTag(tag:) }`, render
    `model.tagFeed(tag:)?.cards` with `ProfileNoteRow` (avatar tap → `.profile`, row tap →
    `.thread`), `navigationTitle("#\(tag)")`, empty/placeholder state.
12. `ios/Chirp/Chirp/Features/SearchSheet.swift` (new) — sheet with autofocused, no-autocap /
    no-autocorrect `TextField` + paste affordance + submit. On submit: `model.classify(query:)`,
    then dismiss and route: `profile→.profile`, `event→.thread`, `hashtag→.hashtag`;
    `nip05`/`search` → inline "Coming soon" note (tailored copy); `unsupported` → inline error.
13. `ios/Chirp/Chirp/Features/HomeFeedView.swift` — add a second `.navigationBarLeading`
    `ToolbarItem` (renders right of the avatar) with a `magnifyingglass` button + accessibility
    label "Search"; `@State private var showSearch` drives `.sheet { SearchSheet() }`. Route via
    the existing `@EnvironmentObject router`.

## Verification

- Rust: `cargo test -p nmp-nip01 -p nmp-app-chirp -p nmp-ffi -p nmp-core` +
  `cargo test -p nmp-testing --test doctrine_lint_smoke` (D0 — `nmp-core` must not name nip01/nip50;
  classifier stays substrate-pure).
- File-size gate: `tools/check-file-size.sh --from-ref origin/master --to-ref HEAD --baseline-ref origin/master`.
- iOS: build + run Chirp on the sim (xcode MCP). Manually confirm the top-bar magnifier sits next
  to the avatar and:
  - paste an `npub`/`nprofile` → Profile screen opens for that user;
  - paste a `note`/`nevent` → Thread screen opens;
  - type `#nostr` (or `nostr`) → Hashtag feed opens and fills with kind:1 notes;
  - type `alice@example.com` → "NIP-05 lookup coming soon"; type free text → "Search coming soon";
  - paste an `naddr`/`nsec` → graceful "not supported" message (no crash).

## Build order / fan-out

Tight Rust↔Swift contract (C symbols + classify JSON), so land **A+B (Rust) first**, then **C+D
(Swift)**. Within Rust, A (tag feed) and B (classifier) are independent → parallelizable in one
worktree. Single PR to `master` (downstream pins by git rev; keep it coherent).
