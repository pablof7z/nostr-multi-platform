# Profile and Thread Subscription Slice

Date: 2026-05-17

## Goal

Prove that Swift UI interactions can ask the Rust backend to open new Nostr subscriptions, then render the resulting projections through the existing FFI view-batch path.

This slice adds two interaction-driven surfaces:

- Author profile view: tapping an author opens backend `kind:0` and author `kind:1/6` REQs.
- Thread view: opening a note asks the backend for the focused event, root/reply context ids, and replies tagged with the root event.

## Implementation

Rust core:

- Added C ABI commands:
  - `nmp_app_open_author(app, pubkey)`
  - `nmp_app_open_thread(app, event_id)`
- Added actor commands for author and thread views.
- Stored event tags in the Rust event cache so thread root/reply relationships can be derived after FFI crossing.
- Added author-view JSON payloads:
  - selected pubkey
  - profile card
  - backend-projected author notes
  - state and note count
- Added thread-view JSON payloads:
  - focused event id
  - root event id
  - chronological thread items
  - previous/next counts relative to the focused note
- Added logical-interest rows for selected author and selected thread.
- Added wire subscription rows for the triggered REQs.

Swift app:

- Added `KernelHandle.openAuthor(pubkey:)` and `KernelHandle.openThread(eventID:)`.
- Added decoded `authorView` and `threadView` payloads.
- Added a profile detail screen that calls Rust on appear and renders backend-projected notes.
- Added a thread detail screen that calls Rust on appear and renders previous replies, selected note, and next replies.
- Updated the UI test to open timeline -> profile -> thread through the real app.

## Backend REQs

Opening an author emits:

- `author-profile-N`: `{"kinds":[0],"authors":[pubkey],"limit":1}`
- `author-notes-N`: `{"kinds":[1,6],"authors":[pubkey],"limit":100}`

Opening a thread emits:

- `thread-ids-N`: `{"ids":[focused, root, reply refs],"limit":20}`
- `thread-replies-N`: `{"kinds":[1,6],"#e":[root],"limit":200}`

Unit coverage now asserts those REQs are produced by the backend command handlers.

## Validation

Commands run:

- `cargo fmt --all`
- `cargo test --workspace`
- `cargo build -p nmp-core --target aarch64-apple-ios-sim`
- `xcodegen generate --spec ios/NmpStress/project.yml`
- XcodeBuildMCP `build_run_sim`
- XcodeBuildMCP `test_sim`
- `cargo run -p nmp-testing --bin reactivity-bench -- --standard --fail-on-gate`
- `cargo run -p nmp-testing --bin firehose-bench -- replay --standard --fail-on-gate`
- `git diff --check`

Results:

- Rust tests: passed, including 2 new backend subscription trigger tests.
- iOS simulator build/run: passed.
- iOS UI test: passed.
- Reactivity bench: passed all standard gates.
- Firehose replay bench: passed all standard gates.

Final UI test metric line:

```text
NMP_REAL_RELAY_METRICS relay=CONNECTED events=151 visible=80 rx=310 KB first_ms=429ms apply_us=155 profile_notes=92
```

Artifacts:

- UI test log: `/Users/pablofernandez/Library/Developer/XcodeBuildMCP/workspaces/nostr-multi-platform-670fb45eb2a8/logs/test_sim_2026-05-17T20-46-15-032Z_pid5193_325791ac.log`
- UI test xcresult: `/Users/pablofernandez/Library/Developer/XcodeBuildMCP/workspaces/nostr-multi-platform-670fb45eb2a8/result-bundles/test_sim_2026-05-17T20-46-15-032Z_pid5193_8431f243.xcresult`
- Manual simulator runtime log: `/Users/pablofernandez/Library/Developer/XcodeBuildMCP/workspaces/nostr-multi-platform-670fb45eb2a8/logs/com.example.NmpStress_2026-05-17T20-47-56-291Z_helperpid86295_ownerpid5193_b1cfb9e5.log`
- Manual screenshot: `/var/folders/bl/w2vvyf7n0sq2vrh10pg8bd4h0000gn/T/screenshot_optimized_5cbb39c9-d18a-4306-8528-9fcfcdc5b8ab.jpg`

## Observations

This proves the important architecture point: Swift is no longer only filtering whatever timeline data already crossed the FFI bridge. A view can ask Rust for a new logical interest, Rust can decide which wire REQs to open, and Swift renders the resulting projection from the next JSON view batches.

The thread view is currently one-hop hydration. It fetches the focused event, explicit root/reply refs already known from tags, and replies tagged to the root. It does not recursively chase every missing reply branch yet.

Profile view still uses the single Primal content relay. This means the known target profile gap remains: the pablo test pubkey's `kind:0` is not reliably available from Primal. The author profile view works for many timeline authors because their profiles arrive from the timeline/profile enrichment path, but robust profile lookup needs the indexer/outbox role.

The lifecycle is open-only for this proof. The backend does not yet close selected-author/thread wire subscriptions when Swift pops the screen, nor does it refcount multiple component-level interests.

The app still has one Rust cache and one current Swift projection payload. The proposed iPhone TTL cache is not implemented yet.

## Relay Role Follow-Up

The Highlighter/NDK review found the relevant pattern: NDK uses `purplepag.es` as an outbox/indexer discovery pool, while content reads stay on content relays such as Primal/Damus.

Next implementation step should split the relay layer into roles:

- `Content`: current `wss://relay.primal.net` timeline/thread content reads.
- `Indexer`: `wss://purplepag.es` for `kind:10002`, `kind:3`, and fallback `kind:0`.
- Later: write/inbox/media roles.

The first concrete version should parse NIP-65 `kind:10002` into an author relay-list cache and use it to route future author-specific profile/thread reads, while keeping Primal as the bounded demo fallback.

## Next Milestones

1. Add `RelayRole` and multi-relay diagnostics.
2. Use `purplepag.es` for NIP-65/indexer reads and profile fallback.
3. Add subscription lifecycle close/refcounting for author/thread views.
4. Add recursive thread hydration with missing-ref backfill.
5. Add the iOS TTL projection cache so repeated renders do not require new FFI reads.
