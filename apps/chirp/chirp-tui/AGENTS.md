# chirp-tui — Agent Guidance

## E2E testing requirement

Every new feature or user-visible behavior change MUST include an end-to-end
test using `rexpect` before the PR is opened. Render snapshot tests and unit
tests on `AppState` are complementary but do not replace this requirement.
Default CI e2e tests must be deterministic: do not make ordinary workspace
tests depend on public-relay publish settlement.

## How e2e tests work here

The app runs as a real binary in a PTY. Tests spawn it, send key sequences, and assert on observable output — primarily the status bar string, which changes synchronously with every action.

Add e2e tests to `tests/e2e.rs`. Add `rexpect` and `vt100` to `[dev-dependencies]` in `Cargo.toml`:

```toml
[dev-dependencies]
rexpect = "0.7.1"
vt100 = "0.16.2"
insta = { version = "1", features = ["yaml"] }
```

Run tests scoped to this crate only — never `cargo test` at workspace root:

```
cargo test -p chirp-tui
```

## Critical: PTY window size must be set immediately after spawn

**Without explicit window sizing, ratatui sees a 0-column terminal and renders nothing.** This was confirmed empirically — the binary exits silently with no output unless the PTY dimensions are set.

```rust
use rexpect::spawn_with_options;

let mut p = spawn_with_options(
    "cargo run -p chirp-tui -- --relay wss://relay.damus.io",
    Some(8_000),
    None,
)?;
// MANDATORY — set before the first draw tick
p.set_window_size_pixels(120, 40)?;  // (cols, rows)
```

If `rexpect` 0.7.1 does not expose `set_window_size_pixels`, fall back to the `nix::ioctl_write_ptr` / `TIOCSWINSZ` approach or shell out to `stty rows 40 cols 120` on the slave PTY fd.

## Status bar is the primary test oracle

The status bar at the bottom-left of the screen updates synchronously with every user action. Its content is predictable and stable, making it the best assertion target. Examples:

| User action | Status bar text |
|---|---|
| startup | `starting NMP runtime` |
| first relay update | `received update #1 (N bytes)` |
| `i` (enter compose) | `compose note: Ctrl+Enter publishes, Esc cancels` |
| `Esc` (cancel compose) | `compose canceled` |
| `r` with nothing selected | `select a note before replying` |
| `Enter` with nothing selected | `select a note before opening a thread` |
| `+` with nothing selected | `select a note before reacting` |

Match on these strings with `p.exp_string("compose note:")` or `p.exp_regex(r"received update #\d+")`.

## Relay setup for tests

Use a deterministic relay fixture, local relay, or state/render harness for
default CI paths. Public relays are acceptable only for ignored opt-in/nightly
tests because publish settlement can stay in active/backoff state long enough
to flake ordinary workspace runs.

For public-relay smoke tests, `wss://relay.damus.io` works and was verified
live. Wait for the first snapshot update before asserting on note content —
content arrives asynchronously:

```rust
// Wait up to 8 s for the first relay snapshot
p.exp_regex(r"received update #\d+")?;
```

For deterministic content tests, run a local `strfry` or `nostr-rs-relay`
instance and seed it with known fixture events before the test. There is
currently no local relay in this repo — add one under `tests/fixtures/` if you
need deterministic wire content.

### Why real relays and not mocks

`AppRuntime` wraps the NMP C-ABI directly with no mockable trait. End-to-end tests with a real relay are the only way to exercise the full path: FFI → actor → relay wire → snapshot → render → keybind → dispatch_action → relay wire. A mock would silently omit every one of those seams.

## What the app renders (verified)

The layout (confirmed empirically at 120×40):

```
chirp  [home][mentions][dms][groups]
┌Feed──────────┐ ┌Detail──────────────────┐ ┌Profile────────┐
│ cards:N       │ │ Note/Thread            │ │ Profile/Detail│
│ <note rows>   │ │ <thread content>       │ │               │
└──────────────┘ └────────────────────────┘ └───────────────┘
┌Compose──────────────────────────────────────────────────────┐
│ i compose  r reply  + react  f follow  F unfollow            │
└─────────────────────────────────────────────────────────────┘
<status text>   updates:N   q quit   1/2/3 focus
```

## Three-layer test strategy

| Layer | Tool | What it covers | Relay needed? |
|---|---|---|---|
| State unit tests | `#[test]` in `app.rs` | navigation, compose, selection logic | no |
| Render snapshots | `ratatui::backend::TestBackend` + `insta` | layout, cell content | no |
| E2E (REQUIRED) | `rexpect` + deterministic relay when the flow depends on relay settlement | full round-trip through NMP FFI | yes |
| Live smoke (opt-in) | ignored `rexpect` + public relay | public relay compatibility | yes |

Unit and snapshot tests live alongside the source. E2E tests live in `tests/e2e.rs`.
