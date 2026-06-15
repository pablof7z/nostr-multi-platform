# Chirp iOS — Polish & Craft Checklist

Authoritative design-quality bar for the Chirp iOS client, distilled from an
Apple-HIG-grounded review (codex `gpt-5.5`, 2026-06-15) plus on-device
screenshot audits. The goal: make Chirp feel like a top-tier native app
(Ivory / Tapestry / Apollo / Apple Mail-Messages caliber), not a prototype.

**The single highest-leverage move:** make `NoteRowView` excellent first. If the
timeline reads like Apple Mail/Messages-level native quality, the whole app stops
feeling half-baked.

All visual polish belongs in `ChirpColor`, `ChirpFont`, `ChirpSpace`, and the
component library. Ordering, retry policy, publish/DM/wallet state stay
Rust-owned — SwiftUI only renders. Prefer NMP UI components (`Nostr*` family);
improve them at the component layer rather than patching call sites.

## P0 — Fix First (biggest feel impact)

- [ ] **Native typography, not an invented brand scale.** Base `ChirpFont` on
  Dynamic Type text styles: largeTitle 34 bold, title1 28 bold, title2 22 bold,
  title3 20 semibold, headline 17 semibold, body 17 regular, callout 16,
  subheadline 15, footnote 13, caption 12/11. Feed body 17pt / ~22pt line
  height. Metadata 13–15pt, never tiny 10pt gray. Support Dynamic Type via
  system styles / `UIFontMetrics`; verify at AX3+.
- [ ] **4pt spacing grid.** Screen margins 16pt (phone). Feed row horizontal
  padding 16, vertical 10–12, avatar→content gap 12, media top gap 8, action-bar
  top gap 8. Minimum hit target 44×44pt (icons 18–22pt, hit area still 44).
- [ ] **Kill childish color.** Semantic colors only: primary / secondary /
  tertiary / separator / systemBackground / secondarySystemBackground /
  groupedBackground + ONE restrained accent. No random hex grays, neon purple
  gradients, candy cards, or tinted backgrounds everywhere. Text contrast ≥4.5:1,
  large text/icons ≥3:1.
- [ ] **Feed = professional list, not toy cards.** Default row: avatar 44pt,
  content column starts at x = 16+44+12 = 72pt, name 15–16 semibold,
  handle/timestamp 13–15 regular secondary, body 17 regular, divider 0.5pt inset
  to the content column. No full card background per note — separators + whitespace.
- [ ] **Native navigation.** ≤4–5 root tabs (Home, Notifications, DMs, Wallet,
  Settings/Profile). Native `TabView`, filled SF Symbol variants when selected,
  large titles on root screens, inline titles on detail. Compose is an action
  (`square.and.pencil` in nav bar or restrained floating button), not a tab.

## P1 — Core Social Polish

- [ ] **Feed actions:** `bubble.left`, `arrow.2.squarepath`, `heart`, `bolt`,
  `square.and.arrow.up`, `ellipsis`. 18–20pt symbols, 44pt hit boxes, secondary
  color by default. Active states only: heart red, repost green, zap amber, reply
  accent. Counts 13pt, optically aligned to icons.
- [ ] **Thread view:** parent note full density; replies tighter with 36pt
  avatars; vertical thread rail 1pt using separator color (not bright accent).
- [ ] **Compose:** full-height sheet / pushed composer with native nav bar
  (Cancel left, Post right). Editor 17pt, secondary placeholder, 44pt toolbar,
  clear disabled state, visible sending + remote-signer-pending states, draft
  preservation. No giant rounded text boxes (except DM compose).
- [ ] **Media & avatars:** circular avatars — home 44, notifications 32, thread
  replies 36, profile 88–104. Single image max height ~520pt, radius 12, grid gap
  2, preserve aspect ratio. BlurHash/average-color placeholders, not blank gray.
- [ ] **Empty / loading / error:** skeletons match final row geometry exactly
  (3–5 rows, subtle opacity animation, no goofy illustrations). Empty state icon
  40–48pt, title 17 semibold, body 15 secondary, one action. Errors preserve
  stale content + inline retry banner.

## P2 — Feel & Craft

- [ ] **Motion:** press feedback 80–120ms; row insert/fade 180–220ms;
  expand/collapse 220–280ms; spring response 0.28–0.38, damping 0.82–0.9. Never
  animate every timeline refresh. Respect Reduce Motion. Haptics sparingly:
  selection on tab/segmented change, light impact on like/zap, success
  notification on publish.
- [ ] **SF Symbols only** unless truly necessary. Consistent weight (.regular);
  selected tab icons `.fill`; hierarchical rendering for secondary toolbar icons.
- [ ] **Buttons:** primary 17 semibold, height 44–50, radius 10–12; secondary
  text-only or subtle bordered. Pressed: opacity 0.65–0.8 or scale 0.98 — no
  dramatic bounce. Native/subtle row tap highlight.
- [ ] **Sheets:** focused temporary tasks only; full-screen reserved for
  onboarding/login/camera/immersive media. Relay settings, profile, thread,
  wallet history = pushed navigation. No nested sheets.

## P3 — The 100 Small Things

- [ ] One invisible vertical ruler: every text baseline + leading edge aligned.
- [ ] 0.5pt pixel-aligned hairlines. No fuzzy 1px custom borders.
- [ ] One radius scale: 8 small controls, 10–12 media/cards, 16 sheets/panels.
- [ ] No nested cards, heavy shadows, decorative gradients, or badge spam.
- [ ] Preserve scroll position across refreshes; no feed jumps.
- [ ] Tab bar visible during normal drill-down; hide only for modal/immersive.
- [ ] Context menus on notes: reply, repost, zap, copy note ID, mute, report.
- [ ] VoiceOver labels for every icon-only action; correct reading order.
- [ ] Dark mode ≠ inverted light mode: audit separators, placeholders, disabled
  states, media overlays.
