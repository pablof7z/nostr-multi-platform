# Chirp Web — UX/UI Design

Status: design spec (pre-build). Audience: whoever builds the `web/chirp/` shell.
Scope: visual language, layout, screens, states, and the NMP Inspector. The shell
is a **thin rendering layer** — Rust owns all state, policy, caching, and relay
management; the UI renders snapshots and dispatches actions. Nothing in this
document requires client-side business logic. Where the design wants data the
framework does not yet ship over the wasm worker protocol, the item is marked
**[GAP-n]** and collected in §9.

---

## 1. Product stance

Chirp Web is the browser face of Chirp, the canonical NMP showcase client
(`apps/chirp/`). Two promises, in tension, resolved deliberately:

1. **It is a real client.** Polished, minimalist, fast, deployable with a
   straight face. The feed is the product.
2. **It is a glass-walled machine room.** NMP's internals — relays, negentropy
   sync, the store, routing, subscriptions, the signer — are *visible by
   design*, as a legible instrument panel, never as a debug dump.

Resolution: the core social surface stays clean; the machinery lives in a
dedicated **Inspector** dock plus a handful of *whisper-quiet* live indicators
woven into normal use (§8). A user who never opens the Inspector gets an
elegant Nostr client. A curious user opens it and goes "oh — *that's* what's
happening under the hood."

Honesty doctrine: the runtime has explicit degraded modes
(`browser_actor_driver_missing`, `protocol_mismatch`, `capability_rejected`,
`wasm_bridge_unavailable`, `signer_not_installed`,
`publish_path_not_wired_for_kind`, `unsupported_signer_*`, ephemeral-store
fallback). The UI **never fakes success**. Every degraded state renders as a
designed object with three parts: a plain-language sentence, the raw reason
code in monospace (the "truth chip"), and what still works (§7).

---

## 2. Data reality — what the shell can render

Design is constrained to what crosses the worker boundary. Inventory as of
today (sources: `crates/nmp-wasm/src/protocol.rs`, `snapshot.rs`;
`crates/nmp-core/src/update_envelope.rs`, `kernel/types.rs`,
`kernel/typed_projections/`, `kernel/relay_diagnostics.rs`;
`crates/nmp-ffi/src/routing_trace.rs`).

**Worker events (JS-visible today):** `hello_accepted{protocol_version,status}`,
`runtime_status{ready|running|degraded(mode)|stopped}`, `action_accepted`,
`update_bytes` (FlatBuffers `NMPU` frame), `capability_failure{capability,
correlation_id, reason}`, `error{code,message}`.

**Tier-3 snapshot envelope (decodable today):** `rev`,
`kernel_schema_version`, `last_tick_ms`, `running`, `update_kind`, metrics
subset (`events_rx`, `visible_items`, `actor_queue_depth`,
`update_sequence`), `relay_statuses[]{role, relay_url, connection, auth,
events_rx, denied}`, `wire_subscriptions[]{wire_id, relay_url, state}`,
`last_error_toast`, `last_error_category`, `last_planner_error`.

**Typed projection sidecar (kernel-side, decoders exist in `nmp-core`):**
`relay_diagnostics` (pre-rolled rows with *labels and tones already chosen by
Rust* — `connection_label`/`connection_tone`, `auth_label`/`auth_tone`,
per-relay wire subs, logical interests with `cache_coverage`),
`configured_relays`, `accounts`, `active_account`, `profile`,
`resolved_profiles`, `claimed_profiles`, `claimed_events`, `publish_queue`,
`publish_outbox`, `outbox_summary`, `action_results`, `action_stages`,
`action_lifecycle`, `signed_events`, plus host feed projections
(`nmp.feed.home`, `thread_view`, `author_view`, `nmp.follow_list`).

**Caveat — the wasm producer is behind the kernel.** The browser snapshot
builder (`nmp-wasm/src/snapshot.rs`) currently writes only `rev`,
`kernel_schema_version`, `running`, and `relay_statuses` with the single
honest connection state `"configured"`. No metrics, no wire subscriptions, no
typed sidecar, no `last_tick_ms`. The design below targets the **kernel
contract** (so the same UI lights up panel by panel as nmp-wasm catches up)
and renders honest "not yet reported by the browser runtime" placeholders for
fields the wasm frame leaves at zero. Every such dependency is a numbered gap
in §9.

Cardinal rule for the shell: **labels and tones come from Rust where Rust
provides them** (`relay_diagnostics` ships `*_label` + `*_tone` strings). The
shell maps tone strings to CSS custom properties and renders. It does not
re-derive status semantics in TypeScript.

---

## 3. Visual language

### 3.1 Principle

"**Field instrument**": the calm of a paper reading surface for content, the
precision of a measuring instrument for diagnostics. Two type voices, one
restrained palette, and a tone system lifted directly from the kernel's own
vocabulary — so the brand *is* the framework's truthfulness.

### 3.2 Typography

| Voice | Stack | Used for |
| --- | --- | --- |
| Prose | `"Inter", ui-sans-serif, system-ui, sans-serif` | Notes, names, buttons, nav |
| Instrument | `"IBM Plex Mono", ui-monospace, monospace` | Counters, relay URLs, ids, reason codes, Inspector tables |

Scale (rem at 16px root): `12 / 13 / 14 / 16 / 20 / 24`. Note body is 16/1.55.
Inspector tables are 13/1.4 mono with `font-variant-numeric: tabular-nums`
everywhere a number can change between frames — counters must not jitter
horizontally. Pubkeys and event ids always render abbreviated
(`a1b2c3d4…e5f6`) in Instrument voice; full value on hover/long-press copyable.

### 3.3 Spacing & density

4px base scale: `4, 8, 12, 16, 24, 32, 48, 64`. Content column measure:
640px max. Feed cards: 16px padding, separated by 1px hairlines — no boxes,
no shadows on the reading surface. Inspector density is deliberately tighter:
32px table rows, 12px padding — it should *feel* like an instrument cluster
next to the airy feed. Border radius: 8px on cards/sheets, 999px on chips and
status dots. One elevation only (modals/popovers): `0 8px 32px
rgb(0 0 0 / 0.16)`.

### 3.4 Color

One brand accent — **relay green** — deliberately the same hue as the
"connected" state, tying brand identity to the live-connection story.

```
Token            Light                Dark                 Role
--bg             #FAFAF8              #131412              page
--bg-raised      #FFFFFF              #1B1C1A              cards, dock, modals
--bg-inset       #F1F1ED              #0E0F0D              inspector wells, code
--ink            #1C1D1A              #E8E7E1              primary text
--ink-2          #6B6C66              #9A9B93              secondary text
--ink-3          #A3A49C              #5F605A              placeholders, hairline icons
--line           #E6E6E0              #2A2B28              hairlines
--accent         #2F7D5B              #5BBd92              brand, links, primary action
--accent-ink     #FFFFFF              #0E2A1E              text on accent
```

Semantic tones — **mapped 1:1 from kernel tone strings** (`ok`, `warn`,
`error`, `muted` for status; `primary`, `write`, `accent`, `secondary` for
relay roles):

```
--tone-ok        #2F7D5B / #5BBD92      connected, auth OK, settled
--tone-warn      #B07A1E / #D9A84E      reconnecting, pending, degraded-partial
--tone-error     #B3403A / #E07A75      disconnected, denied, failed
--tone-muted     --ink-3                configured-but-unobserved, closed, n/a
--role-primary   --accent               content relays
--role-write     #7A5AC8 / #A78BE0      write/outbox relays
--role-accent    #2B7DA8 / #6FB4D8      indexer relays
--role-secondary --ink-2                everything else
```

Usage discipline: color carries *state*, never decoration. The reading
surface is monochrome + accent; tones appear only on status dots, chips, and
Inspector rows. `denied: true` is the one state allowed to paint a row
background (`--tone-error` at 8% alpha) — policy denial deserves weight.

Dark mode follows `prefers-color-scheme`, manual override in Settings,
persisted in `localStorage` (presentation preference only — not runtime
state, so it legitimately lives in the shell).

### 3.5 Motion

Quiet and physical. Durations: 120ms (hover/press, ease-out), 200ms (dock
slide, sheet rise, `cubic-bezier(.2,.8,.2,1)`), 300ms (status dot pulse).
Rules:

- **The heartbeat.** A 6px status dot pulses once (scale 1→1.4→1, opacity
  bump) on every accepted `update_bytes` frame. This is the single ambient
  "the machine is alive" signal, used in the rail and the Inspector header.
- Live counters update in place, no count-up animation — tabular-nums absorbs
  the change. A value that changed this frame gets a 300ms `--tone-ok` text
  flash at 40% then settles. (Flash, not slide: instruments tick.)
- New feed items never shove the viewport. They accumulate behind a pill
  ("12 new chirps") unless the user is at top.
- `prefers-reduced-motion`: pulses become opacity-only, slides become fades,
  flashes are dropped.

### 3.6 Iconography & voice

1.5px stroke line icons (Lucide set or hand-rolled), 20px grid. Status is
always **dot + word**, never dot alone (color-blind safe). Copy voice: plain,
specific, lowercase-calm. "Listening on 3 relays." not "Connecting to the
decentralized network!" Reason codes are never paraphrased away — they appear
verbatim in a mono "truth chip" beside the human sentence.

---

## 4. Shell layout & navigation

### 4.1 Structure (desktop ≥ 1024px)

Three regions: a fixed **rail** (nav + identity + pulse), the **stage**
(feed / thread / profile, 640px measure, centered in remaining space), and
the **Inspector dock** (right, 400px, collapsible). When collapsed, the dock
reduces to a 36px **pulse strip** — the permanent, tasteful seam between the
client and the machine room.

```
┌──────┬──────────────────────────────────────────────┬──┐
│ rail │                  stage                       │▌ │← pulse strip
│ 76px │            (max 640px, centered)             │  │  (36px, click
│      │                                              │  │   to expand)
│  🐦  │   ┌────────────────────────────────────┐     │● │
│      │   │  feed / thread / profile / …       │     │● │← relay dots
│  ⌂   │   │                                    │     │● │
│  ✎   │   │                                    │     │  │
│  ☷   │   │                                    │     │r │← rev ticker
│      │   │                                    │     │1 │  (vertical,
│      │   │                                    │     │2 │   mono)
│  ◍   │   │                                    │     │8 │
│  ◐   │   └────────────────────────────────────┘     │4 │
└──────┴──────────────────────────────────────────────┴──┘
 rail: 🐦 wordmark · ⌂ home · ✎ compose · ☷ inspector
       ◍ identity (signer)  · ◐ theme — pinned bottom
```

With the dock expanded the stage yields width; below 1280px the expanded dock
overlays the stage as a sheet rather than squeezing the measure.

### 4.2 Mobile (< 768px)

Single column; rail becomes a bottom tab bar `⌂ Home · ✎ Compose · ◍ You ·
☷ Inspector`. The pulse strip compresses into the tab bar's Inspector icon: the
icon carries the heartbeat dot and a tiny aggregate relay dot (worst-state
wins). Inspector opens full-screen with its own internal tabs.

### 4.3 Navigation model

URL-addressable, hash or history routes — every state shareable:

```
/                feed (home timeline)
/note/:id        thread view focused on :id
/p/:pubkey       profile
/inspector       inspector (expands dock / full-screen on mobile)
/inspector/:panel    relays | subs | sync | store | routing | signer | frames
```

Compose is a **modal sheet** (`?compose=1` + optional `reply_to`), never a
route-destroying page — composing must not lose the user's place. Thread and
profile push onto the stage with a slide; back restores scroll position
(shell-side presentation state — scroll memory is legitimately the view's).

Navigation dispatches, Rust decides: entering a profile dispatches the
open-author action; entering a thread dispatches open-thread; leaving
dispatches the matching close. The shell never prefetches, caches, or merges
feed data — it renders the projection for wherever the user is.

---

## 5. Key screens

### 5.1 Connect (NIP-07 login)

Chirp Web is **read-first**: no login wall. The feed loads signed-out
immediately; identity is attached when the user wants to write. The identity
slot in the rail shows a hollow avatar ring when no signer is installed.

Flow (host runs the async half, per the `SetIdentity` contract):

1. User clicks the identity slot or any write affordance while signed out →
   Connect sheet rises.
2. Shell checks `window.nostr`. Present → primary button enabled. Absent →
   honest empty state (no fake button).
3. Click → `await window.nostr.getPublicKey()` → send
   `set_identity{kind:"nip07", pubkey_hex}` → on `action_accepted`, identity
   slot fills (avatar resolves from `profile` projection when kind:0
   arrives); on `capability_failure`, the reason renders verbatim.

```
┌─ Connect a signer ────────────────────────────┐
│                                               │
│        ◍   Your keys stay in your             │
│            extension. Chirp never sees        │
│            a private key — events are         │
│            signed by NIP-07.                  │
│                                               │
│   ┌─────────────────────────────────────┐     │
│   │  ⚿  Connect with extension          │     │  ← only if window.nostr
│   └─────────────────────────────────────┘     │    detected
│                                               │
│   Detected: window.nostr ✓                    │  ← mono, instrument voice
│   Signer kind: nip07                          │
│                                               │
│   Reading works without signing in.           │
│   ┌──────────────────┐                        │
│   │  Keep browsing   │                        │
│   └──────────────────┘                        │
└───────────────────────────────────────────────┘

No extension detected:
│   Detected: window.nostr ✗                    │
│   No NIP-07 extension found. Install one      │
│   (Alby, nos2x, …), then return here.         │
│   ┌──────────────────┐  ┌─────────────────┐   │
│   │  Re-check        │  │  Keep browsing  │   │

Pubkey rejected by runtime:
│   ⚠ signer rejected                           │
│   ┌ truth chip ─────────────────────────┐     │
│   │ invalid_pubkey_hex                  │     │
│   └─────────────────────────────────────┘     │
```

Post-connect, the identity popover (click ◍) shows: avatar + display name (or
abbreviated hex until kind:0 resolves), `npub…` copyable, "Signer: NIP-07
extension", and *exactly which write actions are wired* — the same honest
capability list as Inspector ▸ Signer (§5.6.7).

### 5.2 Feed (home timeline)

The product. Calm column, hairline-separated cards, inline composer on top
when signed in.

```
┌──────────────────────────────────────────────────────┐
│  Home                                   ⟳ r1284 ●    │ ← header: rev ticker +
├──────────────────────────────────────────────────────┤   heartbeat (mono, --ink-3)
│  ┌ ◍ ┐  What's happening?                            │
│  └───┘  ………………………………………………………………………………………           │ ← inline composer
│                                       [ Chirp ]      │   (signed-in only)
├──────────────────────────────────────────────────────┤
│              ╭───────────────────╮                   │
│              │  ↑ 12 new chirps  │                   │ ← accumulation pill;
│              ╰───────────────────╯                   │   never auto-scrolls
├──────────────────────────────────────────────────────┤
│ ┌ ◍ ┐ Fiatjaf · @fiatjaf            2m   ⋯           │
│ └───┘ The relay is the message.                      │
│                                                      │
│       ↳ 4    ♡ 31    ⇄ 2              · seen on 3 ▾  │ ← provenance whisper
├──────────────────────────────────────────────────────┤   (hover/focus only, §8)
│ ┌ ◌ ┐ a1b2c3d4…e5f6                  5m   ⋯          │ ← unresolved profile:
│ └───┘ gm                                             │   dotted avatar ring +
│       ↳ 0    ♡ 2     ⇄ 0                             │   abbreviated hex (honest;
├──────────────────────────────────────────────────────┤   fills when kind:0 lands)
│ ── caught up · 3 of 3 relays sent EOSE ──            │ ← EOSE divider (§8)
├──────────────────────────────────────────────────────┤
│ ┌ ◍ ┐ Jack · @jack                   1h   ⋯          │
│ └───┘ …                                              │
└──────────────────────────────────────────────────────┘
```

Behaviors:

- Card click → thread. Avatar/name → profile. `♡` dispatches
  `react{target_event_id}`; the heart fills only on `action_accepted` and
  reverts with an inline truth chip on `capability_failure`. No optimistic
  lying.
- Reply (`↳`) opens compose-as-reply (§5.5) — which today fails closed in
  the browser runtime; the compose sheet says so *before* the user types
  (§7.4), per honesty doctrine.
- Relative timestamps; absolute on hover.
- Counts come from the projection's relation counts; when a count's status
  is pending, render `·` (interpunct), not `0` — *unknown is not zero*.
- Renders `nmp.feed.home` / Chirp timeline cards verbatim; ordering, capping,
  and dedup are kernel concerns. **[GAP-2]** — the wasm frame does not yet
  carry feed projections.

### 5.3 Thread view

Focused note enlarged; ancestors above, replies below, single 2px
`--line` spine. Driven entirely by the `thread_view` projection (focused id,
items, previous/next labels).

```
┌──────────────────────────────────────────────────────┐
│  ← Thread                              ⟳ r1291 ●     │
├──────────────────────────────────────────────────────┤
│ │ ┌ ◍ ┐ Alice · @alice                 3h            │ ← ancestor, 14px,
│ │ └───┘ does anyone actually run negentropy?         │   muted until hover
│ │                                                    │
│ ┝━┳────────────────────────────────────────────      │
│   ┃ ┌ ◍ ┐ Fiatjaf · @fiatjaf                         │
│   ┃ └───┘                                            │ ← focused note: 20px
│   ┃   yes — NIP-77 sync means you only fetch         │   body, full-width,
│   ┃   what you're missing.                           │   actions row visible
│   ┃                                                  │
│   ┃   12:31 · Jun 12, 2026          · seen on 2 ▾    │
│   ┃   ↳ Reply     ♡ 31     ⇄ 2                       │
│ ┝━┻────────────────────────────────────────────      │
│ │  ▲ 2 earlier replies                               │ ← previous/next labels
│ │ ┌ ◍ ┐ Bob · @bob                    1h             │   straight from the
│ │ └───┘ TIL                                          │   projection
│ │ ┌ ◌ ┐ c4d5e6f7…a8b9                 40m            │
│ │ └───┘ source?                                      │
│ │  ▼ 4 more replies                                  │
└──────────────────────────────────────────────────────┘
```

While the projection reports a loading state, ancestors/replies show two
ghost rows (`--ink-3` bars) for at most 800ms before yielding to whatever has
arrived — partial truth beats a long skeleton.

### 5.4 Profile

```
┌──────────────────────────────────────────────────────┐
│  ← Profile                             ⟳ r1296 ●     │
├──────────────────────────────────────────────────────┤
│   ┌────┐                                             │
│   │ ◍  │   Fiatjaf                    [ Follow ]     │ ← primary action label
│   └────┘   @fiatjaf ✓ nip05            (from Rust)   │   comes from projection's
│            npub1qy3…8xj2 ⧉                           │   primary_action.label
│                                                      │
│   nostr ad astra.                                    │
│                                                      │
│   142 notes loaded                                   │ ← note_count_display,
├──────────────────────────────────────────────────────┤   pre-formatted by Rust
│   ┌ profile card · whisper strip ────────────────┐   │
│   │ kind:0 resolved ✓ · cached in store          │   │ ← instrument voice;
│   └──────────────────────────────────────────────┘   │   links to Inspector▸Store
├──────────────────────────────────────────────────────┤
│ ┌ ◍ ┐ Fiatjaf                         2m            │
│ └───┘ The relay is the message.                     │
│       …                                             │
└──────────────────────────────────────────────────────┘
```

- Renders `author_view` (profile card, note count display, primary action).
  Follow/Unfollow dispatch the corresponding actions; the button label and
  its post-action flip come from the projection, not local toggling.
- Unresolved profile (no kind:0 yet): dotted ring, abbreviated hex as the
  name, *no fabricated placeholders* — and the whisper strip says honestly
  `kind:0 not yet seen · profile interest active`.
- The whisper strip is the profile's showcase weave: one mono line stating
  where this profile card came from. Source attribution (store vs. live
  relay) is **[GAP-7]**; until then the strip shows resolved/claimed state
  only, which `resolved_profiles`/`claimed_profiles` already supports.

### 5.5 Compose

Modal sheet, 560px, rising 200ms. Identical sheet for new note and reply.

```
┌─ New chirp ──────────────────────────────── ✕ ─┐
│                                                │
│  ┌ ◍ ┐  ……………………………………………………………………………          │
│  └───┘  ……………………………………………………………………………          │
│         ……………………………………                          │
│                                                │
│  ┌ publish path · instrument strip ─────────┐  │
│  │ sign: nip07 ✓ → publish: nmp.publish     │  │ ← the showcase moment:
│  │ target: Auto (routing decides relays)    │  │   compose shows its own
│  └──────────────────────────────────────────┘  │   pipeline before sending
│                                                │
│                              412   [ Chirp ]   │
└────────────────────────────────────────────────┘
```

Lifecycle (all states are runtime-reported, none invented):

1. **Send** → dispatch `publish_note`; button → spinner "Signing…" (the
   NIP-07 extension may prompt — the sheet stays open, dimmed).
2. `action_accepted` → sheet collapses into a bottom-left **outbox toast**:
   `● sending · accepted by runtime`. With `publish_outbox`/`action_stages`
   flowing **[GAP-2]**, the toast advances through real stages: `signed ✓ →
   sent to 3 relays → settled 3/3` and the stage row is clickable into
   Inspector ▸ Frames.
3. `capability_failure` → sheet stays open, content preserved, failure
   rendered inline with truth chip (e.g. `nip07_sign_failed: user rejected`).
   Nothing is lost, nothing pretends.

Reply variant: a compact quote of the parent sits above the textarea. Today
the browser runtime fails replies closed — so when `reply_to_id` is set, the
instrument strip *pre-declares* it (§7.4 wireframe) and the primary button
reads "Try anyway" — the failure path is the honest showcase of fail-closed
design. **[GAP-8]** tracks wiring NIP-10 replies.

### 5.6 The NMP Inspector

The soul of the showcase. Not a console — an **instrument cluster**: curated,
pre-digested by Rust (`relay_diagnostics` ships labels and tones; the shell
maps tones to tokens and lays out type). Persistent left tab list inside the
dock; every panel deep-linkable.

```
Dock frame (expanded, 400px):

┌─ NMP Inspector ────────────────────────── ⇤ ─┐
│ ● running · rev 1284 · tick 312ms ago        │  ← header heartbeat: status,
├──────────┬───────────────────────────────────┤    rev, staleness from
│ Overview │                                   │    last_tick_ms [GAP-1]
│ Relays ● │                                   │
│ Subs     │         (active panel)            │  ← tab dots mirror each
│ Sync     │                                   │    panel's worst tone
│ Store    │                                   │
│ Routing  │                                   │
│ Signer ✓ │                                   │
│ Frames   │                                   │
└──────────┴───────────────────────────────────┘
```

#### 5.6.1 Overview

One glance, six instruments. Each tile is a number + label + sparkline-free
truth (no fake charts); each tile links to its panel.

```
┌───────────────┬───────────────┬───────────────┐
│ ● RUNNING     │ rev           │ frames         │
│ kernel v1     │ 1,284         │ 4.0/s          │
├───────────────┼───────────────┼───────────────┤
│ relays        │ events rx     │ store          │
│ 3/3 connected │ 12,408        │ 2,114 events   │
├───────────────┴───────────────┴───────────────┤
│ last error: —                                  │
│ queue depth: 0 · update seq 9,212              │
└────────────────────────────────────────────────┘
```

Fields: envelope `running`/`rev`/`kernel_schema_version`, frame rate
(shell-measured arrival rate of `update_bytes` — a transport observation,
legitimately the shell's), `events_rx`, `actor_queue_depth`,
`update_sequence` **[GAP-1]**, store counts **[GAP-3]**, `last_error_toast` +
`last_error_category`.

#### 5.6.2 Relays — live connection panel

The flagship. One card per relay, driven by `relay_diagnostics` rows
(pre-rolled `short_url`, role/connection/auth labels + tones, rolled-up sub
counts, pre-formatted byte/time displays) with `relay_statuses` as the
fallback surface until the sidecar flows **[GAP-2]**.

```
┌─ Relays ── 3 configured ──────────────────────────┐
│                                                   │
│ ● relay.damus.io                      ⌄           │
│   CONTENT · connected · auth ok                   │
│   ├ events 8,204 · 1.2 MB rx · 84 KB tx           │
│   ├ subs 2 active / 3 total · reconnects 0        │
│   ├ negentropy: reconciled ✓                      │ ← NIP-77 probe state
│   └ last event 2s ago                             │   [GAP-4]
│                                                   │
│ ● nos.lol                             ⌄           │
│   CONTENT · reconnecting…  · auth —               │ ← --tone-warn dot;
│   ├ events 3,981 · reconnects 2                   │   "reconnecting" pulses
│   └ last error: ws closed (transient) · 41s ago   │
│                                                   │
│ ▓ relay.snort.social                  ⌄           │ ← denied: 8% error wash,
│   CONTENT · connected · DENIED                    │   full-weight treatment
│   └ closed: restricted — relay requires auth      │
│      ┌ truth chip ──────────────┐                 │
│      │ close_reason: restricted │                 │
│      └──────────────────────────┘                 │
└───────────────────────────────────────────────────┘

Expanded card (⌄) appends the relay's wire subscriptions:

│   wire subs                                       │
│   ┌──────────┬─────────────────────┬──────┬─────┐ │
│   │ sub_8f21 │ kinds 1 · 42 auth.. │ open │ 412 │ │
│   │ sub_a04c │ kind 0 · 12 authors │ EOSE │  12 │ │
│   └──────────┴─────────────────────┴──────┴─────┘ │
```

Detail rules: connection dot pulses only while in a transitional state
(reconnecting); steady states are still. `events_rx` ticks live with the
value-flash from §3.5. While the wasm runtime can only honestly claim
`configured`, the card renders the muted state below — never a green dot it
cannot back **[GAP-2]**:

```
│ ◌ relay.damus.io                                  │
│   CONTENT · configured                            │
│   └ connection state not yet observed by the      │
│     browser runtime                               │
```

#### 5.6.3 Subscriptions & claims

What the kernel is *asking for* and *why* — the two-layer subscription model
made visible: logical interests (refcounted, cache-aware) compiled onto wire
subscriptions.

```
┌─ Subscriptions ───────────────────────────────────┐
│                                                   │
│ INTERESTS (logical)                               │
│ ┌───────────────────┬─────┬──────────┬──────────┐ │
│ │ key               │ ref │ coverage │ state    │ │
│ ├───────────────────┼─────┼──────────┼──────────┤ │
│ │ feed.home         │  1  │ partial  │ active   │ │
│ │ profile.a1b2c3…   │  2  │ cached   │ active   │ │
│ │ thread.9f8e7d…    │  1  │ none     │ warming  │ │
│ └───────────────────┴─────┴──────────┴──────────┘ │
│   coverage = how much the local store already     │
│   answers before any relay is asked               │ ← one-line caption; the
│                                                   │   inspector teaches
│ WIRE (per relay)                                  │
│ ┌──────────┬───────────────┬─────────┬──────────┐ │
│ │ id       │ relay         │ state   │ events   │ │
│ ├──────────┼───────────────┼─────────┼──────────┤ │
│ │ sub_8f21 │ damus.io      │ open    │ 412      │ │
│ │ sub_a04c │ damus.io      │ EOSE    │ 12       │ │
│ │ sub_c1d9 │ nos.lol       │ closed  │ 96       │ │
│ └──────────┴───────────────┴─────────┴──────────┘ │
└───────────────────────────────────────────────────┘
```

Interests come from `relay_diagnostics.interests[]` (key, refcount,
`cache_coverage`, state, relay urls) **[GAP-2]**; wire rows from
`wire_subscriptions` (already in the Tier-3 envelope contract) **[GAP-2]**.
Row click cross-links: an interest highlights the wire subs it compiled to;
a wire sub highlights its relay card in §5.6.2.

#### 5.6.4 Sync — negentropy (NIP-77)

The most "oh, that's clever" panel: range-based reconciliation instead of
refetching the world.

```
┌─ Sync · NIP-77 negentropy ────────────────────────┐
│                                                   │
│  Instead of re-downloading the timeline, Chirp    │
│  and each relay compare fingerprints of event     │
│  ranges and exchange only the differences.        │ ← two-line plain
│                                                   │   explainer, dismissible
│ ● relay.damus.io          reconciled ✓            │
│   probe: complete                                 │
│                                                   │
│ ● nos.lol                 negotiating…            │
│   probe: range exchange in progress               │
│                                                   │
│ ◌ relay.snort.social      unsupported             │
│   relay does not advertise NIP-77 — falling       │
│   back to plain REQ                               │ ← honest fallback,
│                                                   │   stated not hidden
│ ── session ───────────────────────────────────    │
│  rounds 14 · ranges compared 1,208                │
│  have/need exchanged 96 / 31                      │  ← [GAP-5]
│  est. transfer avoided ~1.9 MB                    │
└───────────────────────────────────────────────────┘
```

Per-relay probe state maps from `RelayStatus.negentropy_probe` (the
`NegentropySyncState` discriminant) **[GAP-4]**. The session aggregate block
(rounds, ranges, have/need counts, bytes-avoided estimate) has **no
projection anywhere in the framework today** — flagged as the design's most
wanted framework addition **[GAP-5]**, because "transfer avoided" is the
single most persuasive number NMP can show. Until it exists the session block
renders a muted `session stats not yet reported by the runtime` line.

#### 5.6.5 Store & cache

```
┌─ Store ───────────────────────────────────────────┐
│                                                   │
│  PERSISTENCE                                      │
│  ● in-memory (ephemeral)                          │ ← honest: IndexedDB not
│    events are kept for this tab only —            │   bound yet; never claim
│    browser persistence is not wired yet           │   disk we don't have
│    ┌ truth chip ───────────────────┐              │   [GAP-6]
│    │ store_open_failure: none ·    │              │
│    │ backend: memory               │              │
│    └───────────────────────────────┘              │
│                                                   │
│  EVENTS                                           │
│  stored 2,114 · tombstones 12 · ≈3.1 MB           │
│  duplicates dropped 1,422                         │ ← dedup as a feature:
│                                                   │   "asked 3 relays, kept 1"
│  PROFILES                                         │
│  resolved 184 · claimed 201                       │
│  placeholder avatars on screen: 3                 │
│                                                   │
│  VIEW                                             │
│  visible items 240 · open views 2                 │
│  last update: +8 inserted · 1 updated             │
└───────────────────────────────────────────────────┘
```

Fields: `Metrics.stored_events / tombstones / estimated_store_bytes /
duplicate_events / visible_items / visible_placeholder_avatar_items /
open_views / inserted_count / updated_count`, profile counts from the
`resolved_profiles` / `claimed_profiles` maps, `store_open_failure` for the
ephemeral-fallback signal. All blocked on the wasm metrics/sidecar emission
**[GAP-3]**; persistence state itself is **[GAP-6]**.

#### 5.6.6 Routing — decision trace

Renders the `recent_routing_decisions` trace: for each publish/subscription,
the **lane waterfall** — which relay-selection lanes were tried, in order,
and what each contributed.

```
┌─ Routing ── last 64 decisions ────────────────────┐
│                                                   │
│ ▸ 12:31:02  PUBLISH kind 1  ev a1b2c3…   → 3 url  │
│ ▾ 12:30:40  SUB interest #12  kinds 0    → 2 url  │
│   ┌ lane waterfall ──────────────────────────┐    │
│   │ NIP-65        ── matched 2  ●●           │    │
│   │ Hint          ── empty                   │    │
│   │ Provenance    ── matched 1  ●            │    │
│   │ UserConfigured── empty                   │    │
│   │ Indexer       ── skipped                 │    │
│   │ Fallback      ── not needed              │    │
│   └──────────────────────────────────────────┘    │
│   chosen:                                         │
│    wss://relay.damus.io      nip65·read           │
│    wss://nos.lol             nip65·read provenance│
│                                                   │
│ ▸ 12:29:58  PUBLISH kind 7  ev 9f8e7d…   → 2 url  │
└───────────────────────────────────────────────────┘
```

Collapsed rows: time, type, kind(s), result count. Expanded: lane attempts in
declared order with matched counts, then the final URL set with per-URL lane
badges and direction. This is the panel that answers "*why did my note go to
those relays?*" — the outbox model made tangible.

Data: `nmp_app_recent_routing_decisions` exists as a **pull-only FFI symbol**
(`crates/nmp-ffi/src/routing_trace.rs`) — there is **no worker request for it
in the wasm protocol**. Needs a `WorkerRequest` (or a snapshot projection)
**[GAP-9]**. Until then the panel shows the §7 not-yet-wired placeholder.

#### 5.6.7 Signer

```
┌─ Signer ──────────────────────────────────────────┐
│                                                   │
│  ● NIP-07 extension                               │
│  pubkey a1b2c3d4…e5f6 ⧉                           │
│  installed 12:14 · signs in your extension —      │
│  Chirp never holds a private key                  │
│                                                   │
│  WRITE CAPABILITIES (browser runtime)             │
│  ✓ publish note        nmp.publish                │
│  ✓ react               nmp.nip25.react            │
│  ✓ follow / unfollow   nmp.follow                 │
│  ✗ reply               publish_path_not_wired…    │ ← truthful capability
│                                                   │   table, codes verbatim
│  RECENT SIGNATURES                                │
│  12:31:02  kind 1   accepted                      │
│  12:29:58  kind 7   accepted                      │
│  12:14:21  —        nip07_sign_failed: user       │
│                     rejected                      │
└───────────────────────────────────────────────────┘
```

Sources: the shell's own `set_identity` round-trip + `capability_failure`
events (available today); the capability table is a static contract mirror of
the runtime's wired action set, updated when the runtime's wiring changes;
recent signatures from `action_results`/`signed_events` projections
**[GAP-2]**. Signed-out state: hollow ring + the Connect flow (§5.1) inline.

#### 5.6.8 Frames — transport tail

For the deepest-curious: the raw heartbeat. A capped tail (32) of worker
events, newest first — observations the shell already makes, zero extra
plumbing.

```
┌─ Frames ──────────────────────────────────────────┐
│ protocol v1 · NMPU flatbuffers · worker runtime   │
│                                                   │
│ 12:31:02.412  update_bytes   rev 1284   1.9 KB    │
│ 12:31:02.161  update_bytes   rev 1283   1.9 KB    │
│ 12:31:01.948  action_accepted nmp.publish         │
│ 12:31:01.702  update_bytes   rev 1282   1.8 KB    │
│ 12:30:58.001  runtime_status  running             │
│ …                                                 │
└───────────────────────────────────────────────────┘
```

---

## 6. Core states — empty, loading, offline

Doctrine: **an empty Chirp screen is a diagnostics opportunity.** Every empty
state states what the machine is doing right now, in instrument voice, with a
link into the relevant Inspector panel.

**Boot (cold load).** Wordmark + heartbeat dot, then staged truthful lines as
each milestone really happens — never a fake progress bar:

```
            🐦  chirp

            ● worker started
            ● wasm runtime running        ← hello_accepted/runtime_status
            ◌ listening on 3 relays…      ← start handshake echo
```

Hard ceiling 800ms before yielding to the shell with whatever is true.

**Empty feed.**

```
│              ( no chirps yet )                      │
│                                                     │
│   Listening on 3 relays · 2 subscriptions open      │
│   Events received: 0                                │
│                                                     │
│   The feed fills as relays answer.                  │
│           [ open inspector ▸ relays ]               │
```

**Offline.** Trigger: `navigator.onLine === false` (a browser observation —
legitimately shell-side) and/or all relays in a non-connected tone. Hairline
banner under the header, feed stays rendered from the last good snapshot:

```
│ ◌ offline — showing notes already in the in-memory  │
│   store · nothing is being fetched                  │
```

Copy adapts to persistence truth: with a real store **[GAP-6]** it reads
"showing notes from your local store." Compose stays available but the
publish strip pre-declares `no connected relays — publish will be queued by
the runtime` only if the runtime actually queues; otherwise the action's
`capability_failure` renders honestly.

**Stale frames.** If `last_tick_ms` ages beyond ~5s while `running`
**[GAP-1]**, the rev ticker dims and gains `· stalled 6s` — liveness is a
claim that must be re-earned every frame.

---

## 7. Degraded modes — designed, not apologized

One pattern everywhere — **sentence · truth chip · what still works**:

```
┌─ ⚠ ─ <plain sentence> ────────────────────────────┐
│ ┌ truth chip ────────────────────────────┐        │
│ │ <verbatim reason code / mode>          │        │
│ └────────────────────────────────────────┘        │
│ <what still works> · [inspector link]             │
└───────────────────────────────────────────────────┘
```

Severity tiers decide placement:

| Tier | Placement | Modes |
| --- | --- | --- |
| Blocking | full-stage card | `browser_actor_driver_missing`, `wasm_bridge_unavailable` / in-process fallback, `protocol_mismatch` |
| Banner | hairline banner, app usable | `capability_rejected`, offline, ephemeral store, stalled frames |
| Contextual | inline at the action site | `signer_not_installed`, `unsupported_signer_kind`, `publish_path_not_wired_for_kind`, `nip07_sign_failed`, `unsupported_signer_backend_for_writes` |

### 7.1 `browser_actor_driver_missing` (blocking)

```
┌──────────────────────────────────────────────────┐
│                  ◌  no live engine               │
│                                                  │
│   The Rust core loaded, but the browser actor    │
│   driver isn't wired in this build — Chirp can   │
│   render runtime state, not live timelines.      │
│                                                  │
│   ┌ truth chip ─────────────────────────┐        │
│   │ degraded: browser_actor_driver_     │        │
│   │ missing                             │        │
│   └─────────────────────────────────────┘        │
│                                                  │
│   Works: inspector ▸ overview · frames           │
│   [ open inspector ]                             │
└──────────────────────────────────────────────────┘
```

The Inspector stays fully open in every degraded mode — *especially* then.
The diagnostics product must shine brightest when the social product can't.

### 7.2 `wasm_bridge_unavailable` / in-process fallback (blocking)

Same card; sentence: "Web Workers (or the wasm package) are unavailable here
— every action will be refused honestly." Truth chip:
`browser_bridge_unavailable`. Works: nothing live; the card links to the
README's wasm-pack instructions when served on localhost.

### 7.3 `protocol_mismatch` (blocking, with last-good ghost)

The client keeps the last good snapshot rendered, dimmed to 40%, under the
card — visibly frozen, never silently wrong:

"This shell and the Rust core disagree on the snapshot schema — rendering
stopped rather than misrender." Truth chip: `degraded: protocol_mismatch ·
shell expects schema_version 1`.

### 7.4 Write-path contextual failures

Reply compose, *before* the user types (fail-closed pre-declared):

```
│  ┌ publish path ───────────────────────────────┐  │
│  │ ✗ replies are not wired through the         │  │
│  │   browser publish path yet                  │  │
│  │   publish_path_not_wired_for_kind           │  │
│  └─────────────────────────────────────────────┘  │
│                              412  [ Try anyway ]  │
```

"Try anyway" dispatches and renders the runtime's real refusal — proof the
shell invents neither success nor failure. Reaction/follow failures render
the same chip inline at the control, 120ms shake-free (just the chip; no
melodrama).

### 7.5 Persistence unavailable (banner + store panel)

Never blocks. Banner only when it matters (first write, or offline):
"Heads up — events live in memory for this tab only." Truth chip carries
`store_open_failure` verbatim when present. Inspector ▸ Store is the full
treatment (§5.6.5).

---

## 8. The showcase weave

The framework's background activity, made visible at whisper volume. Budget
rule: **at most one ambient indicator per region**, all in `--ink-3` until
hovered, none animated except the single heartbeat.

| Whisper | Where | Data | Tap target |
| --- | --- | --- | --- |
| Heartbeat + rev ticker | stage header + pulse strip | `update_bytes` arrivals, `rev` | Inspector ▸ Overview |
| Relay dots (one per relay, tone-colored) | pulse strip / mobile tab icon | relay statuses | Inspector ▸ Relays |
| "· seen on 3 ▾" provenance chip | feed/thread cards, hover/focus only | per-event seen-on relays **[GAP-10]** (until then: `claimed_events.relay_count` for claimed events) | popover: relay list + lane badges → Inspector ▸ Routing |
| "— caught up · 3 of 3 relays sent EOSE —" divider | feed, once per session at the live/backfill seam | wire sub EOSE states **[GAP-2]** | Inspector ▸ Subs |
| Dotted avatar ring → fills on resolve | any unresolved author | `resolved_profiles` arrival | Inspector ▸ Store (profiles) |
| Profile/thread whisper strip | profile + thread headers | projection source state | relevant panel |
| Outbox toast with real stages | post-publish | `action_stages` **[GAP-2]** | Inspector ▸ Frames |
| `last_error_toast` | bottom-left toast, 6s, category-toned | envelope field | Inspector ▸ Overview |

The pulse strip is the weave's anchor: a 36px sliver of machine room always
on screen — relay dots, vertical mono rev ticker, heartbeat. Clicking
anywhere on it expands the Inspector to whichever panel matches what you
clicked (a dot → Relays; the ticker → Overview). The seam between client and
framework is literal, visible, and one click wide.

---

## 9. Gap list — data the design needs that the runtime doesn't ship yet

Ordered by build leverage. Per philosophy these are **framework asks, not
shell workarounds** — the shell renders honest "not yet reported by the
browser runtime" placeholders (muted instrument line + truth chip) until each
lands.

| # | Gap | Needed by | Layer |
| --- | --- | --- | --- |
| GAP-1 | wasm snapshot omits `last_tick_ms`, `metrics` (events_rx, queue depth, update_sequence), `update_kind` beyond stub | Overview, staleness signal, header tickers | `nmp-wasm/src/snapshot.rs` envelope fill |
| GAP-2 | wasm emits **no typed projection sidecar** (no `relay_diagnostics`, feed, `thread_view`, `author_view`, profiles, outbox, `action_stages`) and no `wire_subscriptions`; relay `connection` is honestly stuck at `"configured"` (per-relay connection observation = the Stage 3b follow-up noted in `snapshot.rs`) | Feed/thread/profile content, Relays, Subs, outbox toast, EOSE divider | `nmp-wasm` runtime + snapshot builder |
| GAP-3 | Store metrics (`stored_events`, `tombstones`, `estimated_store_bytes`, `duplicate_events`, visible/placeholder counts) not in wasm frames | Inspector ▸ Store | subset of GAP-1/2 emission |
| GAP-4 | `negentropy_probe` exists on kernel `RelayStatus` but (a) is not decoded by `nmp-core::RelayStatusEntry` (subset decoder skips it) and (b) NIP-77 isn't wired in the browser runtime | Sync panel per-relay states, relay-card negentropy line | `update_envelope/relay_status.rs` decode + wasm NIP-77 wiring |
| GAP-5 | **No negentropy session-stats projection anywhere** (rounds, ranges compared, have/need counts, est. bytes avoided) | Sync panel session block — NMP's most persuasive number | new kernel projection (`nmp-nip77` → typed sidecar) |
| GAP-6 | No browser persistence (IndexedDB) binding; `database_name` is a handshake echo only | Store panel persistence block, offline copy | `nmp-wasm` store binding |
| GAP-7 | No source attribution for resolved profiles (store-hit vs. live relay) | profile whisper strip detail | optional kernel projection field |
| GAP-8 | Reply path fails closed (`publish_path_not_wired_for_kind`); NIP-10 tag construction is host-side per issue #906 but unwired in wasm | Reply compose happy path | `nmp-wasm/src/publish_path.rs` |
| GAP-9 | `recent_routing_decisions` is pull-only native FFI (`nmp-ffi/src/routing_trace.rs`); no `WorkerRequest` or projection carries it to the browser | Inspector ▸ Routing (lane waterfall) | new worker request or snapshot projection |
| GAP-10 | No per-event "seen on relays" provenance list (only `claimed_events.relay_count`) | provenance chip popover with relay names | kernel feed-projection field |

---

## 10. Implementation notes (thin-shell contract)

- **State:** one store: latest `RuntimeSnapshot` (status, decoded envelope,
  decoded sidecar entries, capped event tail) + presentation-only signals
  (route, theme, dock open, scroll memory, compose draft). Nothing else. No
  client cache of notes, no merging across frames — each frame replaces the
  view model, monotonic-`rev` guarded (already implemented in `client.ts`).
- **Rendering:** pure projection → JSX. Tone strings → CSS custom properties
  via a single `tone()` lookup. Pre-formatted display strings
  (`*_display`, `*_label`) render verbatim — the shell formats only what only
  it can know (relative wall-clock from its own frame-arrival timestamps).
- **Dispatch:** every user intent is one `WorkerRequest`; the UI changes only
  on the runtime's answer (`action_accepted` / `capability_failure` /
  next snapshot). The sole optimistic surfaces are pure-presentation
  (dock open/close, theme).
- **CSS:** custom properties + a single stylesheet; no UI framework needed
  beyond what's in place (Solid + the existing worker client). Honor
  `prefers-color-scheme`, `prefers-reduced-motion`.
- **Accessibility:** status = dot **and** word; live regions for the toast
  and the new-chirps pill (`aria-live="polite"`); Inspector tables are real
  `<table>`s; full keyboard path: `j/k` feed, `enter` thread, `c` compose,
  `i` inspector, `esc` close.
- **If a screen seems to need client logic, stop:** that is a lower-layer
  gap. Add it to §9 and render the honest placeholder instead.
