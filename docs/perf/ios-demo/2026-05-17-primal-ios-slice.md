# NMP Demo iOS Primal Slice Report

Date: 2026-05-17

## Summary

Implemented the first real iOS runtime slice for ADR-0008: a SwiftUI app backed by Rust core state that connects to `wss://relay.primal.net`, opens real Nostr subscriptions, parses relay frames in Rust, materializes a profile/timeline projection, exposes ADR-0007 diagnostics, and renders on an iOS simulator.

This is no longer a synthetic event generator. The iOS app now receives live relay traffic and shows:

- The requested slice npub: `npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft`.
- Seed dev accounts: pablof7z, fiatjaf, jb55.
- A seed-driven timeline from the union of those accounts' follow lists.
- Relay status, logical interest status, wire subscription status, and runtime logs.
- Swift decode/apply timing, payload bytes, network bytes, event counts, and visible row counts.

## Implementation

### Rust core

File: `crates/nmp-core/src/lib.rs`

Replaced the synthetic stress actor with a real relay-backed kernel while preserving the existing C ABI used by the iOS app.

The actor now:

- Connects to `wss://relay.primal.net` using `tungstenite` + `rustls`.
- Installs the `rustls` ring crypto provider explicitly for iOS simulator compatibility.
- Sends Nostr `REQ` messages for:
  - target profile kind:0 for the requested npub,
  - seed contacts kind:3 for pablof7z, fiatjaf, jb55,
  - seed profiles kind:0,
  - seed timeline kinds:1 and 6 over the unioned follow set,
  - serialized batches of visible author kind:0 profile enrichment.
- Parses `EVENT`, `EOSE`, `NOTICE`, `CLOSED`, and `OK` frames.
- Stores kind:0 profiles with replaceable supersession.
- Stores kind:1 and kind:6 timeline events.
- Parses seed kind:3 follow lists and caps the timeline author set at 500 authors.
- Emits bounded JSON updates to Swift via the Rust-owned update callback.
- Emits `NMP_CORE` runtime logs to stderr so XcodeBuildMCP captures networking history.

Important correction made during validation:

- The first implementation opened too many concurrent profile enrichment REQs and primal returned `NOTICE ERROR: too many concurrent REQs`.
- The actor now serializes profile enrichment requests and closes one-shot REQs after EOSE.

### iOS app

Files:

- `ios/NmpStress/NmpStress/KernelBridge.swift`
- `ios/NmpStress/NmpStress/KernelModel.swift`
- `ios/NmpStress/NmpStress/ContentView.swift`
- `ios/NmpStress/NmpStressUITests/NmpStressUITests.swift`

The app now renders:

- A connection/status header.
- Runtime metrics.
- A profile card for the requested npub.
- A real seed-driven timeline.
- A diagnostics view with relay state, logical interests, wire subscriptions, and runtime logs.
- Author detail navigation from visible timeline rows.

The UI test now launches the app, waits for real primal connectivity, verifies live events and visible rows, opens Diagnostics, returns to Timeline, and scrolls.

## Verification

Commands and tools run:

| Check | Result |
|---|---|
| `git pull` | Pulled `735bd96`, adding ADR-0008 and plan updates. |
| `cargo fmt --all` | Passed. |
| `cargo test --workspace` | Passed. |
| `cargo build -p nmp-core --target aarch64-apple-ios-sim` | Passed. |
| `xcodegen generate --spec ios/NmpStress/project.yml` | Passed. |
| XcodeBuildMCP `build_run_sim` | Passed, launched on iPhone 17 simulator. |
| XcodeBuildMCP `test_sim` | Passed, 1 UI test. |
| `reactivity-bench --standard --fail-on-gate` | Passed. |
| `firehose-bench replay --standard --fail-on-gate` | Passed, still modeled. |
| `git diff --check` | Passed. |

XcodeBuildMCP artifacts from the final run:

- Build/run log: `/Users/pablofernandez/Library/Developer/XcodeBuildMCP/workspaces/nostr-multi-platform-670fb45eb2a8/logs/build_run_sim_2026-05-17T20-24-32-529Z_pid5193_1696615c.log`
- Runtime networking log: `/Users/pablofernandez/Library/Developer/XcodeBuildMCP/workspaces/nostr-multi-platform-670fb45eb2a8/logs/com.example.NmpStress_2026-05-17T20-24-35-808Z_helperpid33125_ownerpid5193_f8c181a0.log`
- UI test log: `/Users/pablofernandez/Library/Developer/XcodeBuildMCP/workspaces/nostr-multi-platform-670fb45eb2a8/logs/test_sim_2026-05-17T20-25-08-903Z_pid5193_45997970.log`
- UI test result: `/Users/pablofernandez/Library/Developer/XcodeBuildMCP/workspaces/nostr-multi-platform-670fb45eb2a8/result-bundles/test_sim_2026-05-17T20-25-08-904Z_pid5193_9a69cf17.xcresult`
- Screenshot: `/var/folders/bl/w2vvyf7n0sq2vrh10pg8bd4h0000gn/T/screenshot_optimized_3f4ca823-899c-4df7-b7c2-24ef1674c5d0.jpg`

## Measured Results

### Xcode UI test

Final real-relay UI test output:

```text
NMP_REAL_RELAY_METRICS relay=CONNECTED events=221 visible=80 rx=385 KB first_ms=372ms apply_us=194
```

The test passed in 11.705 seconds.

### Manual simulator run

After roughly 10 seconds on the iPhone 17 simulator:

| Metric | Value |
|---|---:|
| Relay state | CONNECTED |
| Active wire REQs | 1 |
| Timeline authors | 500 |
| Events received | 219 |
| Visible rows | 80 |
| Profiles loaded | 24 |
| Payload size | 52 KB |
| Network RX | 400 KB |
| First event | 586 ms |
| Max Swift apply | 157 us |
| Process RSS | 218,160 KB |

### Runtime networking log

Final run confirmed the intended sequence:

```text
NMP_CORE 23:24:36 connecting to wss://relay.primal.net
NMP_CORE 23:24:36 relay connected
NMP_CORE 23:24:36 seed account: pablof7z fa984b..018f52
NMP_CORE 23:24:36 seed account: fiatjaf 3bf0c6..fa459d
NMP_CORE 23:24:36 seed account: jb55 32e182..68e245
NMP_CORE 23:24:36 REQ profile-target: target kind:0 profile
NMP_CORE 23:24:36 REQ seed-contacts: seed kind:3 contacts
NMP_CORE 23:24:36 REQ seed-profiles: seed kind:0 profiles
NMP_CORE 23:24:37 opening seed timeline with 500 authors
NMP_CORE 23:24:37 REQ seed-timeline: seed union timeline kinds:1,6
NMP_CORE 23:24:40 EOSE seed-timeline
```

No panic or OS log fault was present in the final run.

### Benchmarks

Latest reactivity report:

- `docs/perf/reactivity-bench/1779049354-run-002.md`
- Overall passed.
- Hashtag firehose: 20,000 raw deltas coalesced to 589, max 58.90 deltas/view/sec.
- Working set: 1M cached events / 10k hot / 100 views modeled at about 19.8 MB.

Latest firehose replay report:

- `docs/perf/firehose-bench/1779049355-replay.md`
- Overall passed.
- Still a prototype modeled replay, not runtime evidence.

## Findings

The real iOS slice is viable:

- Rust can link into the simulator with TLS/WebSocket support.
- Rust can own the relay frame stream, Nostr parsing, projection state, and diagnostics.
- Swift can remain a renderer and performance measurement surface.
- The seed-driven timeline approach works against primal and reaches 500 authors quickly.
- The ADR-0007 diagnostics shape is useful in practice: it exposed the over-eager profile REQ planner bug immediately.

Important runtime observations:

- The target npub's kind:0 did not come back from `wss://relay.primal.net` during direct kind:0 lookup. The app still uses the npub as the profile slice key and seed contact source, but the profile card stays on the deterministic placeholder when primal does not return that kind:0.
- The seed contacts did come back from primal:
  - pablof7z: 500 followees captured by the cap.
  - fiatjaf: 194 followees.
  - jb55: 500 followees captured by the cap.
- Profile enrichment should be planner-controlled. Naively issuing one REQ per visible batch can hit relay limits; serialized one-shot profile batches fixed the immediate issue.
- The app still uses a C ABI staticlib bridge, not UniFFI. That was the fastest way to turn the existing local scaffold into a real iOS runtime slice. UniFFI remains a required ADR-0008 step.

## What Is Not Implemented Yet

This is a real read-path slice, not the full 1a.6 Twitter-clone:

- No LMDB persistence yet.
- No UniFFI-generated bindings yet.
- No login, signer, keychain, compose, reply, or like actions yet.
- No NIP-10 thread view yet.
- No pagination beyond the first 200 timeline events.
- No outbox routing; relay is still hardcoded to primal.
- No NIP-77 sync; REQ only.
- No signature verification or delegation validation on inbound events yet.

## Recommended Next Steps

1. Keep this iOS app as the ADR-0008 proof app and rename the target from `NmpStress` to `NmpDemo`.
2. Implement a proper subscription planner queue: bounded concurrent REQs, close one-shot REQs, retry on relay NOTICE, and expose queue depth in diagnostics.
3. Add a profile fallback strategy for the requested npub: either use the owner's actual profile relay set or add a relay-target resolver for profile lookups before insisting on primal-only.
4. Replace the C ABI bridge with UniFFI once the read-path API is stable.
5. Add LMDB or SQLite persistence next, then remeasure cold-start and restart behavior.
6. Make `firehose-bench live` drive this real iOS/Rust path instead of only the modeled replay.
