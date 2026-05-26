# ADR-0032 — Backend sends raw data; presentation layers format

- **Status:** Accepted (2026-05-25)
- **Supersedes:** the prior `aim.md` §6 anti-pattern prescription
  "Duplicated formatting logic across platforms — Rust pre-formats into
  strings, native renders them" (in effect until commit
  [`ec8decade75c31d1`](../../) doctrine: backend sends raw data, presentation
  layers format).
- **Scope:** every projection / snapshot / FFI type emitted by the kernel
  and the Layer-4 NIP crates (`nmp-core`, `nmp-nip01`, `nmp-nip02`,
  `nmp-nip17`, `nmp-nip29`, `nmp-marmot`).

## Context

The prior doctrine — encoded in `aim.md` §6 anti-pattern #1 and merged
through the V-22 … V-33 series of "thin-shell" PRs between 2026-05-23
and 2026-05-24 — said:

> Rust pre-formats into strings; native renders them.

That rule moved every avatar-tile initial, avatar-tint hex, abbreviated
npub, abbreviated event id, and relative-time label out of the Swift /
Kotlin / Web views and into the kernel snapshot. The intent was to
avoid platforms re-implementing the same algorithm three or four times.

The rule had two structural problems that surfaced in practice:

1. **Cache staleness.** The four Layer-4 projections (`nmp-nip17::DmConversation`,
   `nmp-nip29::GroupChatMessage`, `nmp-marmot::MarmotMessageRow`,
   `nmp-nip02::FollowEntry`) lived in crates that had no read path to
   the kernel's kind:0 profile cache. They computed display strings at
   ingest time using only the raw pubkey — those strings could never
   improve when a later kind:0 arrived, so DM rows and group-chat rows
   showed `npub1abc…xyz` *forever* even after the user's display name
   loaded and rendered correctly elsewhere.
2. **Policy in the wrong place.** Many of the pre-formatted strings
   embedded policy a host application should be free to override —
   *which* abbreviation algorithm to use for a pubkey (8+8 vs 10+6),
   *what* to show when kind:0 is absent (npub fallback vs identicon vs
   blank tile), *how* to bucket the relative-time labels (`"3m"` vs
   `"3 min"` vs locale-aware), *whether* to substitute a placeholder
   identicon URI for a missing picture. Different apps on top of NMP
   have legitimately different answers; the framework should not
   pre-commit them.

## Decision

**NMP is a data framework. Projections and snapshots send raw protocol
data only. Presentation layers (Swift, Kotlin, TypeScript, TUI) own all
formatting decisions.**

"Raw" means:

- **Pubkeys** as 64-char lowercase hex strings.
- **Timestamps** as Unix `u64` integers (seconds).
- **Counts** as raw `u32` / `u64` integers (no `"3 members"` /
  `"12,345 sats"` pluralisation).
- **Display names** verbatim from kind:0 (`display_name` →
  `displayName` → `name`, first non-empty wins). When kind:0 is absent
  or carries none of those fields, the field is **absent**
  (`Option<String> = None`). NMP does NOT substitute `short_npub`.
- **Picture URLs** verbatim from kind:0 `picture` (subject to a
  `starts_with("http")` filter at parse time). When the field is absent
  the projection emits `None` — no `identicon:<hex>` placeholder URI is
  substituted.

The following `nmp_core::display::*` helpers and their forwarders are
**banned** from projection builders, snapshot types, and FFI
serialization paths:

- `display::short_npub`
- `display::avatar_initials`
- `display::avatar_color_hex`
- `display::format_ago_secs`
- `display::to_npub`
- `display::short_hex`
- `display::display_name_initials`

The helpers themselves stay in `nmp_core::display` — they are
**legitimate** in:

- TUI render code (`apps/chirp/chirp-tui/src/`, `crates/nmp-desktop/src/`).
- CLI / REPL output (`crates/chirp-repl/src/`, `crates/nmp-repl/src/`).
- `#[cfg(test)]` blocks and `tests/` integration tests.

Free-form metadata fallbacks (e.g. "Untitled group" when the MLS group
`name` is empty, the 2-char initials extracted from a group `name`,
the pluralised invite-chip label `"3 invites"`) remain in the
projection layer — they are protocol-level decisions about how to
surface a *name* field that has no kind-defined empty-string semantics,
not banned `display::*` forwarders.

## What changed

### Removed fields (by struct)

**`nmp-nip17::DmConversation`** (`crates/nmp-nip17/src/inbox.rs`):
- `peer_npub` (was `display::to_npub`)
- `peer_short_npub` (was `display::short_npub`)
- `peer_avatar_initials` (was `display::avatar_initials`)
- `peer_avatar_color` (was `display::avatar_color_hex`)

**`nmp-nip17::DmMessage`**:
- `created_at_display` (was `display::format_ago_secs`)

**`nmp-nip29::GroupChatMessage`** (`crates/nmp-nip29/src/projection/group_chat.rs`):
- `author_display`, `author_initials`, `author_color_hex`
- `created_at_display`

**`nmp-nip29::GroupChatSnapshot`**:
- `group_initials` (free-form derivation, but the task scope cited it
  as a `*_initials` shadow forwarder of `display::*`).

**`nmp-marmot::MarmotGroupRow`** (`crates/nmp-marmot/src/projection/payload.rs`):
- `members_display: Vec<String>` (was `display::short_npub`)
- `member_count_display: String` (pluralised count)
- `unread_display: Option<String>` (formatted count)
- Added `member_count: u32` and `unread_count: Option<u32>` as raw
  replacements (renamed-and-shape-changed from `unread: u64`).

**`nmp-marmot::PendingWelcomeRow`**:
- `inviter_short` (was `display::short_npub`)

**`nmp-marmot::MarmotMessageRow`**:
- `sender_short`, `sender_initials`, `sender_color_hex`,
  `created_at_display`
- Renamed `sender_npub: String` → `sender_pubkey_hex: String` (the
  value was always hex despite the legacy field name; verified at
  `ops.rs:267` via `m.pubkey.to_hex()`).

**`nmp-marmot` ops envelope** (`crates/nmp-marmot/src/projection/ops.rs`):
- `missing_key_package_result` ships `needs_pubkeys_hex: Vec<String>`
  instead of `needs_display: Vec<String>` (the abbreviated-npub error
  string).

**`nmp-nip02::FollowEntry`** (`crates/nmp-nip02/src/projection.rs`):
- `npub`, `short_npub`, `avatar_initials`, `avatar_color`
- Only the raw hex `pubkey` field remains.

**`nmp-nip01::TimelineEventCard`** (`crates/nmp-nip01/src/timeline_projection.rs`):
- `author_avatar_initials`, `author_avatar_color`,
  `author_pubkey_short`, `short_id`, `created_at_display`
- `author_display_name: String` → `author_display_name: Option<String>`
- `author_picture_url: String` → `author_picture_url: Option<String>`

**`nmp-nip01::AuthorDisplay`** (`crates/nmp-nip01/src/profile_display.rs`):
- `name: String` → `name: Option<String>`
- `picture_url: String` → `picture_url: Option<String>`
- `npub: String` → `npub: Option<String>` (the bech32 encoding is
  pubkey-deterministic; `None` only when the raw hex cannot be parsed)
- `AuthorDisplaySource` enum deleted (`name.is_some()` is the
  authoritative "have we seen kind:0?" signal).

**`nmp-nip01::ProfileDisplay`**:
- `display: String` → `display: Option<String>` (no `short_npub`
  fallback when kind:0 omits all name fields).

**`nmp-core::TimelineItem`** (`crates/nmp-core/src/kernel/types.rs` +
`crates/nmp-core/src/kernel/update.rs`):
- `author_display`, `author_avatar_initials`, `author_avatar_color`,
  `author_avatar_source`, `author_pubkey_short`, `short_id`,
  `created_at_display`
- `author_picture_url: String` → `author_picture_url: Option<String>`
  (no identicon placeholder substituted)
- `created_at_display: String` replaced with `created_at: u64` (raw
  Unix seconds).

**`nmp-core::ProfileCard`**:
- `npub_short`, `avatar_initials`, `avatar_color`, `source` deleted
- `display: String` → `display_name: Option<String>`
- `picture_url: String` → `picture_url: Option<String>`
- `has_profile: bool` kept (the task scope retained it; the field is
  derivable from `display_name.is_some() || picture_url.is_some() ||
  !nip05.is_empty()` and may be revisited in a follow-up).

**`nmp-core::MentionProfilePayload`**:
- Added `pubkey: String` (raw hex) to the struct body so shells flowing
  the projection through a flat JSON array do not lose provenance.
- Deleted `avatar_initials`, `avatar_color`.
- `display: String` → `display_name: Option<String>`.
- `picture_url: String` → `picture_url: Option<String>`.
- `mention_profiles_from_items` rewritten to join against the
  kernel's profile cache directly (was sourcing from the
  now-deleted `TimelineItem` fields).

**`nmp-core::AccountSummary`** (`crates/nmp-core/src/kernel/identity_state.rs`
+ `crates/nmp-core/src/actor/commands/identity.rs`):
- `npub_short`, `avatar_initials`, `avatar_color_hex` deleted.
- `display_name: String` → `display_name: Option<String>` (no
  `<first6>…<last4>` hex placeholder fallback).
- Helpers `account_npub_short`, `account_avatar_initials`,
  `account_avatar_color_hex`, and `display_name_from_hex` deleted.

**`nmp-core::WalletStatus`** (`crates/nmp-core/src/actor/commands/wallet.rs`):
- Added `wallet_pubkey_hex: String` (raw hex extracted from the
  private `WalletConnection.wallet_pubkey_hex`).
- `wallet_npub_short: String` deleted.
- `balance_sats_display: Option<String>` deleted; the raw
  `balance_sats: Option<u64>` stays.
- Helper `format_sats_display` deleted.

**`nmp-core::kernel::types::Profile` cache**:
- `avatar_initials: String` deleted.
- `avatar_color: String` deleted.
- `display: String` kept as the verbatim kind:0 value (empty string
  when absent) — the conversion to `Option<String>` happens at the
  projection boundary (`ProfileCard.display_name`,
  `TimelineEventCard.author_display_name`, etc.).
- `parse_profile` no longer falls back to `short_npub(pubkey)` when
  the parsed metadata carries none of `display_name` / `displayName` /
  `name`.

### Doctrine documentation

- `docs/aim.md` §6 anti-pattern #1 was rewritten in commit
  [`ec8decad`](../../) ("doctrine: backend sends raw data, presentation
  layers format") and now reads:

  > NMP is a data framework. Projections and snapshots send raw
  > protocol data only. Presentation layers own all formatting
  > decisions.

- This ADR documents the *implementation* of that doctrine update.

## Migration guidance for existing shell consumers

Where a shell read a deleted field, it must either:

1. **Compute the formatted value locally at render time** from the
   raw replacement (`author_pubkey`, `created_at`, etc.). The simplest
   abbreviation policy is pure string slicing on the hex:
   ```swift
   // Swift
   func shortPubkey(_ hex: String) -> String {
       guard hex.count >= 16 else { return hex }
       return "\(hex.prefix(8))…\(hex.suffix(8))"
   }
   ```
2. **Join the raw pubkey against `mention_profiles`** (or the matching
   per-feature projection) for the kind:0-derived display name and
   picture URL, falling back to its own short-pubkey rendering when
   the joined entry is null.

Rust shells (`chirp-tui`, `nmp-desktop`) may call
`nmp_core::display::*` helpers directly — those helpers stay in place
for exactly this purpose.

Non-Rust shells (iOS Swift, Android Kotlin, TypeScript) ship their
own thin display helpers in this PR. A subsequent ADR may consolidate
those helpers into a shared `nmp-display` crate with codegen ports,
but that is **out of scope** for this change.

## What this ADR does *not* do

- It does not introduce a new `nmp-display` Layer-0 crate or a doctrine-lint
  rule for banned display helpers in projections. Those land in a follow-up if
  the user decides the cross-platform algorithm-duplication cost is worth the
  additional crate / codegen surface.
- It does not touch the `nmp-marmot` `MarmotGroupRow::display_name`
  fallback (`"Untitled group"` when `name` is empty), the
  `MarmotGroupRow::initials` 2-char extractor, or
  `PendingWelcomeRow::display_name`. Those are free-form metadata
  fallbacks for a `name` field, not banned-helper forwarders, and the
  task scope did not enumerate them for deletion. A follow-up may
  re-evaluate whether free-form fallbacks belong in NMP or the shell.
- It does not generate Swift / Kotlin codegen for the formatting
  helpers. Cross-platform algorithm consistency is a separate
  concern — `nmp_core::display` remains the canonical algorithm
  reference for Rust callers, and the spec-conformant ports for
  Swift / Kotlin / TypeScript are an explicit follow-up.

## References

- `docs/aim.md` §2 (post-`ec8decad`) — the current canonical doctrine
  statement.
- Commit `ec8decad` — `doctrine: backend sends raw data, presentation
  layers format`.
