# nmp-desktop — in-process kernel shell

`crates/nmp-desktop` is a native desktop binary (egui/eframe) that runs the
NMP kernel **in-process**: Rust calling Rust, no FFI seam. It is the fastest
"I can actually use NMP" artifact because it skips the FFI boundary entirely.

## How it works

```
main.rs        eframe boot (900x720 native window)
  └─ app.rs    egui App: status bar · scrollable timeline · compose/sign-in
       ├─ bridge.rs   nmp_core::testing::spawn_actor()  →  (Sender<ActorCommand>, Receiver<String>)
       │               • sends ActorCommand::Start { visible_limit: 80, emit_hz: 4 }
       │               • reader thread parses JSON KernelUpdate snapshots,
       │                 parks the freshest behind a Mutex, requests repaint
       ├─ snapshot.rs deserialize-only mirror of the kernel envelope (D7)
       └─ render.rs   walks nmp_content::ContentTree segments natively
```

Doctrine: UI owns no state beyond the latest snapshot (D7); no app nouns added
to `nmp-core` — the generic `ActorCommand`/snapshot surface is consumed as-is
(D0); rendering is best-effort (D1); no panics across the in-process seam (D6).

## ContentTreeWire / ADR-0018 note

The build brief referenced an `nmp-content::ContentTreeWire` projection landed
under ADR-0018. No such symbol exists in the tree, and there is no ADR-0018
file. `nmp-content` deliberately keeps `Segment` non-serde and documents that
cross-process serialization is **out of scope** for in-process consumers
(`segment.rs` header). A Rust→Rust shell needs no wire projection, so the shell
consumes `ContentTree` / `Segment` directly via `tokenize_with_kind` — the
doctrine-correct path. This is flagged for whoever owns the nmp-content brief.

## Verification

- `cargo build -p nmp-desktop` — clean (1m12s cold).
- `cargo clippy -p nmp-desktop --all-targets -- -D warnings` — clean, 0 warnings.
- `cargo test -p nmp-desktop --test live_feed -- --ignored` — headless proof
  that the in-process kernel connects to `wss://relay.primal.net` and a live
  timeline arrives within 30s (GUI windows cannot open in the CI/agent
  environment, so the kernel+relay path is proven without egui).

  ```
  NMP_CORE EOSE seed-bootstrap
  NMP_CORE NMP_PERF rust_update rev=5 batch_events=90 inserted=80 visible=80
  live feed: items=80 events_rx=91
  test in_process_kernel_renders_live_feed ... ok  (finished in 2.84s)
  ```

## GUI

`cargo run -p nmp-desktop` opens the native window on a real desktop:
top status bar (relay health + rx/note/visible counters), a scrollable
timeline of avatar-initial cards rendering note bodies through `nmp-content`,
and a bottom compose bar with create-account / nsec sign-in and Publish.
Screenshots require an interactive display and are captured on a desktop host.
