# chirp-tui — Product Spec

> A polished, power-user Nostr TUI. Ratatui + tokio + NMP Rust core. Inline avatars,
> display-name resolution, vim-modal input, animated sparklines, iTerm2-native images.

---

## 1. Vision & Goals

`chirp-tui` is a full-featured terminal Nostr client built on top of the existing NMP
kernel (`nmp-core`) and Chirp app crate (`nmp-app-chirp`). It is the reference proof that
the NMP substrate works equally well as a terminal application as it does on iOS.

**Power-user first.** Every action is reachable from the keyboard in ≤3 keystrokes.
**No logic in the shell.** Rust owns all state; the TUI is rendering + capability execution.
**No polling.** Updates arrive via `nmp_app_set_update_callback()` pushed to an mpsc channel.

---

## 2. Architecture

### 2.1 Crate location

`apps/chirp/chirp-tui/` — mirrors `apps/chirp/chirp-repl/`. Reuses `AppRuntime` pattern from
chirp-repl (apps/chirp/chirp-repl/src/app.rs) as a starting point.

### 2.2 Module layout (≤300 LOC per file per AGENTS.md)

```
apps/chirp/chirp-tui/src/
  main.rs           — clap args, raw-mode setup, event loop bootstrap
  app.rs            — AppState, Mode enum, root event handler
  bridge.rs         — NMP push callback → tokio mpsc::Sender<NmpEvent>
  profile_cache.rs  — pubkey → (display_name, picture_url, nip05, color) LRU 512
  image_cache.rs    — URL → decoded pixels; async fetch; iTerm2/Kitty/Sixel/halfblock
  ui/
    layout.rs       — top-level 3-pane split, resize
    timeline.rs     — feed list (tui-widget-list, virtual scroll)
    thread.rs       — depth-indented DAG flat view
    compose.rs      — textarea + @-mention popup
    dm.rs           — DM conversation bubbles
    profile.rs      — author card with avatar
    status_bar.rs   — hotlist, relay indicators, spinner
    help.rs         — contextual ? overlay + pending-key infobox
```

### 2.3 Event loop (no polling — D8 compliant)

```rust
tokio::select! {
    ev  = crossterm::EventStream  => handle_terminal_event(ev),
    msg = nmp_rx                  => handle_nmp_event(msg),   // push, not poll
    _   = render_tick             => handle_render_tick(),    // animation cadence only
    _   = shutdown                => break,
}
```

`handle_nmp_event` reads the latest snapshot via `chirp_snapshot()` and diffs against
previous state. The render tick must never poll app state, inspect relay state, or drain
channels; it only advances already-known animation frames. All data changes arrive via
`nmp_app_set_update_callback()` → bounded mpsc (capacity 64).

### 2.4 Profile resolver

Separate tokio task. Receives pubkeys from a channel, requests profile opening through NMP,
and updates the 512-entry LRU only after the pushed snapshot contains profile data. Colors:
`djb2(npub_bytes) mod 14` → semantic 14-slot palette (excludes white/black). Color is
stable across display-name renames because it is hashed on npub, not name.

### 2.5 Image pipeline

`ratatui-image 11.0` with `Picker::from_query_stdio()` for runtime capability detection.
Fallback ladder: **Kitty → iTerm2 → Sixel → Unicode half-blocks (▀/▄ truecolor)**.
Avatars fetched via a bounded tokio task pool (max 4 concurrent HTTP fetches); render uses
a colored-initials placeholder until fetch resolves. Inline note images (URLs in
`content_tree`) are **opt-in**: `I` key toggles per-session. Guard `from_query_stdio()`
behind `IsTerminal` check so CI never deadlocks.

---

## 3. Layout

Wide layout: title/hotlist bar, left feed, center note/thread, right profile/detail,
collapsible compose tray, and bottom status bar. Pane focus is `1` feed, `2` detail,
`3` profile. `z` cycles focused-pane zoom. Minimum terminal is 80×24: hide the right
pane first, then wrap content. `--basic` collapses to one pane and disables images and
animations.

---

## 4. Keybindings

### Normal mode — navigation
| Key | Action |
|-----|--------|
| `↑` / `↓` (or `k` / `j`) | prev / next item |
| `PgUp` / `PgDn` | scroll by page |
| `Home` / `End` | top / bottom of feed |
| `Enter` | open thread in detail pane |
| `[` / `]` | prev / next sibling reply |
| `Tab` / `Shift+Tab` | cycle feed tabs |
| `1` `2` `3` | focus pane |
| `z` | cycle focused-pane zoom |
| `/` | open search / command palette |
| `Ctrl+?` | contextual keybinding overlay |
| `q` / `Esc` | close detail / cancel |

### Normal mode — actions
| Key | Action |
|-----|--------|
| `i` | compose new note |
| `r` | reply to selected note |
| `+` | react ⚡ (NIP-25 `+`) |
| `b` | repost/boost |
| `f` / `F` | follow / unfollow author |
| `p` | open author profile |
| `o` | list URLs, open in browser |
| `I` | toggle inline image preview |
| `y` | yank note-id to clipboard |

### Compose mode (tui-input + custom multiline)
| Key | Action |
|-----|--------|
| `@` | @-mention autocomplete popup |
| `#` | hashtag autocomplete |
| `Ctrl+Enter` | publish |
| `Esc` | cancel |

### Command / search palette (`/` prefix)
`/home` `/mentions` `/dms` `/groups` `/profile <npub>`
`/relay add <url>` `/relay rm <url>` `/relay status`
`/search <#tag>` `/thread <note-id>` `/basic` `/quit`

---

## 5. Feature Inventory

### F1 — Timeline feed
- Home feed from `chirp_snapshot().cards + blocks`
- Display names from profile cache (shows `npub1…abcd` until resolved)
- Per-author stable color (djb2 of npub)
- Avatars: 2-cell square left of name (ratatui-image, halfblock fallback)
- `created_at` → relative time ("3m ago", "2h ago", "Mon 14:22")
- Braille sparkline of reply velocity at far-right column
- Tab views: Home / Mentions / Global / #tag

### F2 — Thread view
- Depth-indented flat rendering (safe for NIP-10 DAGs — no tree widget)
- Root note at top, replies indented 2 cells per level, clamped at depth 6
- Quote reposts rendered with `┌─ quoted ─┐` border inline
- `[` / `]` navigate sibling replies

### F3 — Compose / Reply / React
- Custom multiline compose widget on top of `tui-input`; split if it exceeds ~50 LOC
- @-mention: `@` → `tui-popup` + `tui-widget-list` filtered by typed prefix
- Character counter: >280 yellow, >800 red
- Pending/published state appears only through Rust-produced snapshots
- Settings → Outbox includes active and settled publish rows; `Enter` opens per-relay detail
- Failed or partially failed rows expose Rust-owned retry/clear actions; the TUI never parses relay errors or decides retry policy
- Reply shows parent note preview above textarea

### F4 — DM inbox (NIP-17)
- Conversation list + bubble thread; outgoing right, incoming left
- `is_outgoing` pre-classified by Rust (`DmInboxSnapshot.DmMessage.is_outgoing`)
- NIP-44 decrypted content rendered directly; no key material in TUI layer

### F5 — Profile view
- Avatar (8-cell tall), display_name + nip05, npub short + `y` to copy
- Bio, follow/unfollow button, recent notes list

### F6 — Group chat (Marmot MLS + NIP-29)
- Room list + chat log; same bubble layout as DMs
- `n` opens a centered Create group modal for protocol, name, relays, NIP-29 local id, and MLS invitees; `/group` and `/mls` commands remain power-user paths
- NIP-29 public group creation collects protocol, display name, and relay; the
  in-relay `local_id` is generated as `slug(display-name)-<random-number>` so
  users do not have to invent protocol identifiers
- Marmot MLS groups use `/mls invite` / `/mls accept` via command palette

### F7 — Search
- `/search #tag` opens firehose-tag feed tab
- Command palette fuzzy search over display names / npubs via nucleo

### F8 — Relay management
- Status bar: per-relay health dot (●/○)
- `/relay status` pane: latency, event counts
- `/relay add` / `/relay rm`

### F9 — Animations & polish
- tachyonfx slide-in (120ms) for new notes arriving in feed
- Braille-frame spinner on relay publish in-flight
- tui-big-text startup banner (200ms, any key skips)
- `--basic` / `NO_ANIMATIONS=1` disables all FX and images

---

## 6. Dependency List

Initial dependencies: `ratatui 0.30`, `crossterm 0.29`, `tokio 1.45`, `tokio-util`,
`color-eyre`, `clap`, `serde_json`, and `is-terminal`; UI helpers `ratatui-image 11.0`,
`ratatui-macros`, `tachyonfx`, `tui-input`, `tui-popup`, `tui-scrollview`,
`tui-widget-list`, `throbber-widgets-tui`, `tui-markdown`, `tui-big-text`, `opaline`;
data helpers `nucleo`, `reqwest`, `image`, `lru`; dev tools `insta` and `expectrl`.

Do not add `tui-textarea` for v1 unless its ratatui version matches the crate. The v1
compose surface is `tui-input` plus a small custom multiline wrapper.

## 7. Milestones & Acceptance Criteria

Treat each milestone as an independent PR/agent lane; split M4 into DM and group PRs if either exceeds one reviewable diff.

### M1 — Skeleton + observer wiring
**Scope**: `apps/chirp/chirp-tui` compiles, renders placeholder layout, NMP push callback fires.

- [ ] `cargo build -p chirp-tui` clean, zero warnings
- [ ] `cargo run -p chirp-tui` opens ratatui window: title bar, 3 empty panes, status bar
- [ ] `Ctrl+C` exits cleanly, raw-mode restored, no terminal corruption
- [ ] `nmp_app_set_update_callback` fires within 5s of startup (status bar logs event count)
- [ ] TestBackend insta golden: layout renders at 120×40 matches snapshot
- [ ] No `sleep` loops anywhere; zero `thread::sleep` or `tokio::time::sleep` in main path
- [ ] `cargo test -p chirp-tui` passes (scoped only, never full-workspace)

### M2 — Timeline read + names + avatars
**Scope**: Full read experience. Home feed with display names, timestamps, avatars, thread view.

- [ ] Feed shows ≥20 notes after relay sync
- [ ] ≥80% of author display_names resolved (not raw pubkeys) within 10s of feed load
- [ ] Each note row: avatar (iTerm2 protocol or halfblock), colored display_name, relative time, content preview
- [ ] Long content truncated at 2 lines; Enter expands to depth-indented thread view
- [ ] `j`/`k` scroll, `gg`/`G` work; no jank at 30 FPS
- [ ] Avatar placeholder (colored initials block) shows during async fetch; no layout shift
- [ ] Profile cache LRU 512 entries; repeat visit to same author is instant (no re-fetch)
- [ ] TestBackend snapshots: feed row render, thread render, avatar-placeholder state

### M3 — Compose / react / reply / follow
**Scope**: Full write experience matching chirp-repl command surface.

- [ ] `i` opens compose textarea; `Ctrl+Enter` publishes; `Esc` cancels
- [ ] Pending/published note appears via Rust snapshot within 200ms of `Ctrl+Enter`
- [ ] Relay ACK spinner resolves to ✓; failure shown in status bar
- [ ] `r` on selected note opens reply with parent note preview above textarea
- [ ] `+` sends NIP-25 `+` reaction; confirmation in status bar
- [ ] `f`/`F` follows/unfollows; status bar confirms
- [ ] @-mention popup appears on `@`, filters by display_name prefix, Enter inserts npub
- [ ] Works against real relays (wss://relay.damus.io or wss://relay.primal.net)

### M4 — Threads + DM inbox + Group chat
**Scope**: Full social graph: threaded conversations, NIP-17 DMs, Marmot MLS groups.

- [ ] DM tab shows conversation list; Enter opens bubbled message thread
- [ ] Outgoing messages right-aligned, incoming left-aligned with sender avatar
- [ ] Composing in DM tab sends NIP-17 gift-wrap via dispatch_action
- [ ] Group chat tab lists Marmot groups; Enter opens chat log
- [ ] Unread badge in hotlist for DMs and mentions
- [ ] `[`/`]` navigate sibling replies in thread view
- [ ] `/profile <npub>` opens profile pane with avatar, bio, recent notes

### M5 — Animations + polish + CI golden tests
**Scope**: Production visual polish, full test suite, demo recordings.

- [ ] tachyonfx slide-in (120ms) on new note arrival — visually smooth at ≥30 FPS
- [ ] Braille-frame spinner on relay publish in-flight
- [ ] tui-big-text startup banner dismisses on any key
- [ ] `--basic` flag disables all animations and images; works in 16-color terminals
- [ ] ≥15 insta snapshot golden scenarios passing in CI via TestBackend
- [ ] expectrl E2E test: load nsec → relay connect → compose note → note visible in feed
- [ ] README demo recording (QuickTime + iTerm2) showing avatars + animations
- [ ] `cargo clippy -p chirp-tui -- -D warnings` passes

---

## 8. Testing Strategy

| Layer | Tool | Scope |
|-------|------|-------|
| Unit | `#[test]` | Profile cache, color hashing, command parsing, content_tree parsing |
| Widget render | `TestBackend` + `insta` | Layout at 80×24, 120×40, 200×50; all pane states |
| E2E PTY | `expectrl` | nsec login → relay → compose → note appears |
| Demo | QuickTime + iTerm2 | Image protocol, animations (manual, per milestone) |

Agents MUST scope all test runs: `cargo test -p chirp-tui`. Never `cargo test` workspace-wide.

---

## 9. Risks & Mitigations

| # | Risk | Mitigation |
|---|------|------------|
| R1 | Custom multiline widget grows beyond the intended small wrapper | Split under `ui/compose/`; only add `tui-textarea` if ratatui-compatible |
| R2 | VHS doesn't render iTerm2/Kitty images | QuickTime for image demos; VHS for non-image flows |
| R3 | `Picker::from_query_stdio()` deadlocks in CI (no tty) | `IsTerminal` guard; always halfblocks in CI |
| R4 | Profile resolver floods relays with kind:0 on cold start | Batch single filter per 50 pubkeys; debounce 500ms |
| R5 | NIP-10 DAG cycles / very deep threads | Clamp depth 6; de-dup event ids in render path |
| R6 | Mouse capture breaks native text selection | Document Shift+click; provide `/mouse off` toggle |
| R7 | emit_hz=4 (250ms) lags compose feedback | Raise to emit_hz=10 in TUI `nmp_app_start()` call |

---

## 10. Non-goals (v1)

- NIP-57 zap UI (NWC executor exists but is deferred per review #38)
- WASM / web build
- Windows / Terminal.app support
- Push notifications / daemon mode
- Plugin system
