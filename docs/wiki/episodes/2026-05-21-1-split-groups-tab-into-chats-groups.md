---
type: episode-card
date: 2026-05-21
session: eb342a0d-84e3-4289-9873-88a947ca8144
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/eb342a0d-84e3-4289-9873-88a947ca8144.jsonl
salience: product
status: active
subjects:
  - groups-tab-ux
  - tab-navigation
  - dm-inbox
  - group-creation
supersedes: []
related_claims: []
source_lines:
  - 1-5
  - 82-98
  - 138-141
  - 214-251
  - 340-368
  - 718-753
  - 776-778
  - 1127-1273
captured_at: 2026-06-18T04:45:23Z
---

# Episode: Split Groups tab into Chats + Groups; encryption as badge, not section

## Prior State

The Groups tab mixed DMs, NIP-29 groups, pending invites, and MLS groups into one view organized by protocol internals (NIP numbers, encryption status as section dividers). The tab icon was lock.shield.fill. DM compose accepted raw hex pubkeys. Group creation used raw npub TextEditor. The Search tab occupied the slot where DMs should live.

## Trigger

User explicitly flagged the mixed UX: 'Groups are groups. DMs are DMs. Whether groups are encrypted or not shouldn't be part of how we list things.' Requested a full UX rethink via an Opus agent review.

## Decision

Tab bar restructured from Home·Search·Groups·Wallet·Settings to Home·Chats·Groups·Wallet·Settings (Search moves to Home nav toolbar). Groups list becomes a single flat list sorted by last activity, with a shield badge for encrypted groups and a # glyph for public groups — no protocol-named sections. DMs promoted to their own 'Chats' tab. Pending invites collapsed into a chip at the top of Groups. Group creation sheet uses a Private/Public segmented toggle with a contact picker, not raw npub fields. All NIP numbers, wire-protocol terms, and hex pubkeys removed from production UI. DM compose replaced hex TextField with a searchable contact picker backed by the NIP-02 follow list. DmConversation enriched with Rust-computed display fields (peerNpub, peerShortNpub, peerAvatarInitials, peerAvatarColor) per the thin-shell doctrine. A new FollowListProjection (Rust KernelEventObserver for kind:3) was created as the data source for the contact picker.

## Consequences

- ChirpTab enum changed: search case removed, chats case added; RootShell restructured accordingly
- MarmotGroupsView renamed to GroupsView; DM section, NIP-29 section, and protocol-vocabulary footers all removed
- New ChatsView.swift (thin tab root) and InvitesView.swift (dedicated invite screen) created
- DmConversation struct gained 4 required display fields computed in Rust (nmp-nip17/src/display.rs)
- New FollowListProjection + FollowListBridge + FollowListStore wired across Rust/FFI/Swift
- Search functionality temporarily has no tab until Home toolbar button is implemented
- Public (NIP-29) group creation is disabled in the UI with 'Coming soon' label
- DM peer display names still fall back to short npub (not profile names) until Rust side provides a peerDisplayName field
- Contact picker currently searches follow list only; npub paste, QR scan, and recent-DM shortcuts are UI stubs

## Open Tail

- Home nav toolbar magnifying-glass button for SearchView not yet implemented
- NIP-29 group creation action not yet wired (UI shows 'Coming soon')
- Rust-side peerDisplayName for DM conversation rows still needed for full profile-aware display

## Evidence

- transcript lines 1-5
- transcript lines 82-98
- transcript lines 138-141
- transcript lines 214-251
- transcript lines 340-368
- transcript lines 718-753
- transcript lines 776-778
- transcript lines 1127-1273

