# Plan — Replace `apps/chirp/chirp-tui` rendering with the approach-b master-detail design

**Branch:** `worktree-agent-a0269b6dde9e0d2b2` (this worktree)
**Working dir:** `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-a0269b6dde9e0d2b2`
**Status:** Plan — not yet executed. Awaiting user confirmation.

---

## TL;DR

The user wants the new TUI mockup at `tui-mockups/approach-b/` to replace the existing `apps/chirp/chirp-tui` because the current rendering "fucking sucks". **Important reframing**: "replace the TUI" actually means **replace the rendering layer only**. The mockup is a visual reference, not a copy-paste source.

The existing crate has ~3,539 LOC; the parts the user dislikes are the rendering (`ui/layout.rs`, `ui/feature_panels.rs`, `ui/help.rs`, and the render-side of `app.rs`) — roughly 600 LOC. The other ~2,900 LOC are load-bearing framework wiring (FFI integration, push callback bridge, JSON projection parsers, command surface, snapshot tests, real-relay e2e tests) and **must be preserved**.

Doing this as one giant PR is unreviewable. The plan is a staged migration (S0 → S7) where every stage leaves the crate building, `cargo test -p chirp-tui` green, and `cargo run -p chirp-tui` usable. First PR contains S0 + S1 (and probably S2). Avatars and rich profile pane are post-merge follow-ups.

---

## What we keep from existing `apps/chirp/chirp-tui`

| File | LOC | Why keep |
|---|---|---|
| `src/runtime.rs` | 317 | Real NMP FFI: `nmp_app_chirp_register`, `nmp_signer_broker_init`, `dispatch_action`, `claim_visible_author_profile`, `react`, `follow`, `open_thread`, `open_author`. Hand-tuned, framework-tested. |
| `src/runtime_commands.rs` | 304 | Per-feature dispatch helpers for `commands.rs`. |
| `src/bridge.rs` | 65 | C callback → mpsc seam. Satisfies D8 (no polling). |
| `src/snapshot.rs` | 234 | `SharedSnapshot` / `RuntimeMetrics` / `RelayRow` / `ActionResult` JSON parsers. Tested against real kernel output shape. |
| `src/feature_snapshot.rs` | 359 | Per-feature projection parsers (accounts, outbox, DM inbox, NIP-29 groups, wallet, profile, thread). |
| `src/timeline.rs` | 263 | `TimelineRow::from_snapshot` — turns `chirp_snapshot()` JSON into the row model the UI consumes. Unit-tested. |
| `src/features.rs` | 110 | `FeatureTab` enum with the 5 real tabs (Home / Chats / Groups / Wallet / Settings) + `IOS_FEATURES` parity table. |
| `src/commands.rs` | 360 | Full `:command` parser: `:account`, `:profile`, `:relay`, `:dm-relays`, `:wallet`, `:dm`, `:group`, `:mls`, `:search`, `:outbox`, `:tab`. All routed through `AppRuntime`. |
| `src/input.rs` | 168 | Key dispatch (state + runtime). |
| `src/render_intents.rs` | 103 | `RenderIntentTracker` diff machinery — drives `claim_visible_author_profile` / `release` against `state.rows`. |
| `src/main.rs` | 144 | Event loop: terminal-reader thread + nmp-forwarder thread → mpsc → `apply_nmp_event` + render. |
| `tests/e2e.rs` | 50 | rexpect against `wss://relay.damus.io`. Asserts on status-bar strings. |
| `src/app.rs` state half | ~250 of 316 | `AppState` fields + mutations (compose mode, command mode, selection, action tracking). Unit-tested. |

**Total preserved:** ~2,900 LOC of working, tested, framework-wired code. None of this is in the mockup.

## What we replace

| File | LOC | Replaced with |
|---|---|---|
| `src/ui/layout.rs` | 291 | New master-detail render (mockup design) for the Home tab. |
| `src/ui/feature_panels.rs` | 229 | Stays for Chats/Groups/Wallet/Settings in this PR; redesigned post-merge. |
| `src/ui/help.rs` | 72 | Stays. |
| `src/ui/shared_snapshot_lines.rs` | 62 | Stays. |
| `src/ui/layout_tests.rs` | 109 | Updated insta snapshots (Home tab only) once render lands. |
| `src/app.rs` UI half | ~66 of 316 | Extended with `pane_focus: PaneFocus` + `detail_cursor: usize`. Existing fields untouched. |

**Total touched:** ~700 LOC.

## What we copy from the mockup

**Visual reference only** — the mockup is the spec for what the new render should *look like*, not the implementation. We rewrite against `ratatui 0.30` from scratch.

- Three-pane master-detail (post list / detail with selectable replies / future profile right pane).
- Compact 3-line list rows: avatar + author + timestamp / body / spacer.
- Detail-pane focus toggle (`l`/`h`/`Esc`).
- Per-item `detail_cursor` (main post + replies) with `SELECTED_BG` highlight + `▶` gutter.
- Context-aware command palette (`/` opens action list for whichever item is focused).
- Color-block avatar fallback (`██` in author hue, deterministic from pubkey).
- Tab bar with active/inactive bullet style.
- Relay panel under the post list.
- Pane-aware footer hints.

## What we explicitly do NOT take from the mockup

1. **DiceBear HTTP fetch at startup.** Violates D8 (no polling/blocking I/O at boot), violates the "avatars come from kind:0 picture_url via the resolver" architecture, and will make e2e tests flaky. Default to colored-block placeholder. Real avatars are a post-merge follow-up via `claim_visible_author_profile` → kind:0 → async fetch.
2. **`ratatui 0.29` + `ratatui-image 8.1.1`.** Existing crate is on `ratatui 0.30` per its `Cargo.toml`. Product spec (`docs/product-spec/chirp-tui.md` §6) targets `ratatui 0.30` + `ratatui-image 11.0`. The new render is written against 0.30 from line one. The mockup's image-rendering code (`StatefulImage` / `Picker::from_query_stdio` / `Image::new(&Protocol)`) does **not** survive — the existing crate has **no `ratatui-image` dependency at all**.
3. **The mockup's 4-tab set (Home / Notifications / DMs / Search).** The real crate ships 5 tabs (Home / Chats / Groups / Wallet / Settings) which are framework-tested surfaces. Tabs we're not redesigning yet keep their existing `feature_panels.rs` rendering.
4. **Replacing `:` command mode with `/`.** `:` is a typed-command paradigm (`:account create`, `:relay add`, `:wallet pay`, etc.) wired into `commands.rs`. `/` is a modal action palette. They're complementary, not competing. **`/` ships as a new second entry point.** `:` stays exactly as is.
5. **Profile as a pushed `View::Feed → View::Profile` nav stack.** The existing crate has profile data via `state.features.author_profile` (kind:0 from `feature_snapshot.rs`) and a focusable `Pane::Profile`. The mockup's profile-replaces-list metaphor is a UX call we defer to S4 — the simpler integration is profile-as-right-column, which matches `ui/layout.rs::render_profile_panel` shape and works with the existing data.

## Hard constraints

- **300 LOC ceiling per file** (AGENTS.md doctrine, `chirp-tui.md` §2.2). The mockup's single 3,000-line `main.rs` cannot land. New rendering ships split across `src/ui/`:
  - `ui/layout.rs` — top-level split, tab bar, footer (≤300)
  - `ui/home.rs` — Home tab three-pane render (≤300)
  - `ui/post_list.rs` — left pane post-list rows (≤300)
  - `ui/post_detail.rs` — right pane with per-item rects + `detail_cursor` (≤300)
  - `ui/palette.rs` — `/` palette overlay (≤300)
  - `ui/relay_panel.rs` — relay health under list (≤300)
- **`cargo build -p chirp-tui`** clean, zero warnings, at every stage. `cargo test -p chirp-tui` green at every stage.
- **No `cargo test` at workspace root** ever (per `CLAUDE.md` / `AGENTS.md`).
- **Scoped tests only.** Plus the always-on `cargo test -p nmp-testing --test doctrine_lint_smoke`.
- **Status bar is the e2e test oracle.** Every new user-visible action must produce a stable status-bar string (see "Status bar contract" below).
- **No `thread::sleep` / `tokio::time::sleep` in render or input paths.** D8 enforcement.
- **PR per stage, agents commit to a branch and open PRs — never push to master.** Per `MEMORY.md` agent push protocol.

## Status bar contract for new actions

Each new interaction must update `state.status` to a string the e2e test can match on. Draft:

| Action | Status bar text |
|---|---|
| `l` or `→` from list focus | `focus detail pane` |
| `h`/`←`/`Esc` from detail focus | `focus list pane` |
| `j`/`k` in detail focus (moves `detail_cursor`) | `detail row N/M` (N = 1-based, M = main + replies count) |
| `/` opens palette | `palette open` |
| `Esc` closes palette | `palette closed` |
| Palette item `View profile` (reply context) | `opening profile <short>` (then existing `opened profile X`) |
| Palette item `Reply to @author` | `replying to <short>` (delegates to existing `start_reply`) |
| Palette item `React ♥` | existing `+ reaction for <short>` flow |

Status strings stay short, predictable, regex-friendly.

---

## Stage breakdown

Every stage leaves: build clean, scoped tests green, app runnable. Each stage is roughly one PR. Stages without PR boundaries marked explicitly are landed together.

### S0 — Land the mockup on this branch as-is

**Goal:** preserve the mockup work that's currently uncommitted before any chirp-tui changes touch the worktree.

- Commit the current `tui-mockups/approach-b/` worktree diff (3,879 lines) on this branch. Single commit, message: `feat(tui-mockup): approach B iteration — focus toggle + reply selection + context palette + profile pane`.
- Do **not** push.
- Open this `Plans/...md` for review with the user.

**Acceptance:** `git status` clean in `tui-mockups/approach-b/`; this Plan reviewed and confirmed by user.

### S1 — Rendering scaffold (PR 1, part A)

**Goal:** prove the new render technique works inside the existing chirp-tui crate, in isolation, behind a feature flag or unused module, against `ratatui 0.30`.

- Add new modules under `apps/chirp/chirp-tui/src/ui/`:
  - `home.rs` — the new master-detail Home render (≤300 LOC). Reads from existing `AppState.rows` / `state.features.author_profile` / `state.metrics`. Outputs identical to old `render_body` Home branch in *content*; new in *visual*.
  - `post_list.rs` — 3-line card rows.
  - `post_detail.rs` — single post detail (no reply selection yet — that's S2). Uses existing `state.selected_row()`.
  - `relay_panel.rs` — health panel (data from `state.relays`).
- Do **not** touch `ui/layout.rs` yet. `home::render` is unwired.
- Snapshot-test the new modules in `ui/layout_tests.rs` with `TestBackend` (don't replace existing snapshots).
- Write each file under the 300-LOC ceiling.
- **No new dependencies.** Specifically: no `ratatui-image`, no `image`, no `ureq`. Avatars are the `██` colored-block span (we already have author-pubkey → color hashing in the mockup; port that logic).

**Acceptance:**
- `cargo build -p chirp-tui` clean (zero warnings)
- `cargo test -p chirp-tui` green (existing tests + new snapshot tests for the new modules)
- `cargo clippy -p chirp-tui -- -D warnings` green
- All new files ≤300 LOC
- New code reachable via unit tests; not yet rendered in the running binary

### S2 — Swap Home render + add focus/reply selection (PR 1, part B)

**Goal:** make the new render the default Home view; introduce `PaneFocus` + `detail_cursor`.

- In `apps/chirp/chirp-tui/src/app.rs`:
  - Add `pane_focus: PaneFocus` and `detail_cursor: usize` to `AppState`.
  - Add `enum PaneFocus { List, Detail }`.
  - Add methods: `focus_detail()`, `focus_list()`, `move_detail_cursor(delta: i32)`, `clamp_detail_cursor()`, `detail_item_count()`. Mirror the mockup but operate on `state.rows[selected]` + the *thread*-aware reply list (a future shape — for now, until S3, count main post = 1 and treat replies as 0 since we don't yet have threaded replies in `TimelineRow`).
  - Status-bar writes per the contract table above.
- In `src/input.rs`:
  - Add bindings: `l`/`Right` → `focus_detail` (when in Normal mode + Home tab); `h`/`Left`/`Esc` (when focus is detail) → `focus_list`; `j`/`k` in detail-focus → `move_detail_cursor`.
  - Existing `j`/`k` semantics in list focus unchanged.
  - Esc priority: existing close-help/close-detail behaviour preserved; new behaviour layered on top.
- In `src/ui/layout.rs`:
  - Replace `render_body`'s Home branch with `home::render`. Other tabs untouched.
  - `panel()` focus-color helper continues to work but driven by `state.pane_focus` instead of `state.focused == Pane::Feed` for the Home tab specifically.
- In `src/ui/post_detail.rs`:
  - Wire `detail_cursor` highlight (currently only main post — replies arrive in S3).
- Unit tests on the new `AppState` methods. Snapshot tests refresh in `ui/layout_tests.rs`. Update existing snapshots intentionally (after eyeballing the diff).
- **New e2e test** in `tests/e2e.rs`: launch, wait for first update, press `l`, assert `focus detail pane`, press `h`, assert `focus list pane`. (Skip if rexpect/PTY size tooling needs fresh setup — the existing test stays green either way.)

**Acceptance:**
- `cargo build -p chirp-tui` clean
- `cargo test -p chirp-tui` green (including new e2e if landed)
- `cargo run -p chirp-tui` shows the new master-detail Home render
- Existing `:`-command tests pass unchanged
- All touched files ≤300 LOC

### S3 — Reply-aware detail pane + context palette (PR 2)

**Goal:** detail cursor walks main + threaded replies; `/` opens action palette whose contents depend on `palette_target()`.

- Extend `TimelineRow` (or add a sibling type) so the *currently focused row* can be queried for its child replies. Existing snapshot shape already has reply ids via the `blocks: [Module: {events: [root, reply, ...], has_gap}]` structure (see `timeline.rs::ids_from_block`). The reply *count* is in `relation_counts.replies`; the reply *bodies* arrive via opening the thread (`runtime.open_thread`).
- Decision: in S3, "replies" in the detail pane are the rows from the active `blocks[].Module.events` chain whose `root` matches the selected row. This is already what `from_snapshot` returns. The detail-pane reply list = consecutive rows in `state.rows` after `state.selected` whose `depth > state.rows[selected].depth` until depth drops back to selection depth.
- Add to `app.rs`:
  - `fn selected_post_replies(&self) -> Vec<&TimelineRow>` — slices `state.rows` per the rule above.
  - `palette_target(&self) -> (author_label, author_pubkey, is_reply)` — uses `pane_focus` + `detail_cursor` to pick main vs reply.
- Add `src/ui/palette.rs` (≤300 LOC) — modal overlay, double-border, action rows. Pure render; logic lives in `app.rs` + `input.rs`.
- Add to `src/app.rs`:
  - `enum Modal { None, CommandPalette { cursor: usize } }` field on `AppState`.
  - Action list builder, parameterized on `palette_target()`:
    - List context: `View profile`, `Reply`, `React ♥`, `Follow`, `Copy id`, `Zap` (placeholder toast)
    - Reply context: `View <author>'s profile`, `Reply to <author>`, `React ♥`, `Follow <author>`, `Zap <author> ⚡`
  - Each action dispatches via existing `AppRuntime` methods: `open_author`, `start_reply`, `react`, `follow`. No new C-ABI symbols.
- In `src/input.rs`:
  - Add Normal-mode `/` → open palette (works in both focus modes).
  - In palette mode: `j`/`k` move cursor; `Enter` executes; `Esc` closes. Same modal-priority pattern as the mockup.
- E2E test: open palette in list focus + execute `View profile`; open palette in detail focus on a reply + execute `Reply to <author>`; assert status bar.

**Acceptance:**
- Build clean, tests green
- All touched files ≤300 LOC
- Existing `:`-command surface unaffected
- E2E asserts both palette contexts

### S4 — Rich profile pane in right column (PR 3, optional or post-merge)

**Goal:** swap the right-column profile pane render to mock-faithful layout (8×4 avatar block, name + npub + follow indicator beside it, bio, stats, recent notes list).

- Reads from `state.features.author_profile: Option<ProfileLine>` (already in `feature_snapshot.rs`).
- Adds `src/ui/profile_pane.rs` (≤300 LOC).
- No backend changes — `runtime.claim_visible_author_profile` is already called by `apply_render_intents` in `main.rs` when `state.rows` shows that author.
- Wider terminal (≥104 cols) shows feed + detail + profile (3-pane). Narrower shows feed + detail only.

**Acceptance:** build clean, tests green, profile renders with mock visual once `p` opens an author.

### S5 — Real avatars via kind:0 (post-merge follow-up)

**Goal:** replace `██` colored-block fallback with actual avatar images when graphics protocol is available.

- Add `ratatui-image 11.0` to `Cargo.toml`.
- Add `apps/chirp/chirp-tui/src/image_cache.rs` (≤300 LOC): URL → `Protocol` LRU.
- Use `Picker::from_query_stdio()` behind an `IsTerminal` guard.
- Fetch from `state.features.author_profile.picture_url` (need to plumb the field through `feature_snapshot.rs` — kind:0 metadata already arrives via the existing pipeline; just exposing one more field).
- Async fetch on a bounded background task pool (max 4 concurrent reqwest GETs). Render placeholder until ready.
- **Not blocking on this PR.** Strictly post-merge.

### S6 — Spec sync + planning surface

- Update `docs/product-spec/chirp-tui.md` §3 (Layout) and §5 (Feature Inventory) to reflect the new visual.
- Add a `WIP.md` entry for whichever stage is in flight at the time.
- Confirm the BACKLOG.md mention (`docs/BACKLOG.md:1275`) still describes the right thing; update if needed.

### S7 — Retire the mockup

After S1–S6 ship to master:

- Delete `tui-mockups/approach-b/` — its job is done; the production crate now has the design.
- Add `tui-mockups/` to the repo's "do not recreate" list if such a thing exists, or just delete the entire `tui-mockups/` parent if empty.
- Reference: the design history is preserved in the chirp-tui commit history.

---

## What lands in the first PR

**Recommendation: S0 + S1 + S2.**

- S0 is mechanical (commit existing mockup work).
- S1 is purely additive (new modules, not yet wired).
- S2 is the actual user-visible swap of the Home render + focus toggle.

This keeps the first PR review tractable: ~700 LOC touched in chirp-tui, all under the 300-LOC ceiling, no new dependencies, full test coverage including a new e2e assertion.

S3 (palette + reply nav) ships as PR 2 the next day.
S4 (rich profile pane) is PR 3 or rolled into the post-merge follow-up.
S5 (avatars) is strictly post-merge.

---

## Risks and watch-outs

1. **`ratatui 0.30` API differences vs the mockup's `0.29` patterns.** `Frame::area()` is fine on both; `Image::new(&Protocol)` / `Picker::from_query_stdio()` / `StatefulImage` have moved in 11.0. Since S1–S4 introduces *no* image deps, this risk only bites in S5.
2. **The reply-list extraction rule (S3) depends on `blocks[].Module.events` ordering.** Verified against `snapshot_rows_follow_block_order` test in `timeline.rs` — order is root, then replies in block order. If the kernel ever flattens differently, the slicing logic in `selected_post_replies` breaks. Make this an explicit unit test.
3. **`Esc` priority gets crowded.** Existing precedence: close help → "detail closed" status → nothing else. New: close palette → close help → return to list focus → "detail closed". Encode the precedence in one function (`fn handle_escape(state: &mut AppState)`) so the order is one place to change.
4. **`l` conflicts with existing keys?** Check: existing chirp-tui has no `l` binding in Normal mode (`features.rs::from_key` matches `h c g w s`; `input.rs` Normal-mode match has `q ? : Tab BackTab 1 2 3 j k Down Up PgDn PgUp Home End Enter p i r + f F Esc`). `h` *is* taken (Home tab key). Decision: in Home tab + Detail focus only, `h` returns to list pane; otherwise `h` still switches to Home tab. The conflict only fires when (a) we're on Home tab and (b) focus is already Detail. Acceptable.
5. **Avatar pubkey-to-color hashing must match across left pane and right pane**, including when an author also appears as a reply author. The mockup's `author_color()` uses a 6-cycle table keyed off the first byte of the trimmed handle. For the real crate we hash on `author_pubkey` (full 64-hex), use the same 14-slot palette the product spec already mandates (`djb2 % 14`). One function in one place — re-used by both panes.
6. **Snapshot test churn.** Updating `ui/layout_tests.rs` snapshots is fine *as long as* every snapshot diff is reviewed by eye — `insta` does not stop you from rubber-stamping a regression.
7. **E2E test timing.** The existing test asserts on tab text after sending `c`/`g`/`w`/`s` keys — those are tab switches. Adding `l`/`h` keys before them changes the input flow. New e2e assertions go in their own `#[test]` function; the existing test stays as-is.

---

## Approval checklist before execution

User to confirm:

- [ ] First PR scope = S0 + S1 + S2 (mockup commit + scaffold + Home swap + focus toggle). ✅/❌
- [ ] Avatars remain colored-block in this PR; S5 (real kind:0 images) is strictly post-merge. ✅/❌
- [ ] `:` command surface stays untouched; `/` palette ships as a new second entry point in S3. ✅/❌
- [ ] Profile-as-right-column (matching existing `Pane::Profile`) instead of profile-as-nav-stack-push (mockup's metaphor). ✅/❌
- [ ] Other tabs (Chats/Groups/Wallet/Settings) keep their existing `feature_panels.rs` render in this PR; redesign is a separate follow-up. ✅/❌
- [ ] Stage-by-stage PRs (S1+S2 → S3 → S4), not a single mega-PR. ✅/❌
- [ ] Status-bar contract strings (see table) are acceptable as the e2e test oracle. ✅/❌

Once confirmed, start with **S0** (commit the mockup work on the current branch). Then S1. Then S2. Open the first PR.
