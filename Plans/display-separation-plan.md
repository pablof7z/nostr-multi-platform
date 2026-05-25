# Plan: display-string separation — backend sends raw pubkeys, presentation formats

> **Status:** draft for user review. Once approved, the canonical entry belongs
> in `docs/BACKLOG.md` §1 (active violations) and an ADR superseding aim.md §2
> anti-pattern #1 — per `AGENTS.md` planning-discipline rules this file is a
> review artefact only.

---

## §0 — Doctrine conflict (read first)

This plan **reverses an explicitly-merged direction**. Before any code moves,
the conflict must be acknowledged at the doctrine level — otherwise the very
first PR will get bounced by a reviewer pointing at `aim.md`.

**The collision:**

| Source | Position |
|---|---|
| `docs/aim.md` §2, RMP bible anti-patterns | *"Duplicated formatting logic across platforms (timestamps, display names) — Rust pre-formats into strings, native renders them."* (immutable per `aim.md` preamble) |
| `crates/nmp-marmot/src/projection/display.rs:1-6` | *"Per aim.md §6 anti-pattern #1 ('Duplicated formatting logic across platforms — Rust pre-formats into strings, native renders them')… every UI string the Marmot surface needs is computed here."* |
| 11 merged PRs in the V-22…V-33 sweep (2026-05-23 → 2026-05-24) | Moved every avatar-tile initials / avatar-tint / short-npub / relative-time helper from Swift into Rust. Each PR cites aim.md §2 by name. |
| This plan | Says backend must send **raw pubkeys**; presentation layer formats. Exact reverse of all of the above. |

**Why the reversal is being proposed (the position to validate before writing code):**

The V-22…V-33 sweep conflated two different concepts:

1. **Pre-formatting derived from a profile cache** — `TimelineItem.author_display`
   at `crates/nmp-core/src/kernel/update.rs:502` does
   `profile.display.unwrap_or_else(|| short_npub(&event.author))`. This is correct
   even under the proposed plan: the kernel observes the kind:0 cache, picks
   the real display name when known, falls back to a pubkey-derived string
   when unknown. **Formatting is a side-effect of cache integration.**
2. **Pre-formatting that never observes the profile cache** — `GroupChatMessage.author_display`
   at `crates/nmp-nip29/src/projection/group_chat.rs:139` is unconditionally
   `short_npub(&event.author)`. This field is **stale by construction**: it
   ignores any later kind:0 arrival for the same pubkey. The Swift / Kotlin /
   TUI shells render the abbreviated npub forever, even after the user's
   display name has loaded and the timeline shows it correctly.

The four cited violations all share shape (2): they live in Layer-4 NIP
projections that have no path to the profile cache. The kernel cache lives in
`Kernel::profiles` (`crates/nmp-core/src/kernel/types.rs:58-78`) and is
accessible only inside `nmp-core::kernel`; `nmp-nip17`, `nmp-nip29`,
`nmp-marmot`, `nmp-nip02` cannot read it without a new substrate seam.

**Three possible doctrines, pick one** (this is the user decision blocking the plan):

- **(A) Full reversal.** Doctrine: backend emits raw pubkeys; presentation
  layer formats. Rip out every `*_display` / `*_initials` / `*_color_hex`
  field everywhere, including from `TimelineItem` / `ProfileCard` /
  `AccountSummary`. This is what the brief literally asks for. Cost: undoes
  11 merged PRs and forces every shell (iOS / Android / TUI / Web) to ship
  bech32 encoding + djb2 hashing + relative-time formatting. Conflicts head-on
  with aim.md §2 anti-pattern #1.
- **(B) Profile-cache integration (recommended).** Doctrine unchanged from
  aim.md: Rust pre-formats. Fix: the four cited Layer-4 projections gain a
  read seam onto the substrate profile cache and use the same
  `profile.display.unwrap_or_else(|| short_npub(...))` pattern
  `TimelineItem` already uses. No fields removed; the cached "abbreviated
  npub" placeholder is replaced by the real display name as soon as kind:0
  lands. **This closes the user-visible defect ("DM peer shows
  `npub1abc…xyz` instead of Alice") without touching the doctrine.**
- **(C) Helper-not-field.** Doctrine softened: backend ships raw pubkey,
  Rust exposes formatting helpers in a `nmp-display` crate that every shell
  (Swift via uniffi, Kotlin via uniffi, TUI via direct dep, Web via wasm)
  consumes. No display string is stored in a projection; every shell calls
  the same helper on every render. Resolves the staleness but multiplies
  FFI surface (every shell now needs `nmp_display_short_npub`,
  `nmp_display_avatar_initials`, `nmp_display_avatar_color_hex`,
  `nmp_display_format_ago_secs`).

The remainder of this plan is written assuming **(A)** because the brief is
explicit. **If the user picks (B) instead**, the structural diff shrinks to
one new substrate seam (`Kernel::profile_snapshot(pubkey) -> Option<DisplayBundle>`)
and the four projections become consumers — the bulk of §4–§8 below collapses.

The user must answer this question before any branch is cut. Logged as
**PD-040 — display-string doctrine** in §3 below.

---

## §1 — Principle (assuming (A))

The backend (every projection in `nmp-core` / `nmp-nipXX` / `nmp-marmot`) emits
**raw pubkeys** (hex, 64 chars) and the **raw substrate fields** needed to
compute display strings (created_at unix secs, kind, content). The
presentation layer — iOS Swift, Android Kotlin, chirp-tui Rust, chirp-web
TypeScript — owns bech32 encoding, abbreviation, initials extraction, avatar
colour hashing, and relative-time bucketing. The substrate exposes a
read-only `ProfileSnapshot { display_name, picture_url, … }` keyed by hex
pubkey; every shell joins display data against that cache itself at render
time. No projection struct carries a `*_display` / `*_short_npub` /
`*_initials` / `*_color_hex` / `*_display_name` field.

---

## §2 — Violations catalogue

Code-verified against HEAD as of 2026-05-25. The `Raw pubkey present?` column
shows whether the struct already carries the underlying hex / npub from which
the display field is derived — the **brief asserts HIGH severity when the raw
pubkey is absent**, but verification finds the raw pubkey present in every
cited case. **The true severity axis is "does the field stale-out when kind:0
arrives?"** — captured in the rightmost column.

| Crate | Struct | Field | Type | display:: call | Raw pubkey present? | Cache-stale? | Severity |
|---|---|---|---|---|---|---|---|
| `nmp-nip17` | `DmConversation` (inbox.rs:130) | `peer_short_npub` (:140) | `String` | `display::short_npub(&peer_pubkey)` (:297) | **yes** — `peer_pubkey` :132 | **YES** | HIGH |
| `nmp-nip17` | `DmConversation` | `peer_npub` (:136) | `String` | `display::to_npub(&peer_pubkey)` (:294) | yes | no (pubkey-deterministic) | LOW |
| `nmp-nip17` | `DmConversation` | `peer_avatar_initials` (:143) | `String` | `display::avatar_initials(&peer_npub)` (:298) | yes | **YES** (collides with kind:0 display_name) | HIGH |
| `nmp-nip17` | `DmConversation` | `peer_avatar_color` (:147) | `String` | `display::avatar_color_hex(&peer_pubkey)` (:299) | yes | no (pubkey-deterministic) | LOW |
| `nmp-nip29` | `GroupChatMessage` (projection/group_chat.rs:72) | `author_display` (:101) | `String` | `short_npub(&event.author)` (:139) | yes — `pubkey` :76 | **YES** | HIGH |
| `nmp-nip29` | `GroupChatMessage` | `author_initials` (:107) | `String` | `avatar_initials(&to_npub(&event.author))` (:140) | yes | **YES** | HIGH |
| `nmp-nip29` | `GroupChatMessage` | `author_color_hex` (:115) | `String` | `avatar_color_hex(&event.author)` (:141) | yes | no | LOW |
| `nmp-marmot` | `MarmotMessageRow` (projection/payload.rs:230) | `sender_short` (:236) | `String` | `display::short_npub(&sender_npub)` (ops.rs:268) | yes — `sender_npub` :234 | **YES** | HIGH |
| `nmp-marmot` | `MarmotMessageRow` | `sender_initials` (:238) | `String` | `display::initials(&sender_npub)` (ops.rs:272) | yes | **YES** | HIGH |
| `nmp-marmot` | `MarmotMessageRow` | `sender_color_hex` (:240) | `String` | `display::avatar_color_hex(&sender_npub)` (ops.rs:273) | yes | no | LOW |
| `nmp-marmot` | `MarmotGroupRow` (payload.rs ~268) | `members_display` | `Vec<String>` | `display::short_npub(hex)` (state.rs:265) | yes — `members: Vec<String>` (hex) | **YES** | HIGH |
| `nmp-marmot` | `PendingWelcomeRow` | `inviter_short` | `String` | `display::short_npub(&c.inviter_npub)` (state.rs:293) | yes — `inviter_npub` | **YES** | HIGH |
| `nmp-nip02` | `FollowEntry` (projection.rs:59) | `short_npub` (:65) | `String` | `display::short_npub(&pubkey)` (:77) | yes — `pubkey` :61 | **YES** | HIGH |
| `nmp-nip02` | `FollowEntry` | `npub` (:63) | `String` | `display::to_npub(&pubkey)` (:75) | yes | no | LOW |
| `nmp-nip02` | `FollowEntry` | `avatar_initials` (:67) | `String` | `display::avatar_initials(&npub)` (:78) | yes | **YES** | HIGH |
| `nmp-nip02` | `FollowEntry` | `avatar_color` (:69) | `String` | `display::avatar_color_hex(&pubkey)` (:79) | yes | no | LOW |
| `nmp-core::kernel` | `TimelineItem` (kernel/types.rs:101) | `author_display` (:104) | `String` | `profile.display.unwrap_or_else(\|\| short_npub(&event.author))` (update.rs:502) | yes — `author_pubkey` :103 | no (already cache-integrated) | LOW |
| `nmp-core::kernel` | `TimelineItem` | `author_avatar_initials` (:108) | `String` | `profile.map_or(.., \|p\| p.avatar_initials.clone())` (update.rs:504) | yes | partial (color comes from cache) | LOW |
| `nmp-core::kernel` | `TimelineItem` | `author_avatar_color` (:109) | `String` | `profile.map_or(avatar_color_hex(…), \|p\| p.avatar_color)` (update.rs:507) | yes | no | LOW |
| `nmp-core::kernel` | `TimelineItem` | `author_pubkey_short` (:135) | `String` | `short_hex_display(&event.author)` (update.rs:537) | yes | no (pubkey-deterministic) | LOW |
| `nmp-core::kernel` | `TimelineItem` | `short_id` (:144) | `String` | `short_hex_display(&event.id)` (update.rs:538) | yes (`id`) | no | LOW |
| `nmp-core::kernel` | `ProfileCard` (types.rs:170) | `npub_short` (:178) | `String` | … | yes (`pubkey`, `npub`) | no | LOW |
| `nmp-core::kernel` | `ProfileCard` | `avatar_initials` (:185), `avatar_color` (:186) | `String` | (from `Profile` cache) | yes | no (this IS the cache) | LOW |
| `nmp-core::kernel` | `AccountSummary` (identity_state.rs:97) | `avatar_initials` (:97), `avatar_color_hex` (:106) | `String` | `account_avatar_initials(&display_name, &npub)` (identity.rs:710) | yes (`npub`) | no (recomputed on kind:0 — update.rs:691) | LOW |
| `nmp-core::kernel` | `MentionProfilePayload` (types.rs:267) | `display`, `avatar_initials`, `avatar_color`, `picture_url` | `String` | (computed at projection time, profile-cache integrated) | **no** — pubkey is the map key, not a struct field | no | MEDIUM (raw key absent from struct body — already a violation if (A) wins) |

**Summary count:**
- 11 HIGH-severity (cache-stale) fields across 4 Layer-4 crates.
- 11 LOW-severity (pubkey-deterministic or already cache-integrated) fields,
  almost all in `nmp-core::kernel`.
- 1 MEDIUM (`MentionProfilePayload` carries no pubkey because pubkey is the
  outer map key — under (A) the struct itself needs a `pubkey: String` field
  even though the key carries it, because the shell consumes the values).

**The brief's "HIGH = pubkey absent" model finds zero matches.** The actual
fault line is "cache-stale": the four Layer-4 projections were carved into
`nmp-nip17` / `nmp-nip29` / `nmp-marmot` / `nmp-nip02` deliberately
substrate-pure (they cannot reach into `Kernel::profiles`), so when they
compute display strings at ingest, those strings can never improve.
**This is the load-bearing finding of the audit.**

---

## §3 — Doctrine implication

`docs/aim.md` §2 anti-pattern #1 is **the source of truth being challenged**.
The cleanest way to land (A) is to write **ADR-NNNN supersedes aim.md §2
anti-pattern #1**, with this rationale:

> The 2026-05-24 V-22…V-33 sweep proved the rule is wrong in practice:
> moving formatting into Rust forced four Layer-4 NIP crates to compute
> pubkey-derived display strings at ingest time with no path to the profile
> cache, producing stale `npub1abc…xyz` placeholders that never update
> after kind:0 arrives. The corrected doctrine is: **the backend emits raw
> data; the presentation layer formats**, and a shared `nmp-display` crate
> (or wasm-exported helper bundle) ships the canonical algorithms so every
> shell is byte-identical without duplicating logic.

**This must be ADR-grade.** Without it, the first reviewer to grep
`aim.md §2` blocks the first PR. `aim.md` calls itself immutable
(line 5: *"This document is the cold-start context for a brand-new working
session. Read it before doing anything else."*), so the ADR must explicitly
amend the immutability. Without that amendment, the plan is structurally
forbidden by the planning-discipline doctrine in `AGENTS.md`.

### Doctrine-lint enforcement (under (A))

The doctrine-lint substrate is at `crates/nmp-testing/bin/doctrine-lint/rules/`,
one file per rule (d0.rs, d6.rs, d8.rs, …), wired through `rules/mod.rs`.
Each rule defines:
- `pub const ID: &str = "D17";`
- `pub fn file_is_exempt(path: &Path) -> bool;`
- `pub fn check(line: &str, is_comment: bool) -> Vec<(usize, String, String)>;`

Pattern for a new **D17 — no display strings in projections** rule:

```rust
// crates/nmp-testing/bin/doctrine-lint/rules/d17.rs
pub const ID: &str = "D17";

const BANNED_FIELD_NAMES: &[(&str, &str)] = &[
    ("_short_npub", "presentation layer formats raw pubkey via the shared nmp-display helpers"),
    ("_avatar_initials", "presentation layer derives initials from raw pubkey + profile cache"),
    ("_avatar_color", "presentation layer hashes raw pubkey via the shared nmp-display helpers"),
    ("_color_hex", "presentation layer hashes raw pubkey via the shared nmp-display helpers"),
    ("_display_name", "presentation layer joins raw pubkey against ProfileSnapshot itself"),
    // …
];

pub fn file_is_exempt(path: &Path) -> bool {
    let s = path.to_string_lossy();
    // Exempt the helper crate that owns the algorithms.
    s.contains("/nmp-display/") ||
    // Exempt presentation layer crates (chirp-tui, web/chirp render).
    s.contains("/chirp-tui/") || s.contains("/web/") ||
    // Exempt iOS Swift / Android Kotlin source trees.
    s.contains("/ios/") || s.contains("/android/")
}
```

Scope: every file matching `crates/nmp-{core,nip*,marmot}/src/**/*.rs`,
non-comment lines, struct field declarations only (the rule must skip
function-body code that legitimately uses helpers transiently in non-emitted
intermediate values — easiest gate is "line contains `pub` and one of the
banned suffixes").

The existing `nmp_core::display::*` module stays — those helpers move into
the new `nmp-display` crate (§4) and `nmp-core` re-exports for backwards
compatibility during the migration. After migration, `nmp-core` may delete
the re-exports; the helpers live exclusively in `nmp-display`.

---

## §4 — Correct model (canonical before/after)

### A. `DmConversation` (nmp-nip17)

**Before** (`crates/nmp-nip17/src/inbox.rs:130-154`):
```rust
pub struct DmConversation {
    pub peer_pubkey: String,
    pub peer_npub: String,
    pub peer_short_npub: String,        // ← delete
    pub peer_avatar_initials: String,   // ← delete
    pub peer_avatar_color: String,      // ← delete
    pub messages: Vec<DmMessage>,
}
```

**After**:
```rust
pub struct DmConversation {
    /// The OTHER party (hex pubkey, 64 chars). Presentation layer joins
    /// this against `ProfileSnapshot` for display_name / picture_url and
    /// against `nmp_display::*` for bech32 / initials / avatar tint.
    pub peer_pubkey: String,
    pub messages: Vec<DmMessage>,
}
```

`peer_npub` deletion is debatable — bech32 encoding is pubkey-deterministic
(LOW severity above) and shells without a bech32 library handy benefit from
pre-computation. **Decision deferred to the user via PD-040.**

### B. `GroupChatMessage` (nmp-nip29)

**Before** (`crates/nmp-nip29/src/projection/group_chat.rs:72-118`):
```rust
pub struct GroupChatMessage {
    pub id: String,
    pub pubkey: String,
    pub content: String,
    pub created_at: u64,
    pub created_at_display: String,      // ← delete (presentation formats)
    pub author_display: String,          // ← delete
    pub author_initials: String,         // ← delete
    pub author_color_hex: String,        // ← delete
    pub kind: u32,
}
```

**After**:
```rust
pub struct GroupChatMessage {
    pub id: String,
    /// Author hex pubkey (64 chars). Presentation layer formats and joins
    /// against `ProfileSnapshot` for `display_name`.
    pub pubkey: String,
    pub content: String,
    pub created_at: u64,
    pub kind: u32,
}
```

### C. `MarmotMessageRow` (nmp-marmot)

**Before** (`crates/nmp-marmot/src/projection/payload.rs:230-250`):
```rust
pub struct MarmotMessageRow {
    pub id: String,
    pub sender_npub: String,
    pub sender_short: String,        // ← delete
    pub sender_initials: String,     // ← delete
    pub sender_color_hex: String,    // ← delete
    pub content: String,
    pub created_at: u64,
    pub created_at_display: String,  // ← delete
    pub epoch: Option<u64>,
}
```

**After**:
```rust
pub struct MarmotMessageRow {
    pub id: String,
    /// MLS-decrypted sender pubkey (hex, 64 chars).
    pub sender_pubkey: String,
    pub content: String,
    pub created_at: u64,
    pub epoch: Option<u64>,
}
```

Note rename `sender_npub` → `sender_pubkey`: the field is hex, not bech32
(see `m.pubkey.to_hex()` at ops.rs:267), so the prior name was a misnomer.
This makes the type contract obvious to shells.

### D. New: `nmp-display` shared-helper crate

A new Layer-0 crate that exists once and is consumed by every presentation
layer (chirp-tui via cargo dep; iOS Swift via codegen Swift port; Android
Kotlin via codegen Kotlin port; chirp-web via wasm export).

```rust
// crates/nmp-display/src/lib.rs
pub fn short_npub(pubkey_hex: &str) -> String { … }
pub fn avatar_initials(npub_or_displayname: &str) -> String { … }
pub fn avatar_color_hex(pubkey_hex: &str) -> String { … }
pub fn to_npub(pubkey_hex: &str) -> String { … }
pub fn short_hex(value: &str) -> String { … }
pub fn format_ago_secs(now: u64, then: u64) -> String { … }
pub fn display_name_initials(name: &str) -> String { … }
```

Content is verbatim from today's `crates/nmp-core/src/display.rs`. The
crate ships:
- A cargo dep (for chirp-tui, future Rust shells).
- A wasm export (for chirp-web).
- A code-generated Swift port via `nmp-codegen` (mirrors the
  `KernelTypes.generated.swift` pattern at
  `ios/Chirp/Chirp/Bridge/Generated/`).
- A code-generated Kotlin port via the same codegen path (when Android
  shell lands).

The codegen is **load-bearing**: hand-porting the djb2 + bech32 + relative-time
algorithms three times is exactly the duplication aim.md §2 was trying to
prevent. The plan does not undo §2's intent — it satisfies it by codegen
instead of by pre-formatting.

### E. New substrate seam: `ProfileSnapshot`

The kernel already holds the profile cache at
`crates/nmp-core/src/kernel/types.rs:58-78` (`Profile { display, picture_url,
nip05, … }`). Today only `TimelineItem` / `ProfileCard` (both inside
`kernel/`) can read it. Under (A), every Layer-4 projection AND every
presentation layer needs read access.

```rust
// crates/nmp-core/src/substrate/profile_snapshot.rs (new)
pub trait ProfileSnapshot: Send + Sync {
    /// Returns the cached metadata for `pubkey_hex`, or `None` if no
    /// kind:0 has arrived yet. Pure read; no side effects.
    fn get(&self, pubkey_hex: &str) -> Option<ProfileDisplay>;
}

#[derive(Clone, Debug, Default)]
pub struct ProfileDisplay {
    pub display_name: String,    // empty when kind:0 has no name field
    pub picture_url: String,     // empty when kind:0 has no picture field
    pub nip05: String,
    pub about: String,
    pub lnurl: Option<String>,
}
```

The kernel implements this trait; the snapshot path serialises the
**entire current profile cache** as one map keyed by hex pubkey on every
tick (today the cache is per-author and embedded inside `TimelineItem` /
`AuthorViewPayload`). Shells read the map once per snapshot and join.

**Performance:** profile cache size today is bounded by the working-set —
order of magnitude is hundreds, not thousands, of pubkeys per session.
Cloning + JSON-serialising on every tick is the same order of cost as the
existing per-`TimelineItem` author block. Measure under
`snapshot_perf_firehose_gate` (`crates/nmp-core/src/kernel/perf_tests.rs`)
before merging.

---

## §5 — Migration order (highest user-visible impact first)

The 11 HIGH-severity fields cluster on two user surfaces: DM list, group
chat. Fix those first; the LOW-severity items either piggy-back or stay.

**Order, with hard dependencies:**

1. **PR 1 — `nmp-display` crate carve-out.** Move `crates/nmp-core/src/display.rs`
   wholesale into `crates/nmp-display/`. `nmp-core::display` becomes
   `pub use nmp_display::*;` for backwards compat. Zero behaviour change.
   **Blocks every later PR; merge first.**
2. **PR 2 — `ProfileSnapshot` substrate seam + serialisation.** New trait
   in `nmp-core::substrate`, kernel impl, JSON-serialised projection
   exposed via FFI. Every shell now has a read path to display_name +
   picture_url keyed by raw pubkey. **Required by PR 3, 4, 5, 6.**
3. **PR 3 — `DmConversation` (nmp-nip17).** Highest visibility: DM list
   shows `npub1abc…xyz` for every peer with no kind:0. Remove 3 fields;
   iOS DmListView (`ios/Chirp/Chirp/Features/DmListView.swift:212-216`)
   joins against `ProfileSnapshot`. TUI consumer at
   `apps/chirp/chirp-tui/src/feature_snapshot.rs:221` updates analogously.
4. **PR 4 — `GroupChatMessage` (nmp-nip29).** Same shape as PR 3.
   ios `GroupChatView.swift` joins; TUI `apps/chirp/chirp-tui/src/timeline.rs:53`
   updates (already falls back to `short_npub`, ironically a model of the
   correct behaviour).
5. **PR 5 — `MarmotMessageRow` + `MarmotGroupRow` + `PendingWelcomeRow`
   (nmp-marmot).** All three live in the same crate; one PR.
6. **PR 6 — `FollowEntry` (nmp-nip02).** Smaller blast radius (one consumer:
   `ios/Chirp/Chirp/Features/DmListView.swift:212` follow picker).
7. **PR 7 — `MentionProfilePayload` (nmp-core::kernel).** The only MEDIUM
   today (pubkey is map key, not struct field). Add `pubkey: String` to the
   struct body so shells consuming a flat array don't lose provenance.
8. **PR 8 — `TimelineItem` + `ProfileCard` + `AccountSummary` LOW-severity
   strip.** Mechanical removal of all `*_display` / `*_initials` /
   `*_color_hex` fields. Largest shell impact (modular timeline, profile
   view, accounts toolbar, compose row, home feed).
9. **PR 9 — D17 doctrine-lint rule** (`crates/nmp-testing/bin/doctrine-lint/rules/d17.rs`).
   Locks the boundary. Must merge last so PRs 1–8 land green.
10. **PR 10 — Codegen Swift port of `nmp-display`.** Generates
    `ios/Chirp/Chirp/Bridge/Generated/Display.generated.swift` from
    the `nmp-display` source via `nmp-codegen`. Replaces the hand-deleted
    Swift helpers from V-22…V-33 with codegen output. **This is the
    aim.md §2 anti-pattern satisfaction step under (A) — without it, the
    plan ships duplicated bech32 / djb2 logic in Swift.**

Total: 10 PRs, ~2 small + 5 medium + 3 large. Calendar: ~1 sprint if
parallelised by separate agents per Layer-4 crate (PRs 3–6 don't conflict).

---

## §6 — Presentation-layer impact (per platform)

### iOS (Swift)

Net add: a Swift package wrapping `nmp-display` (codegen, PR 10). All four
algorithms — `shortNpub`, `avatarInitials`, `avatarColorHex`,
`formatAgoSecs` — return as Swift functions. SwiftUI views call them at
render time instead of binding `*Display` fields.

Specific consumer changes (verified callsites today):
- `ios/Chirp/Chirp/Features/DmListView.swift:190` — `$0.shortNpub.lowercased().contains(q)`
  becomes `Display.shortNpub($0.peerPubkey).lowercased().contains(q)`.
- `ios/Chirp/Chirp/Features/DmListView.swift:212-216` — `initials:
  follow.avatarInitials` and `Text(follow.shortNpub)` become
  `Display.avatarInitials(Profiles.displayName(for: follow.pubkey) ??
  Display.toNpub(follow.pubkey))` and `Text(Display.shortNpub(follow.pubkey))`.
- `ios/Chirp/Chirp/Bridge/MarmotBridge.swift:215-217` — `senderShort`,
  `senderInitials`, `senderColorHex` `CodingKey`s deleted from the
  Decodable.
- `ios/Chirp/Chirp/Bridge/TimelineBlock.swift:183,212,215` — `authorDisplay`,
  `authorAvatarInitials`, `authorDisplayName` fields deleted from
  the Decodable.
- `ios/Chirp/Chirp/Bridge/KernelBridge.swift:953,1230,1231,1784` — every
  `avatarInitials` / `shortNpub` Decodable field deleted; Views look up
  via `Display.*` + `ProfileSnapshot` cache.

Net Swift LOC delta: estimated +120 (new `Display.swift` thin wrapper,
~6 sites that re-derive via cache lookup), –50 (deleted Decodable fields,
deleted Swift-side encoded strings in `GroupChatDecodeTests.swift`). The
codegen `Display.generated.swift` is +250 LOC but mechanical and exempt
from the hand-authored 500-LOC ceiling.

### Android (Kotlin)

Today's Android shell is `android/` — the inventory is post-v1 sized
(`docs/plan.md` M15 says "🟡 Desktop + Android shells"). Same shape as
iOS: codegen Kotlin port of `nmp-display`, Compose views call it directly,
all `*Display` / `*Short` / `*Initials` properties on data classes deleted.
Same delta sign.

### Chirp-TUI (Rust)

Already uses `nmp_core::display::short_npub` directly
(`apps/chirp/chirp-tui/src/timeline.rs:1`). Under (A) it switches the
import to `nmp_display::short_npub` (PR 1 makes that a free rename) and
joins against `ProfileSnapshot` for display name (PR 2). The fallback
pattern at timeline.rs:53-58 (which already does `kind:0 display
.or_else(short_npub)`) is the **model the other shells should follow**.

Net TUI LOC delta: ~+30 (cache lookup helper, two consumer-site rewrites),
near zero deletions (TUI never duplicated the algorithms — that was a
Swift / Kotlin problem only).

### Chirp-Web (TypeScript)

Today consumes `peer_short_npub` etc. via `web/chirp/src/nmp/snapshot.ts:169`.
Under (A), it imports `nmp-display`'s wasm export (PR 1 should already
ship a wasm build alongside the cargo crate — `nmp-display` is a pure-Rust
crate with no I/O, trivially wasm-compatible). Joins against the
`ProfileSnapshot` JSON from PR 2.

Net web LOC delta: +60 (wasm boot + helper imports, four consumer-site
rewrites), –30 (deleted aliasing in `snapshot.ts`).

### Total cross-platform shell delta

Estimated +210 LOC of new code (mostly thin wrappers + cache-join helpers
that didn't need to exist when the kernel pre-formatted), –80 LOC of
deleted Decodable fields and obsolete property aliases. **+250 LOC codegen
on top of that (Swift + Kotlin Display.generated)**, exempt from
hand-authored ceiling.

---

## §7 — Relationship to the profile-fetch plan

`Plans/profile-fetch-plan.md` (Diff 1 — content-mention extractor + Diff 2 —
kind:0 re-fetch on `Nip65Arrived`) closes the **producer** side of the
profile cache: every pubkey the user can see triggers a kind:0 / kind:10002
fetch, and stale cached kind:0 gets refreshed after the author's write
relays are discovered.

This plan closes the **consumer** side: once kind:0 is in
`Kernel::profiles`, the four Layer-4 projections — which today never
observe the cache — gain a path to it (via `ProfileSnapshot`, §4-E) and
their UI rows update in place from `npub1abc…xyz` to `Alice`.

**Sequencing:** profile-fetch is independent and should land first. If
profile-fetch ships without this plan, kind:0 still flows into
`Kernel::profiles` but only `TimelineItem` / `ProfileCard` /
`AccountSummary` consumers benefit; the four Layer-4 projections stay
stale. If this plan ships first, every shell joins against an empty
`ProfileSnapshot` until profile-fetch is wired and kind:0 arrives — same
behaviour as today (the abbreviated npub renders), but with the cache-stale
defect fixed for free as soon as profile-fetch lands.

**Recommended order:** profile-fetch's Diff 1 → Diff 2 → this plan PR 2
(`ProfileSnapshot`) → this plan PRs 3–6 (Layer-4 cleanup) → PRs 7–10. PD-040
must resolve before any of this starts.

---

## §8 — What NOT to do

- **Do not pre-format inside Layer-4 NIP crates and call it cache-integrated.**
  Adding `Arc<dyn ProfileSnapshot>` to `DmInboxProjection::new` and computing
  `peer_display = snapshot.get(pk).map(|p| p.display_name).unwrap_or_else(|| short_npub(pk))`
  inside the projection is **option (B), not (A)**. It works, but it does
  not satisfy the brief; under (A) the field is deleted, not improved.
  **Pick one doctrine and stay there.**
- **Do not leak `nmp_core::display::*` into Layer-4 crates after PR 1.**
  After the carve-out, only `nmp-display` owns those symbols; every other
  crate imports through it. The temporary `nmp-core::display` re-exports
  exist for PR-sequencing only and must be deleted by PR 10. Add this
  deletion as the last step of PR 9 (D17 lint) so the lint enforces it
  forward.
- **Do not duplicate the algorithms by hand in Swift / Kotlin / TS.** The
  V-22…V-33 sweep cited aim.md §2 anti-pattern #1 by name for exactly this
  reason — the same djb2 hash producing a different tint per platform is
  observably worse than no avatar tint at all. The codegen step (PR 10)
  is **non-negotiable**; without it, the plan ships the exact failure
  mode V-25 / V-26 were filed against.
- **Do not introduce per-shell fallback algorithms when `ProfileSnapshot`
  is empty.** Either the shell renders the abbreviated raw pubkey (via
  `nmp-display`, byte-identical across platforms) or it renders nothing
  (placeholder identicon). It must not invent a private `defaultInitials`
  helper, a private `defaultColor` helper, or a private string-pubkey
  abbreviation — those are exactly the violations V-25 / V-27 / V-28
  enumerated and deleted.
- **Do not amend aim.md without an ADR.** `docs/aim.md` declares itself
  cold-start canon; mutating it inline violates planning-discipline. The
  amendment lives in a new ADR (`docs/decisions/00NN-display-separation.md`)
  that aim.md gains a top-of-file pointer to. Per
  `AGENTS.md` "Planning discipline — three canonical files", the ADR is
  the right channel; do not create `docs/plan/display-doctrine.md` or any
  other parallel planning file.
- **Do not stage the deletion behind a feature flag.** "Migration phase"
  feature flags are exactly the "no temporary hacks" violation
  `AGENTS.md` calls non-negotiable. The two doctrines coexisting in the
  tree is the worst possible state — every PR-touched struct must commit
  to one or the other.
- **Do not couple this work to the F-05 codegen pilot.** `nmp-codegen` Swift
  `Decodable` generation (PR #387 merged, more in flight) is orthogonal:
  PR 10 codegens algorithms (functions), F-05 codegens types
  (`Decodable`s). They share the `nmp-codegen` infrastructure but never
  the same output file. Conflating them in one PR doubles review surface.
- **Do not remove `ProfileCard` / `AccountSummary` display fields without
  measuring the snapshot perf gate.** Those fields are read by
  `Kernel::projections` on every tick and embedded in
  `AuthorViewPayload.items` — removing them shifts work from "kernel
  produces formatted strings" to "shell does N lookups against
  `ProfileSnapshot`". Net cost may be lower (no string allocation +
  serialisation in Rust) or higher (FFI hop per lookup); both are
  plausible without measurement. The CI gate
  `snapshot_perf_firehose_gate` must stay green at PR 8 merge.

---

## §9 — Open decision (PD-040 — display-string doctrine)

Logged here for §3 (`docs/BACKLOG.md`) lift-and-shift. The user must pick
**before any branch is cut**:

- **(A)** Reverse aim.md §2 anti-pattern #1 via ADR; backend emits raw
  pubkeys; presentation layer formats via codegen-shared `nmp-display`.
  This plan describes (A).
- **(B)** Preserve aim.md §2; close the four cited HIGH-severity
  cache-stale defects by giving Layer-4 projections a substrate read seam
  onto `Kernel::profiles`. ~80 LOC of net Rust diff; zero shell changes;
  zero codegen work.
- **(C)** Split the difference: backend emits raw pubkeys, but
  `nmp-display` is exposed as host-side Rust helpers consumed via FFI
  symbols (`nmp_display_short_npub` etc.) rather than codegen ports. ~30
  new bespoke FFI symbols (one per algorithm × wrapping convention), which
  collides head-on with the PD-039 deprecation calendar — almost certainly
  the worst of the three.

**Recommendation: (B).** It is the smallest closure of the user-visible
defect, requires no doctrine reversal, and keeps the V-22…V-33 work intact.
The brief argues for (A) on separation-of-concerns grounds; (B) achieves
the same separation by injecting the cache read into projections through
a substrate trait, not by deleting fields. (B) is also the only option
that does not require an aim.md amendment and is therefore the only option
landable inside the current planning-discipline rules.
