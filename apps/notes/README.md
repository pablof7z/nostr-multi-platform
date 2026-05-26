# Notes — second-app stateful spike

A minimal NIP-01 note client built **entirely on the existing NMP substrate
seams** — kernel actor, generic dispatch, raw event observer, NIP-46 signer
broker. **Zero new C-ABI protocol symbols.**

This crate is the **stateful** counterpart to the read-only [Longform
spike](../longform/README.md):

| Spike    | Path       | Proves                                          | Rust LOC | New C-ABI symbols |
| -------- | ---------- | ----------------------------------------------- | -------- | ----------------- |
| Longform | read-only  | substrate enough for non-social readers         | ~152     | 2 (init + getter) |
| Notes    | stateful   | substrate enough for read + write + NIP-46 auth | ~25      | 1 (marker only)   |

## Verdict — framework thesis confirmed for stateful apps

The second app works. Every feature flows through generic seams already used
by Chirp:

| Feature                | Seam used                                                                |
| ---------------------- | ------------------------------------------------------------------------ |
| Read kind:1 timeline   | `nmp_app_register_raw_event_observer(app, ctx, cb, "[1]")`               |
| Publish kind:1 note    | `nmp_app_dispatch_action(app, "nmp.publish", PublishNote JSON)`          |
| Sign-in with nsec      | `nmp_app_signin_nsec(app, secret)`                                       |
| Sign-in with bunker    | `nmp_signer_broker_init(app)` + `nmp_app_signin_bunker(app, uri)`        |
| Bunker QR (nostrconnect)| `nmp_app_nostrconnect_uri(app, nil, nil)`                                |
| iOS scenePhase bridge  | `nmp_app_lifecycle_foreground(app)` / `nmp_app_lifecycle_background(app)`|

The `nmp_app_notes_init` symbol the Rust crate exports is a marker (empty
body) for the app-registration boundary — it is **not** a new protocol
seam. The Notes app would still link and run with the marker deleted; it
exists only so a future iteration that needs custom projection state has a
clean place to add it without changing the FFI shape.

## Layout

```
apps/notes/
├── nmp-app-notes/         # Rust composition shim (25 LOC code, ≤50 LOC budget)
│   ├── Cargo.toml         # depends on nmp-core + nmp-ffi
│   └── src/lib.rs         # `pub use` aggregator + nmp_app_notes_init marker
├── ios/Notes/             # SwiftUI iOS shell (299 LOC, ≤300 LOC budget)
│   ├── NotesApp.swift     # @main, scenePhase wiring
│   ├── Bridge/
│   │   ├── NmpCore.h      # verbatim copy of ios/Chirp/Chirp/Bridge/NmpCore.h
│   │   ├── Notes-Bridging-Header.h
│   │   └── NotesBridge.swift   # KernelHandle wrapper, generic seams only
│   ├── Models/NoteModel.swift  # raw NIP-01 event parser
│   └── Views/
│       ├── ContentView.swift   # TabView root + auth gate
│       ├── AuthView.swift      # nsec input + NIP-46 QR
│       ├── TimelineView.swift  # kind:1 feed
│       └── ComposeView.swift   # text editor + publish
└── README.md              # this file
```

## Building

The Rust crate is a workspace member; build it with:

```bash
cargo check -p nmp-app-notes
# For iOS simulator:
cargo build -p nmp-app-notes --target aarch64-apple-ios-sim
```

## iOS Xcode wire-up

The Swift sources are ready to drop into an Xcode project, but the project
file itself is intentionally not committed (it would force everyone running
`xcodegen` to regenerate UUIDs — see the existing "xcodegen pbxproj churn"
note in the repo memory). To run the app:

1. **Create a new iOS App project in Xcode**:
   - Product Name: `Notes`
   - Interface: SwiftUI
   - Language: Swift
   - Bundle Identifier: anything (e.g. `io.f7z.notes`)
   - Minimum Deployment: iOS 17 (for `@Observable`, `ContentUnavailableView`)

2. **Add the Swift sources**: drag `apps/notes/ios/Notes/` (Notes.app,
   Bridge/, Models/, Views/) into the project navigator. **Uncheck** the
   "Copy items if needed" box so the files stay in the repo.

3. **Configure the bridging header** under target → Build Settings:
   - `SWIFT_OBJC_BRIDGING_HEADER` = `apps/notes/ios/Notes/Bridge/Notes-Bridging-Header.h`
   - `HEADER_SEARCH_PATHS` += `apps/notes/ios/Notes/Bridge`

4. **Link the Rust archive**: under target → Build Phases → Link Binary With
   Libraries, add `target/aarch64-apple-ios-sim/debug/libnmp_app_notes.a`
   (use the device-arch path for device builds). Or set
   `OTHER_LDFLAGS` += `-L$(SRCROOT)/../../../target/aarch64-apple-ios-sim/debug -lnmp_app_notes`.

5. **Add a pre-build script phase** that runs:
   ```bash
   cd "$SRCROOT/../../.." && cargo build -p nmp-app-notes --target aarch64-apple-ios-sim
   ```

6. **Build & run** on iPhone simulator. On first launch the auth tab is
   shown; after a sign-in flow completes the timeline + compose tabs appear.

A `justfile` recipe analogous to `rust-ios-sim` could be added once the
spike graduates to a real product. For the spike — which exists to prove
the framework thesis, not to ship a polished app — the Swift code IS the
proof.
