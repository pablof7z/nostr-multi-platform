# Relay Roles, Lifecycle, and Swift Projection Cache

Date: 2026-05-17

## Goal

Take the profile/thread subscription proof one step further:

- split relay responsibilities into content vs indexer roles
- use `purplepag.es` for discovery reads
- add view close/refcount lifecycle
- recursively hydrate thread context
- let Swift render profile/thread projections from a short-lived local cache

## Implemented

Rust core:

- Split the core into smaller modules:
  - `actor.rs`
  - `ffi.rs`
  - `relay.rs`
  - `kernel/*`
- Added `RelayRole`:
  - `Content`: `wss://relay.primal.net`
  - `Indexer`: `wss://purplepag.es`
- Routed startup discovery reads through the indexer:
  - target `kind:0`
  - target `kind:10002`
  - seed `kind:3`
  - seed `kind:0`
  - seed `kind:10002`
- Kept content reads on Primal:
  - seed bootstrap timeline
  - expanded timeline
  - author notes
  - thread ids/replies
- Added `seed-bootstrap` so the demo paints quickly from the known seed authors while `purplepag.es` discovery warms the broader timeline.
- Parsed NIP-65 `kind:10002` relay lists into an author relay-list cache.
- Added `relay_statuses` to the FFI payload so the app can display content/indexer status separately.
- Added view lifecycle FFI:
  - `nmp_app_close_author`
  - `nmp_app_close_thread`
- Added backend refcounting for author/thread view interests.
- Added close-message generation for open author/thread wire subscriptions.
- Added recursive thread hydration:
  - request focused/root/reply ids
  - request replies for root/focused/referenced events
  - enqueue newly discovered thread refs for follow-up ids/replies

Swift app:

- Added close methods to `KernelHandle` and `KernelModel`.
- Added `relayStatuses` decoding and a multi-relay diagnostics section.
- Added a 60 second in-memory TTL cache for author and thread projections.
- Profile/thread screens now render from the latest matching FFI payload or the Swift TTL cache.
- Profile/thread screens call close on disappear.

Bench/test cleanup:

- Fixed `firehose-bench` missing `SystemTime`/`UNIX_EPOCH` import so the replay gate compiles cleanly.

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

- Rust tests: passed, including 4 kernel tests.
- iOS simulator build/run: passed.
- iOS UI test: passed.
- Reactivity bench: passed all standard gates.
- Firehose replay bench: passed all standard gates.

Final UI test metric line:

```text
NMP_REAL_RELAY_METRICS relay=CONNECTED events=26 visible=27 rx=26 KB first_ms=745ms apply_us=182 profile_notes=59
```

Relevant runtime log evidence:

```text
NMP_CORE 00:01:26 connecting content relay wss://relay.primal.net
NMP_CORE 00:01:27 connecting indexer relay wss://purplepag.es
NMP_CORE 00:01:27 REQ seed-bootstrap@content: seed author bootstrap timeline
NMP_CORE 00:01:27 REQ profile-target@indexer: target kind:0 profile via indexer
NMP_CORE 00:01:27 REQ target-relays@indexer: target NIP-65 relay list
NMP_CORE 00:01:27 REQ seed-contacts@indexer: seed kind:3 contacts via indexer
NMP_CORE 00:01:27 REQ seed-profiles@indexer: seed kind:0 profiles via indexer
NMP_CORE 00:01:27 REQ seed-relays@indexer: seed NIP-65 relay lists
NMP_CORE 00:01:30 REQ seed-timeline@content: seed union timeline kinds:1,6
```

Artifacts:

- UI test log: `/Users/pablofernandez/Library/Developer/XcodeBuildMCP/workspaces/nostr-multi-platform-670fb45eb2a8/logs/test_sim_2026-05-17T21-01-29-379Z_pid5193_79609ed3.log`
- UI test xcresult: `/Users/pablofernandez/Library/Developer/XcodeBuildMCP/workspaces/nostr-multi-platform-670fb45eb2a8/result-bundles/test_sim_2026-05-17T21-01-29-380Z_pid5193_1dc812e0.xcresult`
- Manual runtime log: `/Users/pablofernandez/Library/Developer/XcodeBuildMCP/workspaces/nostr-multi-platform-670fb45eb2a8/logs/com.example.NmpStress_2026-05-17T21-01-26-262Z_helperpid12716_ownerpid5193_8e4a0125.log`

## Current Boundaries

This is now a real role split, but still with a bounded configured relay set. The kernel parses NIP-65 relay lists and surfaces them in diagnostics/cache coverage, but it does not yet open arbitrary per-author dynamic relay sockets from those lists.

Thread hydration is recursive for discovered ids/reply targets, but still bounded by request batching and local caps. That is intentional for the simulator slice.

Swift has a TTL cache for projections, not durable storage. Rust remains the source of truth.

## Next

The next implementation step is dynamic author relay routing: use the NIP-65 cache to open per-author content relays for selected profiles/threads, with connection caps and diagnostics that show which relay role made each routing decision.
