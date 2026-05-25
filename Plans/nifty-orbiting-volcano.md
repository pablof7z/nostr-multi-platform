# Plan: Migrate approach-b TUI design into chirp-tui

## Context

The TUI mockup at `tui-mockups/approach-b/` established a UX direction: master-detail layout, colorful post cards, relay panel, command palette (`/`), pane focus toggle, reply selection, and a profile view in the left pane. This plan ports that visual design into the real `apps/chirp/chirp-tui/` crate, which has ~2,700 LOC of load-bearing NMP wiring that must stay intact. Only the rendering layer (~700 LOC) is replaced.

---

## What stays exactly as-is

- `runtime.rs`, `bridge.rs`, `snapshot.rs`, `feature_snapshot.rs`, `timeline.rs`
- `features.rs`, `commands.rs`, `runtime_commands.rs`, `render_intents.rs`, `main.rs`
- `ui/feature_panels.rs` (Chats / Groups / Wallet / Settings — redesign deferred)
- `ui/help.rs`, `ui/shared_snapshot_lines.rs`
- `tests/e2e.rs` — tests must remain green at every PR boundary
- The `:` command surface — untouched; `/` is a new parallel entry point

---

## What changes

### New rendering modules (all under `apps/chirp/chirp-tui/src/ui/`)

| File | Responsibility | LOC target |
|------|---------------|------------|
| `home.rs` | Home tab layout orchestration (splits, dispatches to sub-renderers) | ≤150 |
| `post_list.rs` | Left-pane feed list + profile-mode post list | ≤250 |
| `post_detail.rs` | Right-pane thread detail with per-item selection | ≤250 |
| `relay_panel.rs` | Relay health panel (bottom of left column) | ≤100 |
| `palette.rs` | `/` command palette modal | ≤200 |
| `colors.rs` | Shared truecolor palette constants | ≤60 |

### Modified files

- `ui/layout.rs` — route Home tab to `home::render()`; keep everything else
- `ui/mod.rs` — export new modules
- `app.rs` — add `detail_cursor: usize`; add `Mode::Palette { cursor: usize }` variant; keep all existing fields
- `input.rs` — add `l`/`→` (focus detail), `h`/`←` (focus list), `j`/`k` in detail mode (move `detail_cursor`), `/` (open palette), palette navigation

---

## Layout after migration

**Home tab — 2-pane always:**
```
┌─────────────────────┬────────────────────────────────┐
│ Left pane (38%)     │ Right pane (62%)               │
│                     │                                │
│ Feed mode:          │ Thread detail + reply list     │
│   post list         │ (reply cursor, / palette)      │
│   relay panel       │                                │
│                     │                                │
│ Profile mode:       │ Same thread detail             │
│   profile header    │ (author's selected post)       │
│   author post list  │                                │
│   relay panel       │                                │
└─────────────────────┴────────────────────────────────┘
```

The 3-pane wide-terminal layout (`feed | detail | profile-right`) is replaced by 2-pane. Profile lives in the left pane (matching the mockup). `Pane::Profile` retains its name but now controls left-pane content.

---

## State additions to AppState

```rust
// In app.rs:
pub detail_cursor: usize,   // 0=main post, 1..=replies.len()=reply index; reset on row change

// In Mode enum:
Palette { cursor: usize },  // new variant alongside Normal | Compose | Command
```

`AppState.status: String` (already exists) is reused for action feedback — no separate toast system needed.

---

## Data mapping (real NMP → visual)

| Visual field | NMP source |
|---|---|
| Author name | `TimelineRow.author` |
| Post body | `TimelineRow.content` |
| Timestamp | `format_age(TimelineRow.created_at)` helper (new) |
| Reactions | `TimelineRow.relation_counts.reactions` |
| Reposts | `TimelineRow.relation_counts.reposts` |
| Reply count | `TimelineRow.relation_counts.replies` |
| Reply depth / thread | `TimelineRow.depth` |
| Relay URL + status | `RelayRow.url` + parse `RelayRow.connection_label` → ●/○/◌ |
| Relay event count | `RelayRow` — expose existing counter if present, else omit |
| Profile bio | `FeatureSnapshot.author_profile.about` |
| Author color | deterministic: `AUTHOR_CYCLE[pubkey.bytes()[0] % N]` |
| Avatar | Colored `██` block in author color (no image crates — S5 post-merge) |

---

## Palette actions and dispatch

| Action | Dispatch target | Context |
|---|---|---|
| View profile | `runtime.open_author(pubkey)` + `focused = Pane::Profile` | main post or reply |
| React ♥ | `runtime.react(note_id, "+")` | main post only |
| Follow | `runtime.follow(pubkey)` | main post or reply |
| Repost | `runtime.dispatch_action("nmp.repost", …)` | main post only |
| Reply | switch to `Mode::Compose`, set `reply_to` | main post or reply |
| Zap | set `state.status = "Connect wallet first (:wallet connect …)"` | main post |

Context-awareness: if `focused == Pane::Detail && detail_cursor > 0`, use the reply's author/id; otherwise use the selected feed row.

---

## PR sequence

### PR 1 — Visual refresh + pane focus

**Scope:** S0 (commit mockup) + new rendering modules + wire Home tab + `l`/`h` focus toggle

**Files touched:** `ui/home.rs` (new), `ui/post_list.rs` (new), `ui/post_detail.rs` (new), `ui/relay_panel.rs` (new), `ui/colors.rs` (new), `ui/mod.rs`, `ui/layout.rs`, `app.rs` (add `detail_cursor`), `input.rs` (add `l`/`h`/`→`/`←`)

**New keyboard bindings:**
- `l` / `→` — focus detail pane (set `focused = Pane::Detail`)
- `h` / `←` — focus list pane (set `focused = Pane::Feed` or `Pane::Profile`)
- `j`/`k` in detail — move `detail_cursor` through post + replies
- `J`/`K` in detail — scroll (existing behavior)
- Existing `1`/`2`/`3` focus keys and `p` profile key remain

**Visual indicator:** left-pane border = `ACCENT_CYAN` when focused, `DIMMER_TEXT` otherwise; right-pane top border flips similarly.

**e2e status-bar contract:** add assertion string `"focus:detail"` / `"focus:feed"` to `state.status` when focus shifts, so `e2e.rs` can assert on it.

**Verification:** `cargo build -p chirp-tui` clean; `cargo test -p chirp-tui` green; manual `cargo run` shows new visual in Home tab; other tabs unchanged.

---

### PR 2 — Command palette + reply selection

**Scope:** `ui/palette.rs`, `Mode::Palette` variant, `/` keybinding, context-aware action dispatch

**Files touched:** `ui/palette.rs` (new), `ui/mod.rs`, `app.rs` (add `Mode::Palette`), `input.rs` (palette nav + `/`), `commands.rs` (palette action handlers)

**Verification:** `cargo test -p chirp-tui` green; press `/` shows palette; Enter on "React ♥" updates reaction count; Enter on "View profile" switches to profile pane; `Esc` closes.

---

### PR 3 — Rich profile pane

**Scope:** Enhance left-pane profile rendering with big colored-block avatar (8×4), bio, following/followers from `FeatureSnapshot.author_profile`, filtered post list.

**Files touched:** `ui/post_list.rs` (profile-mode rendering path), `ui/home.rs` (profile header layout)

**Note:** follower/following counts may not be in `FeatureSnapshot` today — render what's available, leave counts blank if absent rather than hardcoding fakes.

**Verification:** press `p` on a feed post → left pane shows profile header + that author's posts; selecting posts shows thread on right.

---

## Key helpers to write (all new, small)

```rust
// ui/colors.rs
pub const HEADER_BG, LIST_BG, DETAIL_BG, SELECTED_BG, ACCENT_CYAN, ...

// In post_list.rs or a shared util:
fn format_age(unix_ts: u64) -> String  // "2m ago", "1h ago", "3d ago"
fn author_color(pubkey: &str) -> Color // deterministic from pubkey[0]
fn avatar_block(author: &str, bg: Color) -> Span // "██" in author color

// In relay_panel.rs:
fn relay_dot(label: &str) -> (&'static str, Color) // "connected"→●green etc.
```

---

## Out of scope

- Mouse support (can be added later as a small standalone PR)
- Real inline images / avatars (S5, requires `ratatui-image` dep evaluation against ratatui 0.30)
- Chats / Groups / Wallet / Settings tab redesign
- Notification / search tab content
- Zap flow (requires NWC to be connected; palette shows hint instead)
- Deleting `tui-mockups/approach-b/` (after PR 3 is merged and stable)
